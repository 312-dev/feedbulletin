<script lang="ts">
  import { theme, toggleTheme } from "$lib/theme/store";

  let { compact = false }: { compact?: boolean } = $props();

  let current = $state<"light" | "dark">("light");
  // Keep a plain $state mirror of the store value so reactivity works
  // cleanly inside the template; subscribe in $effect and unsubscribe on
  // cleanup.
  $effect(() => {
    const unsub = theme.subscribe((t) => (current = t));
    return unsub;
  });
</script>

<button
  type="button"
  class="vb-theme-toggle"
  class:compact
  onclick={toggleTheme}
  title={current === "dark" ? "Switch to light mode" : "Switch to dark mode"}
  aria-label={current === "dark" ? "Switch to light mode" : "Switch to dark mode"}
>
  {#if current === "dark"}
    <!-- Sun icon — clicking goes back to light. -->
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="4" fill="currentColor" />
      <g stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <line x1="12" y1="2"  x2="12" y2="5" />
        <line x1="12" y1="19" x2="12" y2="22" />
        <line x1="2"  y1="12" x2="5"  y2="12" />
        <line x1="19" y1="12" x2="22" y2="12" />
        <line x1="4.5"  y1="4.5"  x2="6.5"  y2="6.5" />
        <line x1="17.5" y1="17.5" x2="19.5" y2="19.5" />
        <line x1="4.5"  y1="19.5" x2="6.5"  y2="17.5" />
        <line x1="17.5" y1="6.5"  x2="19.5" y2="4.5" />
      </g>
    </svg>
  {:else}
    <!-- Moon icon — clicking goes to dark. -->
    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M21 12.8A8.5 8.5 0 0 1 11.2 3a8.5 8.5 0 1 0 9.8 9.8z"
        fill="currentColor"
      />
    </svg>
  {/if}
</button>

<style>
  .vb-theme-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    color: inherit;
    border: 0;
    padding: 0 8px;
    height: 100%;
    cursor: pointer;
    line-height: 1;
    font: inherit;
    transition: background-color 0.1s ease;
  }
  .vb-theme-toggle.compact {
    padding: 0 6px;
    height: 22px;
    border-radius: 3px;
  }
  .vb-theme-toggle:hover {
    background: rgba(255, 255, 255, 0.10);
  }
  .vb-theme-toggle:focus-visible {
    outline: 1px dotted rgba(255, 255, 255, 0.6);
    outline-offset: -2px;
  }
  .vb-theme-toggle svg {
    display: block;
  }
</style>
