import type { ProviderKind } from "./types";

// Two-tone pixel glyphs: `W` = foreground, `G` = mid grey, `.` = empty.
type Glyph = string[];

const SPARK: Glyph = [
  "G..W..G",
  ".G.W.G.",
  "..WWW..",
  "WWW.WWW",
  "..WWW..",
  ".G.W.G.",
  "G..W..G",
];

const RING: Glyph = [
  "..WWW..",
  ".W...W.",
  "W..G..W",
  "W.GGG.W",
  "W..G..W",
  ".W...W.",
  "..WWW..",
];

/** API spend variants: same shapes, framed, so plan and API cards are told apart at a glance. */
const SPARK_API: Glyph = [
  "GGGGGGG",
  "G..W..G",
  "G.WWW.G",
  "GWW.WWG",
  "G.WWW.G",
  "G..W..G",
  "GGGGGGG",
];

const RING_API: Glyph = [
  "GGGGGGG",
  "G.WWW.G",
  "GW...WG",
  "GW.W.WG",
  "GW...WG",
  "G.WWW.G",
  "GGGGGGG",
];

export const GLYPHS: Record<ProviderKind, Glyph> = {
  claude_subscription: SPARK,
  anthropic_api: SPARK_API,
  codex_subscription: RING,
  openai_api: RING_API,
};

export const ICONS = {
  mark: ["W.W.W", ".....", "W.W.G", ".....", "W.G.G"],
  refresh: [".WWWW", "W..WW", "W....", "W...W", ".WWW."],
  settings: ["W.W.W", ".WWW.", "WW.WW", ".WWW.", "W.W.W"],
  hide: [".....", ".....", "WWWWW", ".....", "....."],
  up: [".....", "..W..", ".WWW.", "WW.WW", "....."],
  down: [".....", "WW.WW", ".WWW.", "..W..", "....."],
  remove: ["W...W", ".W.W.", "..W..", ".W.W.", "W...W"],
  pin: ["WWWWW", ".WWW.", ".WWW.", "WWWWW", "..W.."],
  pinned: ["WWWWW", "WWWWW", "WWWWW", "WWWWW", "..W.."],
  chevron: [".....", "W...W", ".W.W.", "..W..", "....."],
} satisfies Record<string, Glyph>;

export function pixel(glyph: Glyph, className = ""): string {
  let rects = "";
  glyph.forEach((row, y) =>
    [...row].forEach((c, x) => {
      if (c !== ".") rects += `<rect x="${x}" y="${y}" width="1" height="1" class="${c === "G" ? "px-g" : "px-w"}"/>`;
    }),
  );
  return `<svg class="px ${className}" viewBox="0 0 ${glyph[0].length} ${glyph.length}" shape-rendering="crispEdges" aria-hidden="true">${rects}</svg>`;
}

/** A row of square cells, `filled` of which are lit. */
export function cells(percent: number | null, count: number, className = ""): string {
  const lit = percent == null ? 0 : Math.min(count, Math.round((percent / 100) * count));
  // Anything above zero gets at least one lit cell, so "in use" never reads as "empty".
  const shown = percent != null && percent > 0 ? Math.max(1, lit) : lit;
  let html = "";
  for (let i = 0; i < count; i++) html += `<i class="${i < shown ? "on" : ""}"></i>`;
  return `<div class="cells ${className}" aria-hidden="true">${html}</div>`;
}
