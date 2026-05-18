<script lang="ts" module>
  import type { Snapshot } from "./$types";

  // Remember the user's scroll position on this forum-thread list so that
  // when they navigate into a thread and then back, they land where they
  // left off. SvelteKit stashes this in sessionStorage keyed to the history
  // entry — restoration runs on history.back() / forward.
  export const snapshot: Snapshot<number> = {
    capture: () => (typeof window !== "undefined" ? window.scrollY : 0),
    restore: (y) => {
      if (typeof window === "undefined") return;
      // Wait for the threads list to populate before scrolling — otherwise
      // the page is still short and the target offset is past the bottom.
      requestAnimationFrame(() => window.scrollTo(0, y));
    },
  };
</script>

<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import type { Forum, Thread } from "$lib/types";
  import { platformLabel, fmtVbAbsolute, fmtVbFooterTime } from "$lib/types";
  import Breadcrumb from "$lib/components/Breadcrumb.svelte";
  import ThreadRow from "$lib/components/ThreadRow.svelte";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let focusedIndex = $state(-1);

  const PAGE_SIZE = 50;

  let forum = $state<Forum | null>(null);
  let threads = $state<Thread[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let now = $state(new Date());
  let sort = $state<"new" | "hot">("new");
  let supportsHot = $state(false);
  let pageNum = $state(1);
  let totalThreads = $derived(forum?.thread_count ?? 0);
  let totalPages = $derived(Math.max(1, Math.ceil(totalThreads / PAGE_SIZE)));

  $effect(() => {
    const id = Number(page.params.id);
    if (!Number.isFinite(id)) return;
    const currentSort = sort;
    const currentPage = pageNum;
    loading = true;
    error = null;
    api.markForumVisited(id).catch(() => {});
    Promise.all([
      api.getForum(id),
      api.getForumThreads(id, PAGE_SIZE, (currentPage - 1) * PAGE_SIZE, currentSort),
      api.forumSupportsHot(id),
    ])
      .then(([f, t, hot]) => {
        forum = f;
        threads = t;
        supportsHot = hot;
      })
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  let refreshing = $state(false);
  let markingRead = $state(false);

  function goPage(p: number) {
    if (p < 1 || p > totalPages) return;
    pageNum = p;
    if (typeof window !== "undefined") window.scrollTo(0, 0);
  }

  async function refreshThisForum() {
    if (!forum) return;
    refreshing = true;
    try {
      await api.refreshForum(forum.id);
      threads = await api.getForumThreads(forum.id, PAGE_SIZE, (pageNum - 1) * PAGE_SIZE, sort);
    } catch (e) {
      error = String(e);
    } finally {
      refreshing = false;
    }
  }

  async function markAllRead() {
    if (!forum) return;
    markingRead = true;
    // Optimistic: flip every visible thread to read locally.
    threads = threads.map((t) => ({ ...t, read: true }));
    try {
      await api.markForumThreadsRead(forum.id);
    } catch (e) {
      error = String(e);
    } finally {
      markingRead = false;
    }
  }

  function pageNumbers(current: number, total: number): (number | "…")[] {
    if (total <= 7) return Array.from({ length: total }, (_, i) => i + 1);
    const out: (number | "…")[] = [1];
    if (current > 4) out.push("…");
    const lo = Math.max(2, current - 1);
    const hi = Math.min(total - 1, current + 1);
    for (let p = lo; p <= hi; p++) out.push(p);
    if (current < total - 3) out.push("…");
    out.push(total);
    return out;
  }

  function isTypingTarget(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    if (!t) return false;
    const tag = t.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
  }

  function scrollFocusedIntoView() {
    if (focusedIndex < 0) return;
    const el = document.querySelector(`[data-thread-row-idx="${focusedIndex}"]`) as HTMLElement | null;
    if (el) el.scrollIntoView({ block: "nearest" });
  }

  function keyHandler(e: KeyboardEvent) {
    if (isTypingTarget(e)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (threads.length === 0) return;
    if (e.key === "j") {
      e.preventDefault();
      focusedIndex = Math.min(threads.length - 1, Math.max(0, focusedIndex + 1));
      scrollFocusedIntoView();
    } else if (e.key === "k") {
      e.preventDefault();
      focusedIndex = focusedIndex <= 0 ? 0 : focusedIndex - 1;
      scrollFocusedIntoView();
    } else if (e.key === "Enter" && focusedIndex >= 0) {
      e.preventDefault();
      const t = threads[focusedIndex];
      if (t) void goto(`/thread/${t.id}`);
    }
  }

  onMount(() => {
    window.addEventListener("keydown", keyHandler);
    return () => window.removeEventListener("keydown", keyHandler);
  });
</script>

<div class="vb-banner">
  <h1><a href="/" title="Back to home">{forum?.title ?? "…"}</a></h1>
  <div class="vb-actions">
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <button
      class="vb-btn"
      onclick={markAllRead}
      disabled={markingRead || !forum || loading}
      title="Mark every thread in this forum as read"
    >{markingRead ? "Marking…" : "Mark Read"}</button>
    <button
      class="vb-btn"
      onclick={refreshThisForum}
      disabled={refreshing || !forum || loading}
      title="Re-poll this forum's feed now"
    >{refreshing ? "Refreshing…" : "Refresh"}</button>
    <a class="vb-btn" href="/">&larr; Home</a>
  </div>
</div>

<div class="vb-content">
  <Breadcrumb
    crumbs={[
      { label: "Home", href: "/" },
      { label: forum?.title ?? "Forum" },
    ]}
  />

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Loading…</div></div>
  {:else if error}
    <div class="vb-group"><div class="vb-empty" style="color:var(--vb-pill-text)">{error}</div></div>
  {:else if !forum}
    <div class="vb-group"><div class="vb-empty">Forum not found.</div></div>
  {:else}
    <div class="vb-toolbar">
      <span>
        <b>{platformLabel(forum.kind)}</b> · {forum.thread_count} threads
      </span>
      <span class="vb-toolbar-sep">·</span>
      <span>last polled {fmtVbAbsolute(forum.last_polled_at)}</span>
      <span class="vb-spacer"></span>
      <span>Sort:
        <button
          class="vb-sort-toggle"
          class:active={sort === "new"}
          onclick={() => (sort = "new")}
        >New</button>
        <button
          class="vb-sort-toggle"
          class:active={sort === "hot"}
          disabled={!supportsHot}
          title={supportsHot ? "Sort by hot" : "Hot unavailable for this forum"}
          onclick={() => (sort = "hot")}
        >Hot</button>
      </span>
    </div>

    <div class="vb-group">
      <div class="vb-cat">
        <span class="caret">▼</span>
        <span>Threads in {forum.title}</span>
      </div>

      <div class="vb-subbar" style="grid-template-columns: 32px 1fr 70px 70px 200px;">
        <div></div>
        <div>Thread / Thread Starter</div>
        <div style="text-align:right">Replies</div>
        <div style="text-align:right">Views</div>
        <div>Last Post</div>
      </div>

      {#if threads.length === 0}
        <div class="vb-empty">
          <em>No threads yet.</em> The poller may still be running — refresh in a few seconds.
        </div>
      {:else}
        {#each threads as t, idx (t.id)}
          <div class:fr-row-focused={idx === focusedIndex} data-thread-row-idx={idx}>
            <ThreadRow thread={t} />
          </div>
        {/each}
      {/if}
    </div>

    <div class="vb-toolbar vb-pager" style="border-top:none;">
      <span>
        Page <b>{pageNum}</b> of <b>{totalPages}</b>
        {#if totalThreads > 0}<span class="vb-toolbar-sep">·</span>
          <span style="color:var(--vb-very-muted)">{totalThreads} threads</span>{/if}
      </span>
      <span class="vb-spacer"></span>
      <button
        class="vb-pager-btn"
        disabled={pageNum <= 1}
        onclick={() => goPage(pageNum - 1)}
      >&laquo; Prev</button>
      {#each pageNumbers(pageNum, totalPages) as p}
        {#if p === "…"}
          <span class="vb-pager-ellipsis">…</span>
        {:else}
          <button
            class="vb-pager-btn"
            class:active={p === pageNum}
            onclick={() => goPage(p as number)}
          >{p}</button>
        {/if}
      {/each}
      <button
        class="vb-pager-btn"
        disabled={pageNum >= totalPages}
        onclick={() => goPage(pageNum + 1)}
      >Next &raquo;</button>
    </div>
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>

<style>
  /* j/k highlight ring on the focused thread row. Doesn't disturb the
     row's own striping — sits as a left-edge accent + subtle background tint. */
  .fr-row-focused :global(.vb-thread-row) {
    box-shadow: inset 3px 0 0 var(--vb-link);
    background: color-mix(in srgb, var(--vb-link) 10%, transparent) !important;
  }
</style>
