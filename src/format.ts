import { t } from "./i18n";

const LOCALE = "en-US";

/** "2h 14m", "3d 4h", "12m". */
export function duration(seconds: number): string {
  const mins = Math.max(0, Math.round(seconds / 60));
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;
  const [ud, uh, um] = [t("unit.d"), t("unit.h"), t("unit.m")];
  if (d > 0) return h > 0 ? `${d}${ud} ${h}${uh}` : `${d}${ud}`;
  if (h > 0) return m > 0 ? `${h}${uh} ${m}${um}` : `${h}${uh}`;
  return `${m}${um}`;
}

export function usd(value: number): string {
  return new Intl.NumberFormat(LOCALE, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: value >= 1000 ? 0 : 2,
  }).format(value);
}

export function clockTime(ms: number): string {
  return new Intl.DateTimeFormat(LOCALE, { hour: "2-digit", minute: "2-digit" }).format(ms);
}

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}
