<script lang="ts">
  import type { DebugField } from "$lib/types";

  type Props = {
    field: DebugField;
    elementHtml: string;
    submitting: boolean;
    error?: string | null;
    onSubmit: (field: DebugField, complaint: string) => void;
    onClose: () => void;
  };

  let {
    field,
    elementHtml,
    submitting,
    error = null,
    onSubmit,
    onClose,
  }: Props = $props();

  // eslint-disable-next-line svelte/no-reactive-reassign
  let editedField = $state<DebugField>(field);
  let complaint = $state("");

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  const allFields: DebugField[] = [
    "author",
    "author_url",
    "avatar",
    "author_rank",
    "author_post_count",
    "author_join_date",
    "author_location",
    "timestamp",
    "body",
    "post_number",
    "post",
  ];

  function previewHtml(html: string): string {
    if (html.length <= 400) return html;
    return html.slice(0, 400) + "…";
  }

  function submit() {
    if (!complaint.trim()) return;
    onSubmit(editedField, complaint.trim());
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="dcd-backdrop"
  onclick={onClose}
  onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") onClose(); }}
  role="button"
  tabindex="0"
  aria-label="Close dialog"
>
  <div
    class="dcd-modal"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
  >
    <header class="dcd-head">
      <span class="dcd-title">Refine this site's layout</span>
      <button class="dcd-x" onclick={onClose} aria-label="Close">×</button>
    </header>

    <div class="dcd-row">
      <label for="dcd-field">Field you clicked:</label>
      <select id="dcd-field" bind:value={editedField}>
        {#each allFields as f}
          <option value={f}>{f}</option>
        {/each}
      </select>
    </div>

    <div class="dcd-row">
      <span class="dcd-label">Captured element:</span>
      <pre class="dcd-html-preview">{previewHtml(elementHtml)}</pre>
    </div>

    <div class="dcd-row">
      <label for="dcd-complaint">What's wrong with how this renders?</label>
      <textarea
        id="dcd-complaint"
        bind:value={complaint}
        rows="4"
        placeholder="e.g. the username here is wrong — the real author is the bold text inside .username, not this badge"
        disabled={submitting}
      ></textarea>
    </div>

    {#if error}
      <div class="dcd-err">{error}</div>
    {/if}

    <footer class="dcd-foot">
      <button class="dcd-btn" onclick={onClose} disabled={submitting}>Cancel</button>
      <button
        class="dcd-btn dcd-btn-primary"
        onclick={submit}
        disabled={submitting || !complaint.trim()}
      >
        {submitting ? "Refining…" : "Send to agent"}
      </button>
    </footer>
  </div>
</div>

<style>
  .dcd-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 1200;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: Verdana, Tahoma, sans-serif;
    font-size: 12px;
  }
  .dcd-modal {
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-cat-grad-from);
    width: min(560px, 92vw);
    max-height: 86vh;
    overflow: auto;
    box-shadow: 0 10px 30px rgba(0,0,0,0.35);
  }
  .dcd-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: linear-gradient(to bottom, var(--vb-cat-grad-from), var(--vb-cat-grad-to));
    color: var(--vb-cat-text);
    font-weight: bold;
  }
  .dcd-x {
    background: transparent;
    color: var(--vb-cat-text);
    border: none;
    font-size: 18px;
    cursor: pointer;
    line-height: 1;
  }
  .dcd-row {
    padding: 10px 14px;
    border-bottom: 1px solid var(--vb-border);
  }
  .dcd-row label, .dcd-label {
    display: block;
    font-weight: bold;
    margin-bottom: 4px;
    color: var(--vb-meta);
  }
  .dcd-row select,
  .dcd-row textarea {
    width: 100%;
    box-sizing: border-box;
    font-family: inherit;
    font-size: 12px;
    border: 1px solid var(--vb-border);
    padding: 6px 8px;
    background: var(--vb-row-bg-b);
    color: var(--vb-page-text);
  }
  .dcd-html-preview {
    background: var(--vb-blockquote-bg);
    border: 1px solid var(--vb-border);
    margin: 0;
    padding: 8px;
    overflow-x: auto;
    font-family: Consolas, Menlo, monospace;
    font-size: 11px;
    color: var(--vb-meta);
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 160px;
  }
  .dcd-foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 14px;
  }
  .dcd-btn {
    background: var(--vb-row-hover);
    border: 1px solid var(--vb-border);
    color: var(--vb-page-text);
    padding: 5px 12px;
    cursor: pointer;
    font-family: inherit;
    font-size: 12px;
  }
  .dcd-btn:disabled { opacity: 0.5; cursor: default; }
  .dcd-btn-primary {
    background: var(--vb-cat-grad-from);
    color: var(--vb-cat-text);
    border-color: var(--vb-cat-grad-to);
    font-weight: bold;
  }
  .dcd-err {
    padding: 8px 14px;
    background: var(--vb-pill-bg);
    color: var(--vb-pill-text);
    font-size: 11px;
    border-top: 1px solid var(--vb-pill-border);
  }
</style>
