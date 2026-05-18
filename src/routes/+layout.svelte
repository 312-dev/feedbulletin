<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { api } from "$lib/api";
  import "../lib/theme/vb-classic.css";
  import "../lib/theme/skins.css";
  import "../lib/theme/responsive.css";
  import CategoryNavBar from "$lib/components/CategoryNavBar.svelte";
  import { watchSystemTheme, routeSkin } from "$lib/theme/store";
  import { isSkinSlug } from "$lib/theme/skins";
  import type { CategoryWithForums } from "$lib/types";

  let { children } = $props();

  // Categories drive the horizontal top-bar nav. Fetched once on layout mount
  // and refreshed on a slow interval so renames in feeds.yaml show up.
  let categories = $state<CategoryWithForums[]>([]);

  // Routes that have their own dedicated fixed two-tier header
  // (thread + external) hide the global nav bar to avoid stacking strips.
  let hideNavBar = $derived(
    page.route.id?.startsWith("/thread") || page.route.id?.startsWith("/external")
  );

  async function refreshCategories() {
    try { categories = await api.getCategories(); } catch {}
  }

  // ─── Per-forum theme override ────────────────────────────────────────────
  // When the user is on a /forum/[id] or /thread/[id] route AND that forum
  // declares `theme: <slug>` in feeds.yaml, apply that skin while on the
  // route. Resolution:
  //   /forum/[id]    → direct: id → categories[].forums[] match
  //   /thread/[id]   → fetch thread → thread.forum_id → categories[].forums[]
  // When no match or no override, clear routeSkin so the site-wide pick wins.
  function findForumById(id: number) {
    for (const c of categories) {
      for (const f of c.forums) if (f.id === id) return f;
    }
    return null;
  }
  function skinForForumId(id: number) {
    const f = findForumById(id);
    return f && isSkinSlug(f.theme) ? f.theme : null;
  }

  // Cache thread → forum_id lookups so navigating back into the same thread
  // doesn't re-fetch. Keyed by thread id; value is the forum id (or -1 if the
  // fetch failed, so we don't retry forever during one session).
  const threadForumCache = new Map<number, number>();

  $effect(() => {
    const route = page.route.id ?? "";
    if (route.startsWith("/forum/")) {
      const id = Number(page.params.id);
      if (!Number.isFinite(id) || categories.length === 0) {
        routeSkin.set(null);
        return;
      }
      routeSkin.set(skinForForumId(id));
      return;
    }
    if (route.startsWith("/thread/")) {
      const threadId = Number(page.params.id);
      if (!Number.isFinite(threadId) || categories.length === 0) {
        routeSkin.set(null);
        return;
      }
      // Async resolve; guard against the route changing again before the
      // fetch resolves by capturing the threadId we kicked off for and only
      // applying if it still matches the current route at resolution time.
      const cached = threadForumCache.get(threadId);
      if (cached !== undefined) {
        routeSkin.set(cached >= 0 ? skinForForumId(cached) : null);
        return;
      }
      // Optimistic: clear any prior override while we wait. Forum-skinned
      // threads will flip once the fetch lands; site-skinned threads stay put.
      routeSkin.set(null);
      void api.getThread(threadId).then((t) => {
        const stillOnThisRoute =
          (page.route.id ?? "").startsWith("/thread/") &&
          Number(page.params.id) === threadId;
        if (t && Number.isFinite(t.forum_id)) {
          threadForumCache.set(threadId, t.forum_id);
          if (stillOnThisRoute) routeSkin.set(skinForForumId(t.forum_id));
        } else {
          threadForumCache.set(threadId, -1);
        }
      }).catch(() => {
        threadForumCache.set(threadId, -1);
      });
      return;
    }
    routeSkin.set(null);
  });

  // ─── Browser-style zoom controls ────────────────────────────────────────
  // ⌘/Ctrl + `=`/`-`/`0` and trackpad pinch (ctrlKey wheel) drive the native
  // WKWebView zoom for both this UI and the child source-site webview, so
  // pinching while a thread is open zooms whichever surface is visible.
  const ZOOM_KEY = "fr_zoom_level";
  const ZOOM_MIN = 0.5;
  const ZOOM_MAX = 3.0;
  const ZOOM_STEP = 0.1;
  let zoomLevel = 1.0;
  let zoomApplyTimer: ReturnType<typeof setTimeout> | null = null;

  function loadZoom(): number {
    try {
      const raw = localStorage.getItem(ZOOM_KEY);
      const n = raw ? parseFloat(raw) : NaN;
      if (Number.isFinite(n) && n >= ZOOM_MIN && n <= ZOOM_MAX) return n;
    } catch {}
    return 1.0;
  }

  function persistZoom(z: number) {
    try { localStorage.setItem(ZOOM_KEY, String(z)); } catch {}
  }

  function applyZoom(next: number, immediate = false) {
    const clamped = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(next * 100) / 100));
    if (clamped === zoomLevel && !immediate) return;
    zoomLevel = clamped;
    persistZoom(clamped);
    // Debounce backend calls during a rapid pinch so we don't flood IPC.
    if (zoomApplyTimer) clearTimeout(zoomApplyTimer);
    const fire = () => { void api.setZoomLevel(clamped).catch(() => {}); };
    if (immediate) fire();
    else zoomApplyTimer = setTimeout(fire, 30);
  }

  function onKeydown(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey)) return;
    const tgt = e.target as HTMLElement | null;
    const tag = tgt?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tgt?.isContentEditable) return;

    // `=` is the unshifted key for `+` on US layouts; accept both.
    if (e.key === "=" || e.key === "+") {
      e.preventDefault();
      applyZoom(zoomLevel + ZOOM_STEP, true);
    } else if (e.key === "-" || e.key === "_") {
      e.preventDefault();
      applyZoom(zoomLevel - ZOOM_STEP, true);
    } else if (e.key === "0") {
      e.preventDefault();
      applyZoom(1.0, true);
    }
  }

  function onWheel(e: WheelEvent) {
    // macOS delivers trackpad pinch as wheel events with ctrlKey=true and a
    // proportional deltaY. Hold-Ctrl-and-scroll also lands here, which is
    // the conventional desktop-browser "zoom" gesture on every platform.
    if (!e.ctrlKey) return;
    e.preventDefault();
    // deltaY is large (~50+) on trackpad pinches and small (~1-4) on wheel
    // ticks; scale so a single pinch frame moves ~5%.
    const delta = -e.deltaY * 0.01;
    applyZoom(zoomLevel + delta);
  }

  onMount(() => {
    void refreshCategories();
    const t = setInterval(refreshCategories, 30_000);

    zoomLevel = loadZoom();
    if (zoomLevel !== 1.0) {
      void api.setZoomLevel(zoomLevel).catch(() => {});
    }
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("wheel", onWheel, { passive: false });

    // Theme: the store's module-level subscriber already reflected the
    // initial value onto <html data-theme> and pushed it to the backend.
    // Here we just install the OS-preference listener so the app continues
    // to follow system theme changes until the user makes an explicit pick.
    const stopSystemWatch = watchSystemTheme();

    return () => {
      clearInterval(t);
      window.removeEventListener("keydown", onKeydown);
      window.removeEventListener("wheel", onWheel);
      if (zoomApplyTimer) clearTimeout(zoomApplyTimer);
      stopSystemWatch();
    };
  });
</script>

{#if !hideNavBar}
  <CategoryNavBar {categories} />
{/if}
<div class="vb-page">
  {@render children?.()}
</div>
