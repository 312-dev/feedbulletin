<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";

  let query = $state("");
  let thisForumOnly = $state(true);

  let onForumPage = $derived(/^\/forum\/\d+/.test(page.url.pathname));
  let currentForumId = $derived(
    onForumPage ? Number(page.url.pathname.split("/")[2]) : null
  );

  function submit(e: Event) {
    e.preventDefault();
    const q = query.trim();
    if (!q) return;
    const params = new URLSearchParams();
    params.set("q", q);
    if (onForumPage && thisForumOnly && currentForumId) {
      params.set("forum_id", String(currentForumId));
    }
    goto(`/search?${params.toString()}`);
  }
</script>

<form class="search-box" onsubmit={submit}>
  {#if onForumPage}
    <label class="this-forum-only">
      <input type="checkbox" bind:checked={thisForumOnly} />
      this forum only
    </label>
  {/if}
  <input
    type="search"
    class="search-input"
    placeholder="Search…"
    bind:value={query}
    aria-label="Search forums"
  />
  <button type="submit" class="vb-btn search-go" aria-label="Search">
    &#x1F50D;
  </button>
</form>

<style>
  .search-box {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .this-forum-only {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.85);
    display: inline-flex;
    align-items: center;
    gap: 3px;
    cursor: pointer;
    user-select: none;
  }
  .this-forum-only input {
    margin: 0;
    transform: scale(0.85);
  }
  .search-input {
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 11px;
    padding: 1px 6px;
    height: 20px;
    line-height: 18px;
    box-sizing: border-box;
    border: 1px solid #c8cdd3;
    border-radius: 2px;
    background: #ffffff;
    color: #000;
    width: 160px;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.1);
    -webkit-appearance: none;
    /* Explicit color-scheme keeps WebKit from auto-restyling the placeholder
       in link-blue when the document is data-theme="dark". */
    color-scheme: light;
  }
  /* Force a neutral gray placeholder — without this the macOS WKWebView
     paints the placeholder using the parent color-scheme's accent (blue
     in dark mode) which clashes with the white input. */
  .search-input::placeholder {
    color: #888;
    opacity: 1;
  }
  .search-input:focus {
    outline: 1px solid #22229c;
    outline-offset: -1px;
  }
  .search-go {
    padding: 3px 8px;
    font-size: 12px;
    line-height: 1;
  }
</style>
