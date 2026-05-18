<script lang="ts">
  /*
    Site-wide theme picker for the Settings page.

    Behavior the user asked for:
      - Hover an option        → preview that skin globally (transient).
      - Click an option        → mark it as the pending pick (sticky preview).
      - Click "Save"           → commit pending → site-wide localStorage.
      - Click "Revert" / leave → drop pending + preview; site-wide stays as-is.

    The dropdown rows render in their own theme, so the swatch IS the theme:
    each row is a `data-theme="<slug>"`-scoped element whose --vb-* vars
    resolve to that skin's palette. The current dark/light mode propagates
    via data-mode so users see the variant they're actually going to get.
  */
  import { onDestroy } from "svelte";
  import { SKINS, type SkinSlug } from "$lib/theme/skins";
  import {
    siteSkin,
    pendingSkin,
    previewSkin,
    setSiteSkin,
    mode,
  } from "$lib/theme/store";

  let { onDirtyChange = (_: boolean) => {} }: { onDirtyChange?: (dirty: boolean) => void } = $props();

  let open = $state(false);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLDivElement | null>(null);

  // Reflect store values as $state. Each store gets a one-way subscription;
  // commits go back through setSiteSkin / pendingSkin.set / previewSkin.set.
  let site: SkinSlug = $state("vb-classic");
  let pending: SkinSlug | null = $state(null);
  let currentMode: "light" | "dark" = $state("light");
  $effect(() => {
    const a = siteSkin.subscribe((s) => (site = s));
    const b = pendingSkin.subscribe((s) => (pending = s));
    const c = mode.subscribe((m) => (currentMode = m));
    return () => { a(); b(); c(); };
  });

  // Effective pick for highlight = pending if any, else site.
  let highlighted: SkinSlug = $derived((pending ?? site) as SkinSlug);

  // The page consumes this to enable the Save button and to know we need to
  // revert on unmount.
  let dirty = $derived(pending !== null && pending !== site);
  $effect(() => { onDirtyChange(dirty); });

  function toggle() {
    open = !open;
    if (open) {
      // Close on outside-click; install on next tick so this very click
      // doesn't immediately close.
      requestAnimationFrame(() => {
        window.addEventListener("mousedown", onWindowMouseDown);
        window.addEventListener("keydown", onKeydown);
      });
    } else {
      teardown();
    }
  }

  function teardown() {
    window.removeEventListener("mousedown", onWindowMouseDown);
    window.removeEventListener("keydown", onKeydown);
    previewSkin.set(null);
  }

  function onWindowMouseDown(e: MouseEvent) {
    const t = e.target as Node | null;
    if (!t) return;
    if (menu?.contains(t)) return;
    if (trigger?.contains(t)) return;
    open = false;
    teardown();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      open = false;
      teardown();
    }
  }

  function onRowEnter(slug: SkinSlug) {
    previewSkin.set(slug);
  }
  function onRowLeave() {
    previewSkin.set(null);
  }
  function onRowClick(slug: SkinSlug) {
    // Sticky preview = pending pick. Clicking the already-pending pick clears
    // the override (acts as "back to site default"). Clicking the same slug
    // as `site` also clears so we never carry a no-op dirty state.
    if (slug === site) {
      pendingSkin.set(null);
    } else {
      pendingSkin.set(slug);
    }
    open = false;
    teardown();
  }

  /** Called by the parent page when the user clicks Save. */
  export function commit() {
    if (pending && pending !== site) {
      setSiteSkin(pending);
    }
    pendingSkin.set(null);
    previewSkin.set(null);
  }
  /** Called by the parent page when the user clicks Revert. */
  export function revert() {
    pendingSkin.set(null);
    previewSkin.set(null);
  }

  // If the component unmounts (user navigates away from settings) with an
  // unsaved pending pick, drop it so the global skin returns to baseline.
  onDestroy(() => {
    pendingSkin.set(null);
    previewSkin.set(null);
    window.removeEventListener("mousedown", onWindowMouseDown);
    window.removeEventListener("keydown", onKeydown);
  });

  function metaFor(slug: SkinSlug) {
    return SKINS.find((s) => s.slug === slug) ?? SKINS[0];
  }
</script>

<div class="theme-picker">
  <button
    type="button"
    class="vb-btn theme-trigger"
    bind:this={trigger}
    onclick={toggle}
    aria-haspopup="listbox"
    aria-expanded={open}
  >
    <span
      class="theme-trigger-swatch"
      data-theme={highlighted}
      data-mode={currentMode}
      aria-hidden="true"
    ></span>
    <span class="theme-trigger-label">{metaFor(highlighted).name}</span>
    {#if pending && pending !== site}
      <span class="theme-trigger-pending" title="Unsaved change">●</span>
    {/if}
    <span class="theme-trigger-caret" aria-hidden="true">▾</span>
  </button>

  {#if open}
    <div
      class="theme-menu"
      bind:this={menu}
      role="listbox"
      tabindex="-1"
      onmouseleave={onRowLeave}
    >
      {#each SKINS as s (s.slug)}
        <button
          type="button"
          class="theme-row"
          class:selected={s.slug === highlighted}
          data-theme={s.slug}
          data-mode={currentMode}
          role="option"
          aria-selected={s.slug === highlighted}
          onmouseenter={() => onRowEnter(s.slug)}
          onfocus={() => onRowEnter(s.slug)}
          onclick={() => onRowClick(s.slug)}
        >
          <span class="theme-row-banner">{s.name}</span>
          <span class="theme-row-body">
            <span class="theme-row-link">a sample link</span>
            <span class="theme-row-meta">{s.blurb}</span>
          </span>
          {#if s.slug === site}
            <span class="theme-row-tag">site</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .theme-picker {
    position: relative;
    display: inline-block;
  }
  .theme-trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 160px;
  }
  .theme-trigger-swatch {
    width: 14px;
    height: 14px;
    border-radius: 2px;
    border: 1px solid var(--vb-border);
    background: linear-gradient(
      135deg,
      var(--vb-cat-grad-from) 0%,
      var(--vb-cat-grad-from) 49%,
      var(--vb-link) 50%,
      var(--vb-content-bg) 51%,
      var(--vb-content-bg) 100%
    );
    flex-shrink: 0;
  }
  .theme-trigger-label {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .theme-trigger-pending {
    color: var(--vb-pill-text);
    font-size: 10px;
  }
  .theme-trigger-caret {
    color: var(--vb-very-muted);
    font-size: 9px;
  }

  .theme-menu {
    /* Absolute-positioned floating menu under the trigger. The Settings
       toolbar uses a flex layout, so we anchor right-side to keep the menu
       within the content box. */
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 30;
    width: 360px;
    max-height: 70vh;
    overflow-y: auto;
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-border);
    box-shadow: 0 8px 24px var(--vb-content-shadow);
    border-radius: 3px;
  }

  /* Each row is its OWN data-theme scope. Inside it, --vb-* resolves to that
     skin's palette, so the row visually previews the theme. The row uses a
     mini-banner + body that mirrors the real app layout. */
  .theme-row {
    /* Reset button defaults */
    appearance: none;
    width: 100%;
    border: 0;
    padding: 0;
    margin: 0;
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    font-family: var(--vb-font-family);
    font-size: var(--vb-font-size-row);
    cursor: pointer;
    text-align: left;
    display: block;
    border-bottom: 1px solid var(--vb-border);
    position: relative;
  }
  .theme-row:last-child { border-bottom: 0; }
  .theme-row.selected {
    outline: 2px solid var(--vb-link);
    outline-offset: -2px;
  }
  .theme-row:hover {
    /* When the user mouses over a row, the global `previewSkin` flips and the
       whole app reflects this theme. The row itself just gets a subtle
       inset highlight using the row's own hover color. */
    background: var(--vb-row-hover);
  }

  .theme-row-banner {
    display: block;
    background: linear-gradient(
      to bottom,
      var(--vb-cat-grad-from) 0%,
      var(--vb-cat-grad-to) 100%
    );
    color: var(--vb-cat-text);
    padding: 5px 10px;
    font-size: 12px;
    font-weight: bold;
    text-shadow: 0 1px 0 rgba(0, 0, 0, 0.30);
    border-bottom: 1px solid var(--vb-border);
  }
  .theme-row-body {
    display: block;
    padding: 6px 10px 8px;
    background: var(--vb-content-bg);
  }
  .theme-row-link {
    display: block;
    color: var(--vb-link);
    font-size: var(--vb-font-size-forum);
    text-decoration: underline;
    margin-bottom: 2px;
  }
  .theme-row-meta {
    display: block;
    color: var(--vb-muted);
    font-size: 10px;
    font-style: italic;
  }
  .theme-row-tag {
    position: absolute;
    top: 6px;
    right: 8px;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    background: var(--vb-pill-ok-bg);
    color: var(--vb-pill-ok-text);
    border: 1px solid var(--vb-pill-ok-border);
    padding: 0 4px;
    border-radius: 2px;
    font-style: normal;
    font-weight: bold;
  }
</style>
