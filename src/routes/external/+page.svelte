<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onDestroy, onMount } from "svelte";
  import { api } from "$lib/api";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let url = $state<string>("");
  let title = $state<string>("");
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load(u: string) {
    loading = true;
    error = null;
    try {
      await api.openThreadWebview(u);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function back() {
    void api.closeThreadWebview().finally(() => goto("/"));
  }

  async function openExternal() {
    if (!url) return;
    try {
      await api.openExternal(url);
    } catch {
      window.open(url, "_blank");
    }
  }

  function keyHandler(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      back();
    }
  }

  onMount(() => {
    window.addEventListener("keydown", keyHandler);
    return () => window.removeEventListener("keydown", keyHandler);
  });
  onDestroy(() => {
    void api.closeThreadWebview();
  });

  $effect(() => {
    const u = page.url.searchParams.get("url") ?? "";
    const t = page.url.searchParams.get("title") ?? "";
    url = u;
    title = t;
    if (u) void load(u);
  });

  function truncate(s: string, n: number): string {
    return s.length <= n ? s : s.slice(0, n - 1) + "…";
  }
</script>

<div class="fr-header" data-testid="external-header">
  <div class="fr-brand-row">
    <a href="/" class="fr-brand" onclick={(e) => { e.preventDefault(); back(); }}>feedBulletin</a>
    <span class="fr-spacer"></span>
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <ThemeToggle />
  </div>
  <div class="thread-toolbar" data-testid="thread-toolbar">
    <button class="tb-btn" onclick={back} title="Back (Esc)">&larr; Home</button>
    <div class="tb-crumb">
      {#if title}<span class="tb-crumb-title" title={title}>{truncate(title, 100)}</span>{/if}
    </div>
    <span class="tb-spacer"></span>
    <button class="tb-btn" onclick={openExternal} title="Open in browser">&uarr; Browser</button>
  </div>
</div>

{#if loading}
  <div class="thread-overlay-msg">Loading…</div>
{:else if error}
  <div class="thread-overlay-msg thread-overlay-error">{error}</div>
{/if}

<style>
  :global(html), :global(body), :global(.vb-page) {
    margin: 0;
    height: 100%;
    background: var(--vb-content-bg);
  }
  /* WHY: matches /thread/[id] two-tier header — 28px brand + 40px nav = 68px.
     Keep in sync with commands::TOOLBAR_HEIGHT in Rust. */
  .fr-header {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1000;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
  }
  .fr-brand-row {
    height: 28px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: linear-gradient(
      to bottom,
      color-mix(in srgb, var(--vb-banner-grad-to) 100%, transparent),
      color-mix(in srgb, var(--vb-banner-grad-to) 60%, black)
    );
    color: var(--vb-banner-text);
    border-bottom: 1px solid rgba(255,255,255,0.05);
    font-size: 11px;
  }
  .fr-brand {
    color: var(--vb-banner-text);
    font-weight: bold;
    font-size: 12px;
    text-decoration: none;
    letter-spacing: 0.3px;
  }
  .fr-brand:hover { color: color-mix(in srgb, var(--vb-banner-text) 80%, transparent); }
  .fr-spacer { flex: 1 1 auto; }
  .thread-toolbar {
    height: 40px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: linear-gradient(
      to bottom,
      var(--vb-banner-grad-from),
      var(--vb-banner-grad-to)
    );
    color: var(--vb-banner-text);
    border-bottom: 1px solid color-mix(in srgb, var(--vb-banner-grad-to) 70%, black);
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
  }
  .tb-btn {
    background: var(--vb-cat-grad-from);
    color: var(--vb-cat-text);
    border: 1px solid var(--vb-banner-grad-to);
    padding: 4px 10px;
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.2;
  }
  .tb-btn:hover { background: color-mix(in srgb, var(--vb-cat-grad-from) 80%, white); }
  .tb-crumb {
    margin-left: 12px;
    color: color-mix(in srgb, var(--vb-banner-text) 75%, transparent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 1;
  }
  .tb-crumb-title { color: var(--vb-banner-text); font-weight: bold; }
  .tb-spacer { flex: 1 1 auto; }

  .thread-overlay-msg {
    position: fixed;
    top: 84px;
    left: 50%;
    transform: translateX(-50%);
    padding: 10px 16px;
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    color: var(--vb-meta);
    font-family: Verdana, Tahoma, sans-serif;
    font-size: 11px;
    z-index: 999;
    box-shadow: 0 2px 6px var(--vb-content-shadow);
  }
  .thread-overlay-error { color: var(--vb-pill-text); }
</style>
