// UI strings. Kept in one place so they are easy to review and, later, to translate.

const en = {
  "app.title": "AI usage",
  "action.refresh": "Refresh",
  "action.settings": "Settings",
  "action.hide": "Hide",
  "action.pin": "Click to keep open",
  "action.unpin": "Click to let it close",

  "empty.cta": "Add a provider",
  "status.loading": "Loading…",
  "status.updated": "Updated {time}",
  "status.stale": "Showing the last known numbers",

  "meter.five_hour": "5-hour session",
  "meter.weekly": "Weekly",
  "meter.extra_usage": "Extra usage",
  "meter.month_cost": "This month",
  "meter.primary": "Primary limit",
  "meter.secondary": "Secondary limit",
  "meter.windowDays": "{n}-day window",
  "meter.windowHours": "{n}-hour window",
  "unit.d": "d",
  "unit.h": "h",
  "unit.m": "m",

  "error.not_configured": "Needs setup",
  "error.credentials_missing": "Not signed in",
  "error.token_expired": "Login expired",
  "error.unauthorized": "Access denied",
  "error.rate_limited": "Rate limited",
  "error.network": "Offline",
  "error.bad_response": "Unexpected response",
  "error.not_configured.hint": "Finish setting it up in Settings.",
  "error.credentials_missing.hint": "Sign in to the CLI, or pick the login file in Settings.",
  "error.token_expired.hint": "Open the CLI once and it refreshes.",
  "error.unauthorized.hint": "The login or key was rejected.",
  "error.rate_limited.hint": "Backing off; will try again later.",
  "error.network.hint": "Check your internet connection.",
  "error.bad_response.hint": "The service replied with something unexpected.",

  "tray.toggle": "Show / hide",
  "tray.refresh": "Refresh now",
  "tray.settings": "Settings…",
  "tray.quit": "Quit",

  "settings.title": "Settings",
  "settings.providers": "Providers",
  "settings.add": "Add",
  "settings.none": "No providers yet. Pick one below to get started.",
  "settings.name": "Name",
  "settings.enabled": "Show on widget",
  "settings.credentials": "Login file",
  "settings.credentials.hint":
    "Leave empty to use the default location. WSL users can point this at \\\\wsl.localhost\\<distro>\\home\\<user>\\…",
  "settings.apiKey": "Admin API key",
  "settings.apiKey.saved": "Saved. Leave blank to keep it",
  "settings.apiKey.remove": "Forget saved key",
  "settings.budget": "Monthly budget (USD)",
  "settings.budget.hint": "Optional. The bar fills towards this amount.",
  "settings.moveUp": "Move up",
  "settings.moveDown": "Move down",
  "settings.remove": "Remove",
  "settings.widget": "Widget",
  "settings.placement": "Placement",
  "settings.placement.edge": "Drop at the top edge",
  "settings.placement.edge.hint":
    "A small drop hangs from the top of the screen. Hover to peek, click to keep it open, drag it sideways to move it.",
  "settings.placement.desktop": "On the desktop",
  "settings.placement.desktop.hint":
    "A note anywhere on your screen. Drag it by its top bar.",
  "settings.above": "Show above other windows",
  "settings.above.hint":
    "Off: it stays on the desktop, behind your windows. On: always visible on top. Either way it never takes the keyboard from the app you're using.",
  "settings.refresh": "Refresh every",
  "settings.refresh.hint":
    "Updates on its own. API spend is checked at most every 30 min, and a rate-limited service is given more time.",
  "settings.minutes": "{n} min",
  "settings.autostart": "Launch when I sign in",
  "settings.theme": "Theme",
  "settings.theme.auto": "System",
  "settings.theme.dark": "Dark",
  "settings.theme.light": "Light",
  "settings.opacity": "Background opacity",
  "settings.privacy":
    "API keys are kept in your system's credential manager. CLI logins are read locally and only sent to the service that issued them. Nothing else leaves your computer.",
  "settings.save": "Save",
  "settings.saved": "Saved",
  "settings.close": "Close",
  "settings.error.budget": "Budget must be a positive number.",

  "kind.claude_subscription": "Claude plan",
  "kind.codex_subscription": "ChatGPT plan (Codex)",
  "kind.anthropic_api": "Anthropic API spend",
  "kind.openai_api": "OpenAI API spend",
  "kind.claude_subscription.hint": "Pro / Max limits, read from your Claude Code login.",
  "kind.codex_subscription.hint": "Plus / Pro / Team limits, read from your Codex CLI login.",
  "kind.anthropic_api.hint": "Month-to-date spend. Needs an Admin key from console.anthropic.com → Settings → Admin keys.",
  "kind.openai_api.hint": "Month-to-date spend. Needs an Admin key from platform.openai.com → Settings → Admin keys.",
};

export type MessageKey = keyof typeof en;

export function t(key: MessageKey, vars: Record<string, string | number> = {}): string {
  return en[key].replace(/\{(\w+)\}/g, (_, name) => String(vars[name] ?? `{${name}}`));
}

/** Translation for a key that may come from the backend, or `undefined` if unknown. */
export function tMaybe(key: string): string | undefined {
  return (en as Record<string, string>)[key];
}
