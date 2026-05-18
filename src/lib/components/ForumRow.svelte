<script lang="ts">
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import type { Forum } from "$lib/types";
  import { decodeHtmlEntities, fmtCount, fmtVbAbsolute } from "$lib/types";

  type Props = {
    forum: Forum;
    onShowError?: (forum: Forum) => void;
  };
  let { forum = $bindable(), onShowError }: Props = $props();

  function openError(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    onShowError?.(forum);
  }

  async function click(e: MouseEvent) {
    e.preventDefault();
    if (forum.unread) {
      forum = { ...forum, unread: false, last_visited_at: Math.floor(Date.now() / 1000) };
    }
    try {
      await api.markForumVisited(forum.id);
    } catch {
      // ignore — UI already reflects visited state
    }
    goto(`/forum/${forum.id}`);
  }

  async function openLatest(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (!forum.latest_thread_url) return;
    // Prefer the native viewer when we already know this thread locally
    // (forum has been polled). Fall back to the webview overlay if the URL
    // isn't in our threads table yet.
    try {
      const id = await api.findThreadIdBySourceUrl(forum.latest_thread_url);
      if (id != null) {
        api.recordThreadVisit(id).catch(() => {});
        await api.openThread(id, forum.latest_thread_url, forum.latest_thread_title ?? forum.title);
        return;
      }
    } catch {
      // fall through to webview overlay
    }
    try {
      await api.openThreadByUrl(forum.latest_thread_url, forum.latest_thread_title ?? forum.title);
    } catch {
      try {
        await api.openExternal(forum.latest_thread_url);
      } catch {
        window.open(forum.latest_thread_url, "_blank");
      }
    }
  }

  // A forum that has errored once but never succeeded — show em-dashes for counts.
  let neverPolled = $derived(!forum.last_polled_at || (forum.thread_count === 0 && !!forum.last_error));
</script>

<div
  class="vb-forum-row"
  class:unread={forum.unread}
  data-testid="forum-row"
  data-forum-id={forum.id}
>
  <div class="icon" aria-hidden="true">
    <span class="folder" class:folder-open={forum.unread} class:folder-closed={!forum.unread}>
      {forum.unread ? "📂" : "📁"}
    </span>
  </div>

  <div class="main">
    <a class="title" href={`/forum/${forum.id}`} onclick={click}>{forum.title}</a>
    {#if forum.description}
      <div class="desc">{forum.description}</div>
    {/if}
  </div>

  <div class="count threads">
    <span class="value">{neverPolled ? "—" : fmtCount(forum.thread_count)}</span>
    <span class="label">Threads</span>
  </div>

  <div class="count posts">
    <span class="value">{neverPolled ? "—" : fmtCount(forum.post_count)}</span>
    <span class="label">Posts</span>
  </div>

  <div class="lastpost">
    {#if forum.latest_thread_title}
      <a class="lp-title" href={forum.latest_thread_url ?? "#"} onclick={openLatest}>
        Re: {decodeHtmlEntities(forum.latest_thread_title)}
      </a>
      <div class="lp-by">
        by <span class="lp-author">{forum.latest_thread_author ?? "—"}</span>
      </div>
      <div class="lp-when">{fmtVbAbsolute(forum.latest_thread_at)}</div>
    {:else if forum.last_error}
      <button
        type="button"
        class="lp-unavailable"
        title="Click for details"
        onclick={openError}
      >feed temporarily unavailable <span class="lp-unavailable-icon">ⓘ</span></button>
    {:else}
      <span class="lp-empty"><em>No posts yet</em></span>
    {/if}
  </div>
</div>
