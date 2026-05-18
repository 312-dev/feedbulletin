<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { FavoritePost } from "$lib/types";
  import { fmtRelative, fmtVbFooterTime, fmtVbPostTimestamp, decodeHtmlEntities } from "$lib/types";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let favorites = $state<FavoritePost[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let now = $state(new Date());
  let filter = $state("");

  let filteredFavorites = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return favorites;
    return favorites.filter((f) => {
      return (
        f.thread_title.toLowerCase().includes(q) ||
        (f.author ?? "").toLowerCase().includes(q) ||
        (f.body_excerpt ?? "").toLowerCase().includes(q)
      );
    });
  });

  type Group = {
    thread_id: number;
    thread_title: string;
    thread_source_url: string;
    most_recent_favorited_at: number;
    items: FavoritePost[];
  };

  let groups = $derived.by<Group[]>(() => {
    const byThread = new Map<number, Group>();
    for (const f of filteredFavorites) {
      const g = byThread.get(f.thread_id);
      if (g) {
        g.items.push(f);
        if (f.favorited_at > g.most_recent_favorited_at) {
          g.most_recent_favorited_at = f.favorited_at;
        }
      } else {
        byThread.set(f.thread_id, {
          thread_id: f.thread_id,
          thread_title: f.thread_title,
          thread_source_url: f.thread_source_url,
          most_recent_favorited_at: f.favorited_at,
          items: [f],
        });
      }
    }
    // Within each group: newest favorite first.
    for (const g of byThread.values()) {
      g.items.sort((a, b) => b.favorited_at - a.favorited_at);
    }
    // Across groups: newest group first.
    return Array.from(byThread.values()).sort(
      (a, b) => b.most_recent_favorited_at - a.most_recent_favorited_at
    );
  });

  async function load() {
    loading = true;
    try {
      favorites = await api.listFavoritePosts();
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function unfavorite(f: FavoritePost) {
    const before = favorites;
    favorites = favorites.filter(
      (x) => !(x.thread_id === f.thread_id && x.post_index === f.post_index)
    );
    try {
      await api.unfavoritePost(f.thread_id, f.post_index);
    } catch (e) {
      favorites = before;
      error = String(e);
    }
  }

  onMount(() => {
    void load();
  });
</script>

<div class="vb-banner">
  <h1><a href="/" title="Back to home">My Favorites</a></h1>
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
      {#if filter.trim()}
        <b>{filteredFavorites.length}</b> of <b>{favorites.length}</b>
        {favorites.length === 1 ? "favorite" : "favorites"} match
      {:else}
        <b>{favorites.length}</b>
        {favorites.length === 1 ? "favorite" : "favorites"}
        {#if groups.length > 0}
          <span class="vb-toolbar-sep">·</span>
          across <b>{groups.length}</b>
          {groups.length === 1 ? "thread" : "threads"}
        {/if}
      {/if}
    </span>
    <span class="vb-spacer"></span>
    {#if favorites.length > 0}
      <input
        class="fav-filter"
        type="search"
        placeholder="Filter favorites…"
        bind:value={filter}
        autocomplete="off"
      />
    {/if}
  </div>

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Loading…</div></div>
  {:else if error}
    <div class="vb-group"><div class="vb-empty" style="color:var(--vb-pill-text)">{error}</div></div>
  {:else if favorites.length > 0 && filteredFavorites.length === 0}
    <div class="vb-group">
      <div class="vb-empty">
        <em>No favorites match "{filter}".</em>
      </div>
    </div>
  {:else if favorites.length === 0}
    <div class="vb-group">
      <div class="vb-empty">
        <em>No favorites yet.</em>
        Star a post in any thread to save it here — the whole thread will stay cached.
      </div>
    </div>
  {:else}
    {#each groups as g (g.thread_id)}
      <div class="vb-group">
        <div class="vb-cat">
          <span class="caret">▼</span>
          <a class="fav-thread-link" href={`/thread/${g.thread_id}`}>
            {decodeHtmlEntities(g.thread_title)}
          </a>
          <span class="fav-thread-meta">{g.items.length} {g.items.length === 1 ? "post" : "posts"}</span>
        </div>

        {#each g.items as f (f.post_index)}
          <div class="fav-post-row">
            <div class="fav-post-head">
              <span class="fav-author">
                {#if f.author}<b>{f.author}</b>{:else}<i>—</i>{/if}
              </span>
              {#if f.timestamp_raw}
                <span class="fav-sep">·</span>
                <span class="fav-time">{fmtVbPostTimestamp(f.timestamp_raw)}</span>
              {/if}
              <span class="fav-sep">·</span>
              <span class="fav-when">favorited {fmtRelative(f.favorited_at)}</span>
              <span class="fav-actions">
                <a
                  class="fav-action"
                  href={`/thread/${f.thread_id}#post-${f.post_index}`}
                >View in thread →</a>
                <button
                  type="button"
                  class="fav-action fav-unfav"
                  onclick={() => unfavorite(f)}
                >Unfavorite</button>
              </span>
            </div>
            {#if f.body_excerpt}
              <div class="fav-excerpt">{f.body_excerpt}</div>
            {/if}
          </div>
        {/each}
      </div>
    {/each}
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>

<style>
  .fav-filter {
    width: 220px;
    padding: 3px 8px;
    font-family: inherit;
    font-size: 11px;
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
  }
  .fav-filter:focus {
    outline: none;
    border-color: var(--vb-link);
  }
  .fav-thread-link {
    color: var(--vb-cat-text);
    text-decoration: none;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fav-thread-link:hover { text-decoration: underline; }
  .fav-thread-meta {
    color: var(--vb-cat-text);
    opacity: 0.75;
    font-size: 11px;
    margin-left: 8px;
  }
  .fav-post-row {
    border-bottom: 1px solid var(--vb-border);
    padding: 8px 12px;
    background: var(--vb-content-bg);
  }
  .fav-post-row:last-child { border-bottom: none; }
  .fav-post-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px;
    font-size: 11px;
    color: var(--vb-meta);
  }
  .fav-author b { color: var(--vb-page-text); }
  .fav-sep { color: var(--vb-very-muted); }
  .fav-time, .fav-when { color: var(--vb-muted); }
  .fav-actions {
    margin-left: auto;
    display: inline-flex;
    align-items: baseline;
    gap: 12px;
  }
  .fav-action {
    color: var(--vb-link);
    text-decoration: none;
    background: transparent;
    border: 0;
    padding: 0;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .fav-action:hover { text-decoration: underline; color: var(--vb-link-hover); }
  .fav-unfav { color: var(--vb-pill-text); }
  .fav-excerpt {
    margin-top: 4px;
    font-size: 12px;
    color: var(--vb-page-text);
    line-height: 1.45;
  }
</style>
