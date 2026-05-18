<script lang="ts">
  import { page } from "$app/state";
  import { api } from "$lib/api";
  import type { Category, Thread } from "$lib/types";
  import { fmtVbFooterTime } from "$lib/types";
  import Breadcrumb from "$lib/components/Breadcrumb.svelte";
  import ThreadRow from "$lib/components/ThreadRow.svelte";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  const PAGE_SIZE = 50;

  let category = $state<Category | null>(null);
  let threads = $state<Thread[]>([]);
  let forumMap = $state<Record<number, { id: number; title: string }>>({});
  let total = $state(0);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let now = $state(new Date());
  let sort = $state<"new" | "hot">("new");
  let supportsHot = $state(false);
  let pageNum = $state(1);
  let totalPages = $derived(Math.max(1, Math.ceil(total / PAGE_SIZE)));

  $effect(() => {
    const id = Number(page.params.id);
    if (!Number.isFinite(id)) return;
    const currentSort = sort;
    const currentPage = pageNum;
    loading = true;
    error = null;
    Promise.all([
      api.getCategory(id),
      api.getCategoryThreads(id, PAGE_SIZE, (currentPage - 1) * PAGE_SIZE, currentSort),
      api.categoryThreadCount(id),
      api.categorySupportsHot(id),
      api.getCategories(),
    ])
      .then(([c, t, n, hot, cats]) => {
        category = c;
        threads = t;
        total = n;
        supportsHot = hot;
        const map: Record<number, { id: number; title: string }> = {};
        for (const cc of cats) {
          for (const fm of cc.forums) {
            map[fm.id] = { id: fm.id, title: fm.title };
          }
        }
        forumMap = map;
      })
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  function goPage(p: number) {
    if (p < 1 || p > totalPages) return;
    pageNum = p;
    if (typeof window !== "undefined") window.scrollTo(0, 0);
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
</script>

<div class="vb-banner">
  <h1><a href="/" title="Back to home">{category?.name ?? "…"}</a></h1>
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
      { label: category?.name ?? "Category" },
    ]}
  />

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Loading…</div></div>
  {:else if error}
    <div class="vb-group"><div class="vb-empty" style="color:var(--vb-pill-text)">{error}</div></div>
  {:else if !category}
    <div class="vb-group"><div class="vb-empty">Category not found.</div></div>
  {:else}
    <div class="vb-toolbar">
      <span><b>All threads in {category.name}</b></span>
      <span class="vb-toolbar-sep">·</span>
      <span>{total} threads aggregated</span>
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
          onclick={() => (sort = "hot")}
        >Hot</button>
      </span>
    </div>

    <div class="vb-group">
      <div class="vb-cat">
        <span class="caret">▼</span>
        <span>Recent activity across {category.name}</span>
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
          <em>No threads loaded yet.</em>
        </div>
      {:else}
        {#each threads as t (t.id)}
          <ThreadRow thread={t} forum={forumMap[t.forum_id] ?? null} />
        {/each}
      {/if}
    </div>

    <div class="vb-toolbar vb-pager" style="border-top:none;">
      <span>
        Page <b>{pageNum}</b> of <b>{totalPages}</b>
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
