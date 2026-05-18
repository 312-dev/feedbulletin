/**
 * Registry of all available skins (palette families).
 *
 * Each skin owns the full set of `--vb-*` CSS custom properties — declared in
 * its own file under `src/lib/theme/skins/<slug>.css` — for both light and
 * dark modes. The shared structural rules live in `vb-classic.css` and consume
 * the variables only; they don't know which skin is active.
 *
 * Selection happens via two attributes on `<html>`:
 *   data-theme="<slug>"   — palette family (this list)
 *   data-mode="light|dark" — mode within that family
 *
 * Slugs are persisted to localStorage and to feeds.yaml; do not rename a slug
 * without considering existing user configs.
 */
export type SkinSlug =
  | "vb-classic"
  | "phpbb-prosilver"
  | "slashdot-classic"
  | "myspace-2006"
  | "livejournal-cozy"
  | "geocities-90s"
  | "bbs-amber"
  | "bbs-green"
  | "matrix-rain"
  | "letterpress-paper"
  | "zine-riot"
  | "brutalist-mono"
  | "vaporwave"
  | "earthtones"
  | "bubblegum"
  | "tactical-od"
  | "blueprint";

export type SkinMeta = {
  slug: SkinSlug;
  name: string;
  blurb: string;
  /** A 2-color swatch sampled from the light palette for dropdown previews. */
  swatch: [string, string];
};

export const SKINS: SkinMeta[] = [
  { slug: "vb-classic",        name: "vBulletin 4 (Classic)",      blurb: "Steely blue, Verdana, 2012 forum default.",                  swatch: ["#4F76A6", "#FFFFFF"] },
  { slug: "phpbb-prosilver",   name: "phpBB Prosilver",            blurb: "Quieter forum-default blues, neutral chrome.",               swatch: ["#69869C", "#ECEDEE"] },
  { slug: "slashdot-classic",  name: "Slashdot Classic",           blurb: "Geeky green on cream with monospace accents.",               swatch: ["#006600", "#F4F7E8"] },
  { slug: "myspace-2006",      name: "MySpace 2006",               blurb: "Hot purple + magenta, friendly bubble blocks.",              swatch: ["#5C3D8B", "#FFD0F0"] },
  { slug: "livejournal-cozy",  name: "LiveJournal Cozy",           blurb: "Sage and parchment, serif body, soft and warm.",             swatch: ["#7A8C5C", "#F3EFE3"] },
  { slug: "geocities-90s",     name: "GeoCities '99",              blurb: "Primary colors, tiled construction-zone energy.",            swatch: ["#FF1493", "#FFFF00"] },
  { slug: "bbs-amber",         name: "BBS Amber",                  blurb: "Vintage modem terminal — amber phosphor on near-black.",     swatch: ["#FFB000", "#1A1108"] },
  { slug: "bbs-green",         name: "BBS Green",                  blurb: "Green phosphor terminal, scanline-lite.",                    swatch: ["#33FF33", "#0A1408"] },
  { slug: "matrix-rain",       name: "Matrix Rain",                blurb: "Hacker — neon green and black, faint link glow.",            swatch: ["#00FF66", "#050905"] },
  { slug: "letterpress-paper", name: "Letterpress Paper",          blurb: "Cream stock, ink colors, serif — NYT/FT feel.",              swatch: ["#1A1A1A", "#F1ECDC"] },
  { slug: "zine-riot",         name: "Zine Riot",                  blurb: "Hot pink + yellow + cyan, sticker / zine energy.",           swatch: ["#FF2F92", "#FFF200"] },
  { slug: "brutalist-mono",    name: "Brutalist Mono",             blurb: "Stark black/white, mono, hard borders.",                     swatch: ["#000000", "#FFFFFF"] },
  { slug: "vaporwave",         name: "Vaporwave Sunset",           blurb: "Lavender, teal, peach pastels with sunset gradients.",       swatch: ["#FF77E9", "#7DF9FF"] },
  { slug: "earthtones",        name: "Earthtones",                 blurb: "Sand, olive, terracotta — understated and warm.",            swatch: ["#A0522D", "#E6D7B8"] },
  { slug: "bubblegum",         name: "Bubblegum",                  blurb: "Soft pink, lavender, mint — bright but pastel.",             swatch: ["#FF8EC7", "#E6F7FF"] },
  { slug: "tactical-od",       name: "Tactical OD",                blurb: "Olive drab and ranger orange, military / utility.",          swatch: ["#4B5320", "#FF7F00"] },
  { slug: "blueprint",         name: "Blueprint",                  blurb: "Drafting blueprint grid with white technical ink.",          swatch: ["#0E4D92", "#E8F0FA"] },
];

export const DEFAULT_SKIN: SkinSlug = "vb-classic";

const SLUG_SET = new Set<SkinSlug>(SKINS.map((s) => s.slug));

/** True if `s` is a recognized skin slug. Used by the YAML validator and the
 * theme store when sanitizing user input or persisted prefs. */
export function isSkinSlug(s: unknown): s is SkinSlug {
  return typeof s === "string" && SLUG_SET.has(s as SkinSlug);
}

export function getSkinMeta(slug: SkinSlug): SkinMeta {
  // SAFE: the type ensures slug is in SKINS; .find is for the data, not a
  // safety net.
  return SKINS.find((s) => s.slug === slug) ?? SKINS[0];
}
