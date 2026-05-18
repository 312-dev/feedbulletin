<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { CategoryWithForums } from "$lib/types";
  import { fmtVbFooterTime } from "$lib/types";
  import CategoryHeader from "$lib/components/CategoryHeader.svelte";
  import ForumRow from "$lib/components/ForumRow.svelte";
  import FeedErrorDialog from "$lib/components/FeedErrorDialog.svelte";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";
  import type { Forum } from "$lib/types";

  let categories = $state<CategoryWithForums[]>([]);
  let open = $state<Record<number, boolean>>({});
  let loading = $state(true);
  let refreshing = $state(false);
  let marking = $state(false);
  let error = $state<string | null>(null);
  let now = $state(new Date());
  let errorDialogForum = $state<Forum | null>(null);

  async function load() {
    try {
      categories = await api.getCategories();
      for (const c of categories) {
        if (open[c.id] === undefined) open[c.id] = true;
      }
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function refreshAll() {
    refreshing = true;
    try {
      await api.refreshAll();
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      refreshing = false;
    }
  }

  async function markAllRead() {
    marking = true;
    // Optimistic: clear unread locally first.
    categories = categories.map((c) => ({
      ...c,
      forums: c.forums.map((f) => ({ ...f, unread: false })),
    }));
    try {
      await api.markAllForumsVisited();
    } catch (e) {
      error = String(e);
    } finally {
      marking = false;
    }
  }

  onMount(() => {
    load();
    const t = setInterval(() => {
      load();
      now = new Date();
    }, 5000);
    return () => clearInterval(t);
  });

  let totalForums = $derived(
    categories.reduce((acc, c) => acc + c.forums.length, 0)
  );
</script>

<div class="vb-banner" data-testid="banner">
  <h1><a href="/">feedBulletin</a></h1>
  <div class="vb-actions">
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <button class="vb-btn" onclick={markAllRead} disabled={marking || loading}>
      {marking ? "Marking…" : "Mark Forums Read"}
    </button>
    <button class="vb-btn" onclick={refreshAll} disabled={refreshing}>
      {refreshing ? "Refreshing…" : "Refresh"}
    </button>
    <a class="vb-btn" href="/settings" title="Edit feeds.yaml">Settings</a>
  </div>
</div>

<div class="vb-content">
  <div class="vb-toolbar">
    <span>{categories.length} {categories.length === 1 ? "category" : "categories"}</span>
    <span class="vb-toolbar-sep">·</span>
    <span>{totalForums} {totalForums === 1 ? "forum" : "forums"}</span>
    <span class="vb-spacer"></span>
    <a href="/">Home</a>
  </div>

  {#if loading}
    <div class="vb-group">
      <div class="vb-empty">Loading…</div>
    </div>
  {:else if error}
    <div class="vb-group">
      <div class="vb-empty">
        <div style="color:var(--vb-pill-text);font-weight:bold;">Could not load forums:</div>
        <div style="color:var(--vb-muted);margin-top:6px;">{error}</div>
      </div>
    </div>
  {:else if categories.length === 0}
    <div class="vb-group">
      <div class="vb-empty">
        <strong>No forums configured.</strong><br />
        Edit <code>feeds.yaml</code> in the project root and restart the app.
      </div>
    </div>
  {:else}
    {#each categories as category (category.id)}
      <div class="vb-group" data-testid="category" data-category-name={category.name}>
        <CategoryHeader
          id={category.id}
          name={category.name}
          forumCount={category.forums.length}
          open={open[category.id]}
          onToggle={() => (open[category.id] = !open[category.id])}
        />

        {#if open[category.id]}
          <div class="vb-subbar">
            <div></div>
            <div>Forum</div>
            <div style="text-align:right">Threads</div>
            <div style="text-align:right">Posts</div>
            <div>Last Post</div>
          </div>
          {#each category.forums as forum, idx (forum.id)}
            <ForumRow bind:forum={category.forums[idx]} onShowError={(f) => (errorDialogForum = f)} />
          {/each}
        {/if}
      </div>
    {/each}
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>

<FeedErrorDialog forum={errorDialogForum} onClose={() => (errorDialogForum = null)} />
