import { writable, get, derived, type Readable } from "svelte/store";
import { api } from "$lib/api";
import { DEFAULT_SKIN, isSkinSlug, type SkinSlug } from "./skins";

/** Light/dark mode within a skin. */
export type Mode = "light" | "dark";

const MODE_KEY = "fr_theme";  // historical key — kept so existing users don't lose their mode pick
const SKIN_KEY = "fr_skin";

// ─── mode (light/dark) ───────────────────────────────────────────────────────

function initialMode(): Mode {
  if (typeof window === "undefined") return "light";
  try {
    const saved = localStorage.getItem(MODE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {}
  try {
    if (window.matchMedia("(prefers-color-scheme: dark)").matches) return "dark";
  } catch {}
  return "light";
}

/** Has the user explicitly picked a light/dark mode (vs. inheriting from OS)? */
export function hasExplicitModeChoice(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return localStorage.getItem(MODE_KEY) !== null;
  } catch {
    return false;
  }
}

// ─── skin (palette family) ───────────────────────────────────────────────────

function initialSkin(): SkinSlug {
  if (typeof window === "undefined") return DEFAULT_SKIN;
  try {
    const saved = localStorage.getItem(SKIN_KEY);
    if (isSkinSlug(saved)) return saved;
  } catch {}
  return DEFAULT_SKIN;
}

/** The skin the user picked site-wide. Per-forum YAML overrides take precedence
 * while on that forum's routes; this is the fallback when no override applies. */
export const siteSkin = writable<SkinSlug>(initialSkin());

/** Route-scoped override: a forum's YAML `theme:` field, or null. The layout
 * sets this when a route loads a forum that has its own skin, and clears it
 * on navigation away. */
export const routeSkin = writable<SkinSlug | null>(null);

/** Settings-page transient overrides — a "hover preview" while pointing at a
 * dropdown option, and a "pending pick" the user clicked but hasn't saved.
 * Both clear when the settings page unmounts without saving. */
export const previewSkin = writable<SkinSlug | null>(null);
export const pendingSkin = writable<SkinSlug | null>(null);

/** Final applied skin — what actually gets written to `data-theme`.
 * Precedence: hover > pending > route > site. */
export const activeSkin: Readable<SkinSlug> = derived(
  [previewSkin, pendingSkin, routeSkin, siteSkin],
  ([$preview, $pending, $route, $site]) => $preview ?? $pending ?? $route ?? $site,
);

// ─── reflection onto <html> ──────────────────────────────────────────────────

export const mode = writable<Mode>(initialMode());

function reflect(skin: SkinSlug, m: Mode) {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", skin);
    document.documentElement.setAttribute("data-mode", m);
  }
  // Tell the backend so child WKWebviews (thread view) flip color-scheme too.
  void api.setWebviewColorScheme(m === "dark").catch(() => {});
}

// Re-reflect whenever skin or mode changes. The store-level subscribers run on
// import (module top level), so the initial value lands before the first paint.
let lastSkin: SkinSlug = initialSkin();
let lastMode: Mode = initialMode();
activeSkin.subscribe((s) => {
  lastSkin = s;
  reflect(s, lastMode);
});
mode.subscribe((m) => {
  lastMode = m;
  reflect(lastSkin, m);
});

// ─── public API ──────────────────────────────────────────────────────────────

export function setMode(next: Mode) {
  try { localStorage.setItem(MODE_KEY, next); } catch {}
  mode.set(next);
}

export function toggleMode() {
  setMode(get(mode) === "dark" ? "light" : "dark");
}

/** Commit a skin pick site-wide. Persists to localStorage. */
export function setSiteSkin(next: SkinSlug) {
  try { localStorage.setItem(SKIN_KEY, next); } catch {}
  siteSkin.set(next);
}

/** OS-level prefers-color-scheme follower. Only acts while the user hasn't
 * made an explicit mode pick. */
export function watchSystemMode(): () => void {
  if (typeof window === "undefined") return () => {};
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const onChange = (e: MediaQueryListEvent) => {
    if (hasExplicitModeChoice()) return;
    mode.set(e.matches ? "dark" : "light");
  };
  mq.addEventListener?.("change", onChange);
  return () => mq.removeEventListener?.("change", onChange);
}

// ─── backwards-compat aliases ────────────────────────────────────────────────
// The pre-skin theme system exported `theme` (the mode store) and
// `toggleTheme`. Keep them so ThemeToggle.svelte and the rest of the codebase
// don't have to change in one go.
export const theme = mode;
export const toggleTheme = toggleMode;
export const hasExplicitChoice = hasExplicitModeChoice;
export const watchSystemTheme = watchSystemMode;
export type Theme = Mode;
