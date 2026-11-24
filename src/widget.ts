import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, EVENTS } from "./api";
import { applyAppearance } from "./appearance";
import { clockTime, duration, escapeHtml, usd } from "./format";
import { t, tMaybe, type MessageKey } from "./i18n";
import { cells, GLYPHS, ICONS, pixel } from "./pixel";
import type { Config, ErrorCode, Meter, Placement, ProviderReport } from "./types";
import "./styles/base.css";
import "./styles/widget.css";

/** Panel width plus room for the 3px pixel shadow. */
const PANEL_W = 268;
/** Collapsed drop size. Mirrors `DROP_SIZE` in src-tauri/src/lib.rs. */
const DROP = { w: 64, h: 26 };
const CELLS = 28;
const HOVER_OPEN_MS = 90;
const LEAVE_CLOSE_MS = 320;
/** Length of the closing animation in widget.css. */
const CLOSE_ANIM_MS = 190;
/** Errors the user fixes in Settings rather than something being broken. */
const SETUP: ErrorCode[] = ["not_configured", "credentials_missing"];
const OPEN_KEY = "widget.open";

const win = getCurrentWindow();
const shell = document.getElementById("shell")!;
const note = document.getElementById("app")!;
const rows = document.getElementById("rows")!;
const drop = document.getElementById("drop") as HTMLButtonElement;
const top = document.getElementById("top")!;
const refreshButton = document.getElementById("refresh") as HTMLButtonElement;
const pinButton = document.getElementById("pin") as HTMLButtonElement;

let config: Config;
let placement: Placement = "edge";
let reports: ProviderReport[] = [];
/** Fetching is done by the Rust scheduler; this only tracks the manual refresh button. */
let busy = false;
/** Edge mode: the window is panel-sized (open or animating closed). */
let expanded = false;
/** Edge mode: stays open when the pointer leaves. */
let pinned = false;
let hoverTimer: number | undefined;
let closeTimer: number | undefined;
const openRows = new Set<string>(readOpenRows());

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function readOpenRows(): string[] {
  try {
    return JSON.parse(localStorage.getItem(OPEN_KEY) ?? "[]");
  } catch {
    return [];
  }
}

function writeOpenRows() {
  try {
    localStorage.setItem(OPEN_KEY, JSON.stringify([...openRows]));
  } catch {
    // Only a convenience; the widget works without it.
  }
}

// ---------------------------------------------------------------------------
// Data

async function refresh() {
  if (busy) return;
  busy = true;
  refreshButton.classList.add("busy");
  try {
    reports = await api.refreshNow();
  } catch (e) {
    console.error("refresh_now failed", e);
  } finally {
    busy = false;
    refreshButton.classList.remove("busy");
    render();
  }
}

const updatedAt = () => (reports.length ? Math.max(...reports.map((r) => r.fetchedAt)) * 1000 : null);

/** The meter that matters most right now: the fullest window, or the spend. */
function headline(meters: Meter[]): Meter | undefined {
  return [...meters].sort((a, b) => (b.percent ?? -1) - (a.percent ?? -1))[0];
}

function valueText(meter: Meter): string {
  if (meter.spentUsd != null) return usd(meter.spentUsd);
  return `${Math.round(meter.percent ?? 0)}%`;
}

// ---------------------------------------------------------------------------
// Rendering

function render() {
  const updated = updatedAt();
  document.getElementById("mark")!.title = updated ? t("status.updated", { time: clockTime(updated) }) : t("app.title");
  refreshButton.title = t("action.refresh");
  document.getElementById("settings")!.title = t("action.settings");
  document.getElementById("hide")!.title = t("action.hide");
  pinButton.title = pinned ? t("action.unpin") : t("action.pin");
  pinButton.innerHTML = pixel(pinned ? ICONS.pinned : ICONS.pin);
  pinButton.classList.toggle("is-on", pinned);

  const enabled = config.providers.filter((p) => p.enabled);
  if (enabled.length === 0) {
    rows.innerHTML = `<button class="quiet link" data-action="settings">${t("empty.cta")} →</button>`;
  } else if (reports.length === 0) {
    rows.innerHTML = `<p class="quiet">${t("status.loading")}</p>`;
  } else {
    rows.innerHTML = reports.map(renderRow).join("");
  }
  renderDrop(enabled.length);
  requestAnimationFrame(() => void layout());
}

/** The collapsed drop shows the single most pressing number. */
function renderDrop(enabledCount: number) {
  const tops = reports.map((r) => headline(r.usage?.meters ?? [])).filter((m): m is Meter => m?.percent != null);
  const worst = tops.sort((a, b) => (b.percent ?? 0) - (a.percent ?? 0))[0];
  const value = document.getElementById("drop-value")!;
  const mini = document.getElementById("drop-cells")!;

  if (enabledCount === 0) {
    value.textContent = "+";
    mini.innerHTML = "";
  } else if (!worst) {
    value.textContent = "··";
    mini.innerHTML = cells(0, 4, "drop-meter");
  } else {
    value.textContent = `${Math.round(worst.percent!)}%`;
    mini.innerHTML = cells(worst.percent, 4, "drop-meter");
  }
  drop.classList.toggle("is-alert", (worst?.percent ?? 0) >= 85);
  drop.title = t("app.title");
}

function renderRow(report: ProviderReport, index: number): string {
  const isOpen = openRows.has(report.id);
  const meters = report.usage?.meters ?? [];
  const top = headline(meters);
  const crit = (top?.percent ?? 0) >= 85;
  const failed = !top;

  return `
    <section class="row ${isOpen ? "is-open" : ""} ${failed ? "is-failed" : ""} ${report.stale ? "is-stale" : ""}" style="--i:${index}">
      <button class="row-head" data-action="toggle" data-id="${escapeHtml(report.id)}" aria-expanded="${isOpen}">
        ${pixel(GLYPHS[report.kind], "glyph")}
        <span class="name">${escapeHtml(report.label)}</span>
        <span class="value">${crit ? '<i class="alert"></i>' : ""}${failed ? "··" : escapeHtml(valueText(top))}</span>
      </button>
      ${top?.percent != null ? cells(top.percent, CELLS) : ""}
      ${isOpen ? renderDetails(report, meters) : ""}
    </section>`;
}

function renderDetails(report: ProviderReport, meters: Meter[]): string {
  let error = "";
  if (report.error) {
    const setup = SETUP.includes(report.error.code);
    error = `
      <div class="note-line ${setup ? "is-action" : ""}" ${setup ? 'data-action="settings"' : ""} title="${escapeHtml(report.error.message)}">
        <span>${escapeHtml(report.stale ? t("status.stale") : t(`error.${report.error.code}` as MessageKey))}${setup ? " →" : ""}</span>
        ${report.stale ? "" : `<small>${t(`error.${report.error.code}.hint` as MessageKey)}</small>`}
      </div>`;
  }
  const meta = [report.usage?.plan, clockTime(report.fetchedAt * 1000)].filter(Boolean).map((s) => escapeHtml(String(s)));
  return `
    <div class="details">
      ${meters.map(renderMeter).join("")}${error}
      <div class="meta">${meta.join(" · ")}</div>
    </div>`;
}

function renderMeter(meter: Meter): string {
  let value = valueText(meter);
  if (meter.spentUsd != null && meter.budgetUsd != null) value += ` / ${usd(meter.budgetUsd)}`;
  const reset = meter.resetsAt != null ? duration(meter.resetsAt - Date.now() / 1000) : "";
  return `
    <div class="meter">
      <div class="meter-line">
        <span class="meter-label">${escapeHtml(meterLabel(meter))}</span>
        <span class="meter-value">${escapeHtml(value)}</span>
      </div>
      ${meter.percent != null ? cells(meter.percent, CELLS, "mini") : ""}
      ${reset ? `<div class="meter-reset">↺ ${escapeHtml(reset)}</div>` : ""}
    </div>`;
}

/**
 * Keys look like `weekly`, `window_2592000` (window length in seconds) or
 * `weekly:Fable` (a per-model limit). Unknown keys fall back to the backend's label.
 */
function meterLabel(meter: Meter): string {
  const [base, scope] = meter.key.split(":");
  let label = tMaybe(`meter.${base}`);
  const secs = Number(base.match(/^window_(\d+)$/)?.[1]);
  if (!label && secs > 0) {
    label = secs % 86_400 === 0 ? t("meter.windowDays", { n: secs / 86_400 }) : t("meter.windowHours", { n: Math.round(secs / 3600) });
  }
  if (!label) return meter.label;
  return scope ? `${label} · ${scope}` : label;
}

// ---------------------------------------------------------------------------
// Window size and placement

let lastSize = "";

/** The window has no chrome and isn't click-through, so it always wraps exactly what's visible. */
async function layout() {
  const panelH = Math.ceil(note.getBoundingClientRect().height);
  const [w, h] = placement === "desktop" || expanded ? [PANEL_W, panelH] : [DROP.w, DROP.h];
  const key = `${placement}:${w}x${h}`;
  if (h <= 0 || key === lastSize) return;
  lastSize = key;
  try {
    if (placement === "edge") await api.edgeLayout(w, h);
    else await win.setSize(new LogicalSize(w, h));
  } catch (e) {
    console.error("layout failed", e);
  }
}

async function openPanel() {
  window.clearTimeout(closeTimer);
  if (placement !== "edge") return;
  shell.classList.remove("closing");
  if (!expanded) {
    expanded = true;
    // Grow the (transparent) window first, then play the reveal inside it.
    await layout();
  }
  shell.classList.add("open", "opening");
  window.setTimeout(() => shell.classList.remove("opening"), 700);
}

function closeSoon(delay = LEAVE_CLOSE_MS) {
  window.clearTimeout(hoverTimer);
  if (placement !== "edge" || pinned || dragging) return;
  window.clearTimeout(closeTimer);
  closeTimer = window.setTimeout(closePanel, delay);
}

async function closePanel() {
  if (!expanded || pinned) return;
  shell.classList.remove("open");
  shell.classList.add("closing");
  await sleep(CLOSE_ANIM_MS);
  if (shell.classList.contains("open")) return; // reopened meanwhile
  shell.classList.remove("closing");
  expanded = false;
  await layout();
}

function setPinned(value: boolean) {
  pinned = value;
  render();
}

function applyPlacement(next: Placement) {
  placement = next;
  document.body.classList.toggle("mode-edge", next === "edge");
  document.body.classList.toggle("mode-desktop", next === "desktop");
  expanded = false;
  pinned = false;
  lastSize = "";
  shell.classList.remove("open", "closing");
  // Desktop mode: the OS moves the window by its top bar. Edge mode: our own sideways drag.
  for (const el of [top, document.getElementById("mark")!]) {
    if (next === "desktop") el.setAttribute("data-tauri-drag-region", "");
    else el.removeAttribute("data-tauri-drag-region");
  }
}

// ---------------------------------------------------------------------------
// Edge mode: hover to peek, click to keep open, drag the top bar sideways

drop.addEventListener("pointerenter", () => {
  window.clearTimeout(hoverTimer);
  hoverTimer = window.setTimeout(openPanel, HOVER_OPEN_MS);
});
drop.addEventListener("click", () => {
  setPinned(true);
  void openPanel();
});
document.documentElement.addEventListener("pointerenter", () => window.clearTimeout(closeTimer));
document.documentElement.addEventListener("pointerleave", () => closeSoon());

let dragging = false;
let dragStartX = 0;
let dragFrame = 0;

top.addEventListener("pointerdown", (event) => {
  if (placement !== "edge" || (event.target as HTMLElement).closest("button")) return;
  dragging = false;
  dragStartX = event.screenX;
  top.setPointerCapture(event.pointerId);
  void api.edgeDragStart();
});
top.addEventListener("pointermove", (event) => {
  if (placement !== "edge" || !top.hasPointerCapture(event.pointerId)) return;
  const dx = event.screenX - dragStartX;
  if (!dragging && Math.abs(dx) < 3) return;
  dragging = true;
  top.classList.add("dragging");
  cancelAnimationFrame(dragFrame);
  dragFrame = requestAnimationFrame(() => void api.edgeDrag(dx));
});
top.addEventListener("pointerup", (event) => {
  if (!top.hasPointerCapture(event.pointerId)) return;
  top.releasePointerCapture(event.pointerId);
  top.classList.remove("dragging");
  void api.edgeDragEnd();
  // Let the click that ends a drag pass without toggling anything.
  window.setTimeout(() => (dragging = false), 0);
});

// ---------------------------------------------------------------------------

document.addEventListener("click", (event) => {
  const el = (event.target as HTMLElement).closest<HTMLElement>("[data-action]");
  switch (el?.dataset.action) {
    case "refresh":
      return void refresh();
    case "settings":
      return void api.openSettings();
    case "hide":
      return void win.hide();
    case "pin":
      return setPinned(!pinned);
    case "toggle": {
      const id = el.dataset.id!;
      if (!openRows.delete(id)) openRows.add(id);
      writeOpenRows();
      return render();
    }
  }
});

function updateTrayLabels() {
  api
    .setTrayLabels({ toggle: t("tray.toggle"), refresh: t("tray.refresh"), settings: t("tray.settings"), quit: t("tray.quit") })
    .catch(console.error);
}

async function main() {
  document.getElementById("mark")!.innerHTML = pixel(ICONS.mark);
  refreshButton.innerHTML = pixel(ICONS.refresh);
  document.getElementById("settings")!.innerHTML = pixel(ICONS.settings);
  document.getElementById("hide")!.innerHTML = pixel(ICONS.hide);

  config = await api.getConfig();
  applyAppearance(config);
  applyPlacement(config.placement);
  updateTrayLabels();

  await listen<Config>(EVENTS.configChanged, ({ payload }) => {
    const moved = payload.placement !== config.placement;
    config = payload;
    applyAppearance(config);
    if (moved) applyPlacement(config.placement);
    render();
  });
  await listen<ProviderReport[]>(EVENTS.usageUpdated, ({ payload }) => {
    reports = payload;
    render();
  });

  reports = await api.getUsage();
  render();
  // Only the "resets in" countdowns change between fetches.
  window.setInterval(render, 60_000);
}

main().catch(console.error);
