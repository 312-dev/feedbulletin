<script lang="ts">
  import type { Forum } from "$lib/types";
  import { fmtVbAbsolute } from "$lib/types";
  import { api } from "$lib/api";

  type Props = {
    forum: Forum | null;
    onClose: () => void;
  };
  let { forum, onClose }: Props = $props();

  let refreshing = $state(false);
  let refreshError = $state<string | null>(null);

  async function retry() {
    if (!forum) return;
    refreshing = true;
    refreshError = null;
    try {
      await api.refreshForum(forum.id);
      onClose();
    } catch (e) {
      refreshError = String(e);
    } finally {
      refreshing = false;
    }
  }

  function keyHandler(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={keyHandler} />

{#if forum}
  <div
    class="fr-modal-backdrop"
    role="button"
    tabindex="-1"
    aria-label="Close dialog"
    onclick={onClose}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") onClose(); }}
  >
    <div
      class="fr-modal"
      role="dialog"
      aria-modal="true"
      aria-label="Feed fetch error"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
    >
      <div class="fr-modal-head">
        <h2>Feed Error</h2>
        <button class="fr-modal-close" onclick={onClose} aria-label="Close">×</button>
      </div>
      <div class="fr-modal-body">
        <div class="fr-modal-row"><b>Forum:</b> {forum.title}</div>
        <div class="fr-modal-row"><b>URL:</b> <code>{forum.source_url}</code></div>
        {#if forum.last_polled_at}
          <div class="fr-modal-row">
            <b>Last attempt:</b> {fmtVbAbsolute(forum.last_polled_at)}
          </div>
        {/if}
        {#if forum.last_visited_at}
          <div class="fr-modal-row">
            <b>Last visited:</b> {fmtVbAbsolute(forum.last_visited_at)}
          </div>
        {/if}
        <div class="fr-modal-row fr-modal-error-label"><b>Error:</b></div>
        <pre class="fr-modal-error">{forum.last_error}</pre>

        {#if refreshError}
          <div class="fr-modal-row fr-modal-retry-err">
            <b>Retry failed:</b> <code>{refreshError}</code>
          </div>
        {/if}
      </div>
      <div class="fr-modal-foot">
        <button class="vb-btn" onclick={onClose}>Close</button>
        <button class="vb-btn vb-btn-primary" onclick={retry} disabled={refreshing}>
          {refreshing ? "Retrying…" : "Retry now"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .fr-modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 5000;
    backdrop-filter: blur(2px);
  }
  .fr-modal {
    width: min(640px, 90vw);
    max-height: 80vh;
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    border: 1px solid var(--vb-border);
    border-radius: 6px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
  }
  .fr-modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: linear-gradient(to bottom, var(--vb-banner-grad-from), var(--vb-banner-grad-to));
    color: var(--vb-banner-text);
    border-bottom: 1px solid var(--vb-border);
    border-top-left-radius: 6px;
    border-top-right-radius: 6px;
  }
  .fr-modal-head h2 {
    margin: 0;
    font-size: 13px;
    font-weight: bold;
    letter-spacing: 0.3px;
  }
  .fr-modal-close {
    background: transparent;
    border: 0;
    color: var(--vb-banner-text);
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .fr-modal-close:hover {
    color: color-mix(in srgb, var(--vb-banner-text) 70%, transparent);
  }
  .fr-modal-body {
    flex: 1;
    overflow: auto;
    padding: 14px 16px;
  }
  .fr-modal-row {
    margin-bottom: 6px;
    word-break: break-all;
  }
  .fr-modal-row code {
    font-family: "SF Mono", Menlo, Consolas, monospace;
    font-size: 11px;
    padding: 1px 4px;
    background: var(--vb-row-bg-b);
    border-radius: 2px;
  }
  .fr-modal-error-label {
    margin-top: 12px;
  }
  .fr-modal-error {
    margin: 4px 0 0 0;
    padding: 10px 12px;
    background: var(--vb-row-bg-b);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    font-family: "SF Mono", Menlo, Consolas, monospace;
    font-size: 11px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 200px;
    overflow: auto;
    color: var(--vb-pill-text);
  }
  .fr-modal-retry-err {
    margin-top: 8px;
    color: var(--vb-pill-text);
  }
  .fr-modal-foot {
    padding: 10px 14px;
    border-top: 1px solid var(--vb-border);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .vb-btn-primary {
    background: var(--vb-link);
    color: white;
    border-color: var(--vb-link);
  }
</style>
