<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { HistoryRow } from "$lib/types";
  import { fmtRelative, fmtVbFooterTime, decodeHtmlEntities } from "$lib/types";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let history = $state<HistoryRow[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let now = $state(new Date());

  async function load() {
    loading = true;
    try {
      history = await api.listThreadHistory(10);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function removeOne(row: HistoryRow) {
    const before = history;
    history = history.filter((h) => h.thread_id !== row.thread_id);
    try {
      await api.clearThreadHistoryEntry(row.thread_id);
    } catch (e) {
      history = before;
      error = String(e);
    }
  }

  async function clearAll() {
    if (history.length === 0) return;
    if (!confirm(`Clear all ${history.length} entries from history?`)) return;
    const before = history;
    history = [];
    try {
      await api.clearThreadHistoryAll();
    } catch (e) {
      history = before;
      error = String(e);
    }
  }

  onMount(() => {
    void load();
  });
</script>

<div class="vb-banner">
  <h1><a href="/" title="Back to home">History</a></h1>
  <div class="vb-actions">
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <a class="vb-btn" href="/">&larr; Home</a>
  </div>
</div>

<div class="vb-content">
  <div class="vb-toolbar">
    <span>
      Showing your last <b>{history.length}</b> viewed
      {history.length === 1 ? "thread" : "threads"}
    </span>
    <span class="vb-spacer"></span>
    {#if history.length > 0}
      <button class="vb-btn" onclick={clearAll}>Clear all</button>
    {/if}
  </div>

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Loading…</div></div>
  {:else if error}
    <div class="vb-group">
      <div class="vb-empty" style="color:var(--vb-pill-text)">{error}</div>
    </div>
  {:else if history.length === 0}
    <div class="vb-group">
      <div class="vb-empty">
        <em>No history yet.</em> Open a thread and it'll show up here.
      </div>
    </div>
  {:else}
    <div class="vb-group">
      <div class="vb-cat">
        <span class="caret">▼</span>
        <span>Recently viewed</span>
      </div>
      {#each history as h (h.thread_id)}
        <div class="hist-row">
          <div class="hist-main">
            <a class="hist-title" href={`/thread/${h.thread_id}`}>
              {decodeHtmlEntities(h.title)}
            </a>
            <div class="hist-meta">
              {#if h.forum_title}
                in
                {#if h.forum_id != null}
                  <a class="hist-forum" href={`/forum/${h.forum_id}`}>{h.forum_title}</a>
                {:else}
                  {h.forum_title}
                {/if}
                <span class="hist-sep">·</span>
              {/if}
              <span class="hist-when">viewed {fmtRelative(h.visited_at)}</span>
            </div>
          </div>
          <button
            class="hist-remove"
            title="Remove from history"
            aria-label="Remove from history"
            onclick={() => removeOne(h)}
          >×</button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>

<style>
  .hist-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--vb-border);
    background: var(--vb-content-bg);
  }
  .hist-row:last-child { border-bottom: none; }
  .hist-main { flex: 1; min-width: 0; }
  .hist-title {
    color: var(--vb-link);
    text-decoration: none;
    font-weight: bold;
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hist-title:hover { text-decoration: underline; }
  .hist-meta {
    margin-top: 2px;
    font-size: 11px;
    color: var(--vb-muted);
  }
  .hist-forum {
    color: var(--vb-link);
    text-decoration: none;
    font-weight: bold;
  }
  .hist-forum:hover { text-decoration: underline; }
  .hist-sep { color: var(--vb-very-muted); margin: 0 4px; }
  .hist-when { color: var(--vb-muted); }
  .hist-remove {
    width: 24px;
    height: 24px;
    border: 0;
    background: transparent;
    color: var(--vb-very-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    border-radius: 3px;
  }
  .hist-remove:hover {
    color: var(--vb-pill-text);
    background: color-mix(in srgb, var(--vb-pill-text) 12%, transparent);
  }
</style>
