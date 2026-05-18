<script lang="ts">
  import type { Thread } from "$lib/types";
  import { decodeHtmlEntities, fmtCount, fmtVbAbsolute } from "$lib/types";
  import { api } from "$lib/api";
  import { goto } from "$app/navigation";

  type Props = {
    thread: Thread;
    forum?: { id: number; title: string } | null;
  };
  let { thread, forum = null }: Props = $props();

  function openForum(e: MouseEvent, forumId: number) {
    e.preventDefault();
    e.stopPropagation();
    goto(`/forum/${forumId}`);
  }

  async function openThread(e: MouseEvent) {
    e.preventDefault();
    // Optimistic: flip read immediately so the row dims, then fire-and-forget the persist.
    if (!thread.read) {
      thread.read = true;
      api.markThreadRead(thread.id).catch(() => {});
    }
    api.recordThreadVisit(thread.id).catch(() => {});
    try {
      await api.openThread(thread.id, thread.source_url, thread.title);
    } catch (err) {
      console.error("open thread failed:", err);
      try {
        await api.openExternal(thread.source_url);
      } catch {
        window.open(thread.source_url, "_blank");
      }
    }
  }

  async function openUser(e: MouseEvent, url: string | null | undefined, title: string) {
    e.preventDefault();
    e.stopPropagation();
    if (!url) return;
    try {
      await api.openThreadByUrl(url, title);
    } catch {
      try {
        await api.openExternal(url);
      } catch {
        window.open(url, "_blank");
      }
    }
  }
</script>

<div
  class="vb-thread-row"
  class:unread={!thread.read}
  class:read={thread.read}
  data-testid="thread-row"
  data-thread-id={thread.id}
>
  <div class="icon" aria-hidden="true">
    <span class="folder folder-thread">{thread.read ? "📄" : "📬"}</span>
  </div>

  <div class="main">
    <a class="title" href={thread.source_url} onclick={openThread}>{decodeHtmlEntities(thread.title)}</a>
    <div class="meta">
      {#if thread.op_author}
        Started by
        {#if thread.op_author_url}
          <a
            class="username-link"
            href={thread.op_author_url}
            onclick={(e) => openUser(e, thread.op_author_url, thread.op_author ?? "")}
          ><b>{thread.op_author}</b></a>
        {:else}
          <b>{thread.op_author}</b>
        {/if}
      {:else}—{/if}
      {#if thread.pubdate} · {fmtVbAbsolute(thread.pubdate)}{/if}
      {#if forum}
        · in
        <a
          class="forum-link"
          href={`/forum/${forum.id}`}
          onclick={(e) => openForum(e, forum.id)}
        >{decodeHtmlEntities(forum.title)}</a>
      {/if}
    </div>
  </div>

  <div class="num replies">
    <span class="mlabel">Rep</span>
    <span class="value">{fmtCount(thread.reply_count)}</span>
  </div>

  <div class="num views">
    <span class="mlabel">View</span>
    <span class="value">—</span>
  </div>

  <div class="lastpost">
    {#if thread.last_poster || thread.last_post_at}
      {#if thread.last_poster_url && thread.last_poster}
        <a
          class="username-link"
          href={thread.last_poster_url}
          onclick={(e) => openUser(e, thread.last_poster_url, thread.last_poster ?? "")}
        ><b>{thread.last_poster}</b></a>
      {:else}
        <b>{thread.last_poster ?? "—"}</b>
      {/if}
      <div style="color:var(--vb-very-muted);font-size:10px;">{fmtVbAbsolute(thread.last_post_at)}</div>
    {:else if (thread.reply_count ?? 0) > 0}
      <!-- Activity exists (reply_count > 0) but the feed doesn't expose
           last_poster / last_post_at. Render two narrow skeleton bars so the
           eye registers "there is something here, we just don't have specifics"
           without reading repetitive placeholder text on every row. -->
      <span class="skel-line skel-line-name" aria-label="last reply details unavailable"></span>
      <span class="skel-line skel-line-time"></span>
    {:else if thread.pubdate}
      <i class="no-replies">no replies</i>
    {:else}
      —
    {/if}
  </div>
</div>

<style>
  .username-link {
    color: var(--vb-link);
    text-decoration: none;
  }
  .username-link:hover {
    color: var(--vb-link-hover);
    text-decoration: underline;
  }
  .username-link b {
    color: inherit;
    font-weight: bold;
  }
  .forum-link {
    color: var(--vb-link);
    text-decoration: none;
    font-weight: bold;
  }
  .forum-link:hover {
    color: var(--vb-link-hover);
    text-decoration: underline;
  }
  /* WHY: vBulletin convention — unread threads render with a bold title;
     visited rows dim to indicate read state. */
  :global(.vb-thread-row.unread .title) {
    font-weight: bold;
  }
  :global(.vb-thread-row.read .title) {
    font-weight: normal;
    color: var(--vb-row-read-text);
  }
  :global(.vb-thread-row.read),
  :global(.vb-thread-row.read:nth-child(even)),
  :global(.vb-thread-row.read:nth-child(odd)) {
    background: var(--vb-row-bg-read);
  }

  /* "has activity, details unavailable" — two stubby bars that mimic the
     visual rhythm of a real "username\n10 May 2026, 14:23" without resorting
     to repetitive placeholder text. Two distinct widths and shades so it
     visually echoes a name-then-timestamp stack. Colors come from theme
     vars so dark mode doesn't render light-mode link blue on a dark cell. */
  .skel-line {
    display: block;
    border-radius: 1px;
    margin: 2px 0;
    background: linear-gradient(
      90deg,
      var(--vb-skeleton-from) 0%,
      var(--vb-skeleton-mid) 50%,
      var(--vb-skeleton-from) 100%
    );
  }
  .skel-line-name {
    height: 8px;
    width: 60%;
  }
  .skel-line-time {
    height: 6px;
    width: 45%;
    opacity: 0.6;
  }
  .no-replies {
    color: var(--vb-very-muted);
    font-style: italic;
    font-size: 10px;
  }
</style>
