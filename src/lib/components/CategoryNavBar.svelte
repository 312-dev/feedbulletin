<script lang="ts">
  import { page } from "$app/state";
  import { onMount, tick } from "svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import type { Category, CategoryWithForums } from "$lib/types";

  type AnyCat = Category | CategoryWithForums;
  let { categories }: { categories: AnyCat[] } = $props();

  let bar = $state<HTMLElement | null>(null);
  let measureRow = $state<HTMLElement | null>(null);
  let visibleCount = $state<number>(0);
  let moreOpen = $state(false);

  let activeId = $derived.by(() => {
    const m = page.url.pathname.match(/^\/category\/(\d+)/);
    return m ? Number(m[1]) : null;
  });

  // WHY: render every category off-screen first to measure its true width, then
  // walk the list left-to-right adding tabs to the visible row until we'd
  // overflow the container minus the right-side action area (More + theme
  // toggle).
  const ACTIONS_RESERVE_PX = 110;
  const TAB_GAP_PX = 2;
  const HOME_BTN_PX = 70;

  function recompute() {
    if (!bar || !measureRow) return;
    const total = bar.clientWidth - HOME_BTN_PX;
    const tabs = Array.from(measureRow.children) as HTMLElement[];
    if (tabs.length === 0) {
      visibleCount = 0;
      return;
    }
    let n = 0;
    let used = 0;
    for (let i = 0; i < tabs.length; i++) {
      const w = tabs[i].offsetWidth + TAB_GAP_PX;
      const cap = total - ACTIONS_RESERVE_PX;
      if (used + w > cap) break;
      used += w;
      n++;
    }
    visibleCount = Math.max(0, n);
  }

  onMount(() => {
    void tick().then(recompute);
    const ro = new ResizeObserver(recompute);
    if (bar) ro.observe(bar);
    window.addEventListener("resize", recompute);
    function onDocClick(e: MouseEvent) {
      if (!bar) return;
      if (!bar.contains(e.target as Node)) moreOpen = false;
    }
    document.addEventListener("click", onDocClick);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", recompute);
      document.removeEventListener("click", onDocClick);
    };
  });

  $effect(() => {
    if (categories) void tick().then(recompute);
  });

  let visible = $derived(categories.slice(0, visibleCount));
  let overflow = $derived(categories.slice(visibleCount));
</script>

<nav class="vb-navbar" bind:this={bar} aria-label="Forum categories">
  <a class="vb-nav-item vb-nav-home" href="/" class:active={page.url.pathname === "/"}>Home</a>

  {#each visible as cat (cat.id)}
    <a
      class="vb-nav-item"
      class:active={activeId === cat.id}
      href={`/category/${cat.id}`}
    >{cat.name}</a>
  {/each}

  <div class="vb-nav-actions">
    {#if overflow.length > 0}
      <div class="vb-nav-more-wrap">
        <button
          type="button"
          class="vb-nav-item vb-nav-more"
          class:active={moreOpen}
          onclick={(e) => { e.stopPropagation(); moreOpen = !moreOpen; }}
          aria-haspopup="menu"
          aria-expanded={moreOpen}
        >More <span class="vb-nav-caret">▾</span></button>
        {#if moreOpen}
          <div class="vb-nav-dropdown" role="menu">
            {#each overflow as cat (cat.id)}
              <a
                role="menuitem"
                class="vb-nav-dd-item"
                class:active={activeId === cat.id}
                href={`/category/${cat.id}`}
                onclick={() => (moreOpen = false)}
              >{cat.name}</a>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
    <ThemeToggle />
  </div>

  <!-- Hidden measuring row — same tab styling so offsetWidth lines up. -->
  <div class="vb-nav-measure" aria-hidden="true" bind:this={measureRow}>
    {#each categories as cat (cat.id)}
      <span class="vb-nav-item">{cat.name}</span>
    {/each}
  </div>
</nav>

<style>
  .vb-navbar {
    position: relative;
    display: flex;
    align-items: stretch;
    height: 24px;
    padding: 0 4px;
    background: linear-gradient(
      to bottom,
      var(--vb-cat-grad-from) 0%,
      var(--vb-cat-grad-to) 100%
    );
    border-top: 1px solid color-mix(in srgb, var(--vb-cat-grad-from) 70%, white);
    border-bottom: 1px solid color-mix(in srgb, var(--vb-cat-grad-to) 60%, black);
    font: 11px Verdana, Tahoma, "Lucida Sans", sans-serif;
    color: var(--vb-cat-text);
    /* IMPORTANT: must not clip — the More dropdown overflows below. */
    overflow: visible;
  }
  .vb-nav-item {
    display: inline-flex;
    align-items: center;
    height: 100%;
    padding: 0 10px;
    margin-right: 2px;
    color: var(--vb-cat-text);
    text-decoration: none;
    white-space: nowrap;
    line-height: 1;
    border: 0;
    background: transparent;
    font: inherit;
    cursor: pointer;
    box-sizing: border-box;
  }
  .vb-nav-item:hover {
    background: rgba(255,255,255,0.10);
  }
  .vb-nav-item.active {
    background: linear-gradient(
      to bottom,
      color-mix(in srgb, var(--vb-content-bg) 85%, var(--vb-cat-grad-from)) 0%,
      color-mix(in srgb, var(--vb-content-bg) 65%, var(--vb-cat-grad-from)) 100%
    );
    color: var(--vb-link);
    font-weight: bold;
  }
  .vb-nav-home {
    font-weight: bold;
    border-right: 1px solid rgba(255,255,255,0.15);
    margin-right: 6px;
  }
  .vb-nav-actions {
    margin-left: auto;
    display: flex;
    align-items: stretch;
    height: 100%;
  }
  .vb-nav-more-wrap {
    position: relative;
    display: flex;
    align-items: stretch;
  }
  .vb-nav-more {
    background: linear-gradient(
      to bottom,
      color-mix(in srgb, var(--vb-cat-grad-from) 80%, white) 0%,
      color-mix(in srgb, var(--vb-cat-grad-to) 80%, white) 100%
    );
    border-left: 1px solid color-mix(in srgb, var(--vb-cat-grad-to) 60%, black);
    border-right: 1px solid color-mix(in srgb, var(--vb-cat-grad-to) 60%, black);
    font-weight: bold;
  }
  .vb-nav-more:hover,
  .vb-nav-more.active {
    background: color-mix(in srgb, var(--vb-cat-grad-from) 70%, white);
  }
  .vb-nav-caret { margin-left: 4px; font-size: 9px; }

  .vb-nav-dropdown {
    position: absolute;
    right: 0;
    top: 100%;
    margin: 0;
    padding: 3px 0;
    background: var(--vb-content-bg);
    border: 1px solid color-mix(in srgb, var(--vb-cat-grad-to) 60%, black);
    border-top: 0;
    min-width: 200px;
    box-shadow: 2px 4px 8px rgba(0,0,0,0.30);
    z-index: 2000;
  }
  .vb-nav-dd-item {
    display: block;
    padding: 5px 14px;
    color: var(--vb-link);
    text-decoration: none;
    font-size: 11px;
    border-bottom: 1px solid var(--vb-border);
    white-space: nowrap;
  }
  .vb-nav-dd-item:last-child { border-bottom: 0; }
  .vb-nav-dd-item:hover {
    background: var(--vb-row-hover);
    color: var(--vb-link-hover);
  }
  .vb-nav-dd-item.active {
    background: var(--vb-cat-grad-from);
    color: var(--vb-cat-text);
    font-weight: bold;
  }

  /* Off-screen measuring row — items here have the same font/padding so their
     offsetWidth matches the visible tabs' actual widths. */
  .vb-nav-measure {
    position: absolute;
    left: -10000px;
    top: 0;
    visibility: hidden;
    pointer-events: none;
    display: flex;
    align-items: stretch;
    height: 24px;
    white-space: nowrap;
  }
</style>
