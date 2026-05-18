<script lang="ts">
  import { page } from "$app/state";
  import { api } from "$lib/api";
  import type { Forum, Thread } from "$lib/types";
  import { fmtVbFooterTime } from "$lib/types";
  import Breadcrumb from "$lib/components/Breadcrumb.svelte";
  import ThreadRow from "$lib/components/ThreadRow.svelte";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let results = $state<Thread[]>([]);
  let forum = $state<Forum | null>(null);
  let forumMap = $state<Record<number, { id: number; title: string }>>({});
  let loading = $state(false);
  let error = $state<string | null>(null);
  let now = $state(new Date());

  let lastSig = $state<string>("");

  $effect(() => {
    const q = (page.url.searchParams.get("q") ?? "").trim();
    const fidRaw = page.url.searchParams.get("forum_id");
    const fid = fidRaw && Number.isFinite(Number(fidRaw)) ? Number(fidRaw) : undefined;
    const sig = `${q}::${fid ?? ""}`;
    if (sig === lastSig) return;
    lastSig = sig;

    if (!q) {
      results = [];
      forum = null;
      loading = false;
      error = null;
      return;
    }
    loading = true;
    error = null;
    const fpromise = fid !== undefined ? api.getForum(fid) : Promise.resolve(null);
    // When searching across all forums, fetch the full category tree once so
    // each result row can render its parent forum without N round-trips.
    const catsPromise = fid === undefined ? api.getCategories() : Promise.resolve(null);
    Promise.all([api.searchThreads(q, fid), fpromise, catsPromise])
      .then(([rs, f, cats]) => {
        results = rs;
        forum = f;
        const map: Record<number, { id: number; title: string }> = {};
        if (cats) {
          for (const c of cats) {
            for (const fm of c.forums) {
              map[fm.id] = { id: fm.id, title: fm.title };
            }
          }
        }
        forumMap = map;
      })
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  let q = $derived((page.url.searchParams.get("q") ?? "").trim());
</script>

<div class="vb-banner" data-testid="search-banner">
  <h1>
    <a href={forum ? `/forum/${forum.id}` : "/"} title={forum ? `Back to ${forum.title}` : "Back to home"}>
      Search Results for "{q}"{#if forum} &mdash; in {forum.title}{/if}
    </a>
  </h1>
  <div class="vb-actions">
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <a class="vb-btn" href="/">&larr; Home</a>
  </div>
</div>

<div class="vb-content">
  <Breadcrumb
    crumbs={[
      { label: "Home", href: "/" },
      ...(forum ? [{ label: forum.title, href: `/forum/${forum.id}` }] : []),
      { label: `Search: ${q}` },
    ]}
  />

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Searching…</div></div>
  {:else if error}
    <div class="vb-group"><div class="vb-empty" style="color:var(--vb-pill-text)">{error}</div></div>
  {:else if !q}
    <div class="vb-group"><div class="vb-empty">Enter a query in the search box above.</div></div>
  {:else}
    <div class="vb-toolbar">
      <span>
        <b>{results.length}</b> {results.length === 1 ? "result" : "results"}
        {#if forum}in <b>{forum.title}</b>{:else}across all forums{/if}
      </span>
      <span class="vb-spacer"></span>
      {#if forum}
        <a href={`/search?q=${encodeURIComponent(q)}`}>Search all forums</a>
      {/if}
    </div>

    <div class="vb-group" data-testid="search-results">
      <div class="vb-cat">
        <span class="caret">▼</span>
        <span>Matching Threads</span>
      </div>

      <div class="vb-subbar" style="grid-template-columns: 32px 1fr 70px 70px 200px;">
        <div></div>
        <div>Thread / Thread Starter</div>
        <div style="text-align:right">Replies</div>
        <div style="text-align:right">Views</div>
        <div>Last Post</div>
      </div>

      {#if results.length === 0}
        <div class="vb-empty" data-testid="search-empty">
          {#if forum}
            <em>No matches in this forum.</em>
          {:else}
            <em>No matches across all forums.</em>
          {/if}
        </div>
      {:else}
        {#each results as t (t.id)}
          <ThreadRow thread={t} forum={forum ? null : (forumMap[t.forum_id] ?? null)} />
        {/each}
      {/if}
    </div>
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>
