import type { Config } from "./types";

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
let theme: Config["theme"] = "auto";

function paint() {
  const resolved = theme === "auto" ? (darkQuery.matches ? "dark" : "light") : theme;
  document.documentElement.dataset.theme = resolved;
}

darkQuery.addEventListener("change", paint);

/** Applies theme and opacity from the config to the current window. */
export function applyAppearance(config: Config) {
  theme = config.theme;
  paint();
  document.documentElement.style.setProperty("--surface-alpha", String(config.opacity));
}
