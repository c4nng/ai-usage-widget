import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import { applyAppearance } from "./appearance";
import { escapeHtml } from "./format";
import { t, type MessageKey } from "./i18n";
import { GLYPHS, ICONS, pixel } from "./pixel";
import { isApiKind, PROVIDER_KINDS, type Config, type Placement, type ProviderConfig, type ProviderKind } from "./types";
import "./styles/base.css";
import "./styles/settings.css";

const REFRESH_CHOICES = [2, 5, 10, 15, 30, 60];
const DEFAULT_LABELS: Record<ProviderKind, string> = {
  claude_subscription: "Claude",
  codex_subscription: "ChatGPT · Codex",
  anthropic_api: "Anthropic API",
  openai_api: "OpenAI API",
};

/** Tiny pictures of the two placements. */
const PLACEMENT_ART: Record<Placement, string[]> = {
  edge: [
    "GGGGGGGWWWGGGG",
    "G......WWW...G",
    "G.......W....G",
    "G............G",
    "G............G",
    "G............G",
    "GGGGGGGGGGGGGG",
  ],
  desktop: [
    "GGGGGGGGGGGGGG",
    "G.WWWWWW.....G",
    "G.W....W.....G",
    "G.W....W.GGG.G",
    "G.WWWWWW.GGG.G",
    "G............G",
    "GGGGGGGGGGGGGG",
  ],
};

const page = document.getElementById("page")!;

let draft: Config;
let addKind: ProviderKind = "claude_subscription";
let openId: string | null = null;
/** Field name of the dropdown that is currently open. */
let openSelect: string | null = null;
/** Typed but unsaved API keys, by provider id. */
const keyDrafts = new Map<string, string>();
const savedKeys = new Set<string>();
const forgetKeys = new Set<string>();
const removedIds = new Set<string>();
const defaultPaths = new Map<ProviderKind, string>();
let status: { text: string; error: boolean } | null = null;

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const attr = (value: unknown) => escapeHtml(String(value ?? ""));
const find = (id: string) => draft.providers.find((p) => p.id === id);

/** A pixel-styled dropdown; native <select> popups ignore the theme on Windows. */
function dropdown(field: string, value: string, options: [string, string][]): string {
  const current = options.find(([v]) => v === value)?.[1] ?? value;
  const isOpen = openSelect === field;
  const list = isOpen
    ? `<ul class="dropdown-list" role="listbox">
        ${options
          .map(
            ([v, label]) =>
              `<li role="option" aria-selected="${v === value}" data-action="pick" data-field="${field}" data-value="${attr(v)}">${escapeHtml(label)}</li>`,
          )
          .join("")}
      </ul>`
    : "";
  return `
    <div class="dropdown ${isOpen ? "is-open" : ""}">
      <button type="button" class="dropdown-button" data-action="dropdown" data-field="${field}" aria-haspopup="listbox" aria-expanded="${isOpen}">
        <span>${escapeHtml(current)}</span>${pixel(ICONS.chevron, "chevron")}
      </button>
      ${list}
    </div>`;
}

function placementCard(value: Placement): string {
  const selected = draft.placement === value;
  return `
    <button type="button" class="choice ${selected ? "is-selected" : ""}" data-action="pick" data-field="placement" data-value="${value}" aria-pressed="${selected}">
      ${pixel(PLACEMENT_ART[value], "choice-art")}
      <span class="choice-title">${t(`settings.placement.${value}`)}</span>
      <span class="choice-hint">${t(`settings.placement.${value}.hint`)}</span>
    </button>`;
}

function render() {
  const scrollY = window.scrollY;
  document.title = `${t("settings.title")} · AI Usage Widget`;
  page.innerHTML = `
    <main class="content">
      <h1>${pixel(ICONS.mark)}${t("settings.title")}</h1>

      <section>
        <h2>${t("settings.providers")}</h2>
        <div class="providers">
          ${draft.providers.length ? draft.providers.map(renderProvider).join("") : `<p class="none">${t("settings.none")}</p>`}
        </div>
        <div class="add-row">
          ${dropdown("add-kind", addKind, PROVIDER_KINDS.map((k) => [k, t(`kind.${k}` as MessageKey)]))}
          <button class="button" data-action="add">${t("settings.add")}</button>
        </div>
        <p class="hint">${t(`kind.${addKind}.hint` as MessageKey)}</p>
      </section>

      <section>
        <h2>${t("settings.placement")}</h2>
        <div class="choices">${placementCard("edge")}${placementCard("desktop")}</div>
        <label class="check layer-toggle">
          <input type="checkbox" data-field="aboveWindows" ${draft.aboveWindows ? "checked" : ""} />
          <span>${t("settings.above")}</span>
        </label>
        <p class="hint">${t("settings.above.hint")}</p>
      </section>

      <section>
        <h2>${t("settings.widget")}</h2>
        <div class="form">
          <div class="field">
            <span>${t("settings.refresh")}</span>
            ${dropdown("refreshMinutes", String(draft.refreshMinutes), REFRESH_CHOICES.map((n) => [String(n), t("settings.minutes", { n })]))}
            <small>${t("settings.refresh.hint")}</small>
          </div>
          <div class="field">
            <span>${t("settings.theme")}</span>
            ${dropdown("theme", draft.theme, (["auto", "dark", "light"] as const).map((v) => [v, t(`settings.theme.${v}`)]))}
          </div>
          <label class="field">
            <span>${t("settings.opacity")} <output>${Math.round(draft.opacity * 100)}%</output></span>
            <input type="range" min="40" max="100" step="2" value="${Math.round(draft.opacity * 100)}" data-field="opacity" />
          </label>
          <label class="check">
            <input type="checkbox" data-field="autostart" ${draft.autostart ? "checked" : ""} />
            <span>${t("settings.autostart")}</span>
          </label>
        </div>
      </section>

      <p class="privacy">${t("settings.privacy")}</p>
    </main>

    <footer class="actions">
      <span class="status ${status?.error ? "is-error" : ""}" role="status">${status ? escapeHtml(status.text) : ""}</span>
      <button class="button" data-action="close">${t("settings.close")}</button>
      <button class="button primary" data-action="save">${t("settings.save")}</button>
    </footer>`;
  window.scrollTo(0, scrollY);
}

function renderProvider(p: ProviderConfig, index: number): string {
  const open = openId === p.id;
  const usesKey = isApiKind(p.kind);
  const kindName = t(`kind.${p.kind}` as MessageKey);
  const hasKey = savedKeys.has(p.id) && !forgetKeys.has(p.id);

  const specific = usesKey
    ? `
      <label class="field">
        <span>${t("settings.apiKey")}</span>
        <input type="password" autocomplete="off" spellcheck="false" data-field="apiKey" data-id="${attr(p.id)}"
          value="${attr(keyDrafts.get(p.id) ?? "")}" placeholder="${attr(hasKey ? t("settings.apiKey.saved") : p.kind === "anthropic_api" ? "sk-ant-admin01-…" : "sk-admin-…")}" />
      </label>
      ${hasKey ? `<button class="link small" data-action="forget-key" data-id="${attr(p.id)}">${t("settings.apiKey.remove")}</button>` : ""}
      <label class="field">
        <span>${t("settings.budget")}</span>
        <input type="number" min="0" step="1" inputmode="decimal" data-field="monthlyBudget" data-id="${attr(p.id)}" value="${attr(p.monthlyBudget ?? "")}" placeholder="—" />
        <small>${t("settings.budget.hint")}</small>
      </label>`
    : `
      <label class="field">
        <span>${t("settings.credentials")}</span>
        <input type="text" spellcheck="false" data-field="credentialsPath" data-id="${attr(p.id)}"
          value="${attr(p.credentialsPath ?? "")}" placeholder="${attr(defaultPaths.get(p.kind) ?? "")}" />
        <small>${escapeHtml(t("settings.credentials.hint"))}</small>
      </label>`;

  return `
    <article class="provider ${open ? "is-open" : ""} ${p.enabled ? "" : "is-disabled"}">
      <div class="provider-head">
        <button class="provider-title" data-action="toggle-open" data-id="${attr(p.id)}" aria-expanded="${open}">
          ${pixel(GLYPHS[p.kind], "glyph")}
          <span class="provider-name">${escapeHtml(p.label || kindName)}</span>
          <span class="provider-kind">${escapeHtml(kindName)}</span>
        </button>
        <button class="icon-button" data-action="up" data-id="${attr(p.id)}" title="${attr(t("settings.moveUp"))}" ${index === 0 ? "disabled" : ""}>${pixel(ICONS.up)}</button>
        <button class="icon-button" data-action="down" data-id="${attr(p.id)}" title="${attr(t("settings.moveDown"))}" ${index === draft.providers.length - 1 ? "disabled" : ""}>${pixel(ICONS.down)}</button>
        <button class="icon-button" data-action="remove" data-id="${attr(p.id)}" title="${attr(t("settings.remove"))}">${pixel(ICONS.remove)}</button>
      </div>
      ${
        open
          ? `<div class="provider-body">
              <p class="hint">${t(`kind.${p.kind}.hint` as MessageKey)}</p>
              <label class="field">
                <span>${t("settings.name")}</span>
                <input type="text" data-field="label" data-id="${attr(p.id)}" value="${attr(p.label)}" maxlength="40" />
              </label>
              <label class="check">
                <input type="checkbox" data-field="enabled" data-id="${attr(p.id)}" ${p.enabled ? "checked" : ""} />
                <span>${t("settings.enabled")}</span>
              </label>
              ${specific}
            </div>`
          : ""
      }
    </article>`;
}

function addProvider() {
  const id = `${addKind}-${crypto.randomUUID().slice(0, 8)}`;
  draft.providers.push({ id, kind: addKind, label: DEFAULT_LABELS[addKind], enabled: true, credentialsPath: null, monthlyBudget: null });
  openId = id;
}

function move(id: string, delta: number) {
  const i = draft.providers.findIndex((p) => p.id === id);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= draft.providers.length) return;
  [draft.providers[i], draft.providers[j]] = [draft.providers[j], draft.providers[i]];
}

/** Values chosen from dropdowns and the placement cards. */
function pick(field: string, value: string) {
  switch (field) {
    case "add-kind":
      addKind = value as ProviderKind;
      break;
    case "refreshMinutes":
      draft.refreshMinutes = Number(value);
      break;
    case "theme":
      draft.theme = value as Config["theme"];
      applyAppearance(draft);
      break;
    case "placement":
      draft.placement = value as Placement;
      break;
  }
}

async function save() {
  const badBudget = draft.providers.some(
    (p) => p.monthlyBudget != null && (!Number.isFinite(p.monthlyBudget) || p.monthlyBudget <= 0),
  );
  if (badBudget) {
    status = { text: t("settings.error.budget"), error: true };
    return render();
  }

  try {
    for (const id of removedIds) await api.deleteSecret(id);
    for (const id of forgetKeys) if (!keyDrafts.get(id)?.trim()) await api.deleteSecret(id);
    for (const [id, key] of keyDrafts) if (key.trim() && find(id)) await api.setSecret(id, key.trim());

    const saved = await api.saveConfig(draft);
    draft = clone(saved);
    removedIds.clear();
    forgetKeys.clear();
    keyDrafts.clear();
    await loadSavedKeys();
    applyAppearance(saved);
    status = { text: t("settings.saved"), error: false };
  } catch (e) {
    status = { text: String(e), error: true };
  }
  render();
}

async function loadSavedKeys() {
  savedKeys.clear();
  for (const p of draft.providers.filter((p) => isApiKind(p.kind))) {
    if (await api.hasSecret(p.id).catch(() => false)) savedKeys.add(p.id);
  }
}

page.addEventListener("click", (event) => {
  const el = (event.target as HTMLElement).closest<HTMLElement>("[data-action]");
  // Any click outside an open dropdown closes it.
  if (openSelect && !(event.target as HTMLElement).closest(".dropdown")) {
    openSelect = null;
    if (!el) return render();
  }
  if (!el || (el as HTMLButtonElement).disabled) return;
  const id = el.dataset.id ?? "";
  switch (el.dataset.action) {
    case "dropdown":
      openSelect = openSelect === el.dataset.field ? null : el.dataset.field!;
      return render();
    case "pick":
      pick(el.dataset.field!, el.dataset.value!);
      openSelect = null;
      break;
    case "add":
      addProvider();
      break;
    case "toggle-open":
      openId = openId === id ? null : id;
      break;
    case "up":
      move(id, -1);
      break;
    case "down":
      move(id, 1);
      break;
    case "remove":
      draft.providers = draft.providers.filter((p) => p.id !== id);
      removedIds.add(id);
      keyDrafts.delete(id);
      break;
    case "forget-key":
      forgetKeys.add(id);
      keyDrafts.delete(id);
      break;
    case "save":
      void save();
      return;
    case "close":
      void getCurrentWindow().close();
      return;
    default:
      return;
  }
  status = null;
  render();
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && openSelect) {
    openSelect = null;
    render();
  }
});

// Text inputs update the draft in place without re-rendering, so focus and caret survive.
page.addEventListener("input", (event) => {
  const el = event.target as HTMLInputElement;
  const field = el.dataset.field;
  if (!field) return;
  const provider = el.dataset.id ? find(el.dataset.id) : undefined;

  switch (field) {
    case "label":
      if (!provider) return;
      provider.label = el.value;
      el.closest(".provider")!.querySelector(".provider-name")!.textContent = el.value || t(`kind.${provider.kind}` as MessageKey);
      return;
    case "credentialsPath":
      if (provider) provider.credentialsPath = el.value.trim() || null;
      return;
    case "apiKey":
      if (provider) keyDrafts.set(provider.id, el.value);
      return;
    case "monthlyBudget":
      if (provider) provider.monthlyBudget = el.value.trim() === "" ? null : Number(el.value);
      return;
    case "enabled":
      if (!provider) return;
      provider.enabled = el.checked;
      el.closest(".provider")!.classList.toggle("is-disabled", !el.checked);
      return;
    case "opacity":
      draft.opacity = Number(el.value) / 100;
      el.closest(".field")!.querySelector("output")!.textContent = `${el.value}%`;
      return;
    case "autostart":
      draft.autostart = el.checked;
      return;
    case "aboveWindows":
      draft.aboveWindows = el.checked;
      return;
  }
});

async function main() {
  const config = await api.getConfig();
  draft = clone(config);
  applyAppearance(config);
  for (const kind of ["claude_subscription", "codex_subscription"] as const) {
    const path = await api.defaultCredentialsPath(kind).catch(() => null);
    if (path) defaultPaths.set(kind, path);
  }
  await loadSavedKeys();
  render();
}

main().catch((e) => {
  page.textContent = String(e);
});
