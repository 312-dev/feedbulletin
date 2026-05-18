<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { api } from "$lib/api";
  import { decodeHtmlEntities } from "$lib/types";
  import type {
    Forum,
    Thread,
    Post,
    PostsReadyPayload,
    AvatarsResolvedPayload,
    DebugField,
    ProfileRevision,
  } from "$lib/types";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import NativeThreadView from "$lib/components/NativeThreadView.svelte";
  import DebugComplaintDialog from "$lib/components/DebugComplaintDialog.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";

  let thread = $state<Thread | null>(null);
  let forum = $state<Forum | null>(null);
  let siblings = $state<Thread[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let bootedWebview = $state(false);
  let favoriteIndexes = $state<Set<number>>(new Set());
  let lastReadPostIndex = $state<number | null>(null);

  // Native-render state. Default: webview until we know the host has a profile
  // OR we get a posts_ready event. Once we have posts, render mode follows
  // per-host preference (or "native" if unset).
  let posts = $state<Post[] | null>(null);
  let renderMode = $state<"native" | "webview">("webview");
  // Compact mode persists in localStorage — it's a UI density preference,
  // not a per-host scrape choice. Read on mount via $effect so SSR doesn't
  // touch window.
  let compactMode = $state(false);
  function toggleCompact() {
    compactMode = !compactMode;
    try {
      if (typeof window !== "undefined") {
        window.localStorage.setItem("fb-compact-mode", compactMode ? "1" : "0");
      }
    } catch {
      // localStorage can throw in private mode / disabled storage — ignore.
    }
  }
  let hostForCurrent = $state<string>("");
  let contentTypeForCurrent = $state<string>("");
  let renderPrefs = $state<Record<string, "native" | "webview">>({});
  let learnState = $state<"idle" | "learning" | "failed">("idle");
  let postsReadyUnlisten: UnlistenFn | null = null;
  let postsUnavailableUnlisten: UnlistenFn | null = null;
  let avatarsResolvedUnlisten: UnlistenFn | null = null;

  // Debugger mode — click-to-refine the site profile.
  let debugMode = $state(false);
  let debugMenuOpen = $state(false);
  let debugCapture = $state<{ field: DebugField; html: string } | null>(null);
  let refineSubmitting = $state(false);
  let refineError = $state<string | null>(null);
  let revisions = $state<ProfileRevision[]>([]);
  let canUndo = $derived(
    revisions.length > 1 &&
      revisions.find((r) => r.is_current)?.parent_revision_id != null
  );
  let canReset = $derived(
    revisions.length > 1 && revisions.find((r) => r.is_current)?.parent_revision_id != null
  );

  function toggleDebugMenu(e: MouseEvent) {
    e.stopPropagation();
    debugMenuOpen = !debugMenuOpen;
    if (debugMenuOpen) void refreshRevisions();
  }
  function closeDebugMenu() { debugMenuOpen = false; }

  function pickInspect() {
    debugMode = !debugMode;
    debugMenuOpen = false;
  }
  async function pickUndo() {
    debugMenuOpen = false;
    await undoLast();
  }
  async function pickReset() {
    debugMenuOpen = false;
    await resetAll();
  }
  async function pickClearAll() {
    debugMenuOpen = false;
    if (!hostForCurrent) return;
    try { await api.clearLayoutForHost(hostForCurrent); } catch {}
    // Force a fresh learn by re-opening the webview; learnState pill will reappear.
    learnState = "learning";
    posts = null;
    contentTypeForCurrent = "";
    if (thread) {
      renderMode = "webview";
      await api.openThreadWebview(thread.source_url, {
        threadId: thread.id,
        platformHint: forum?.kind,
      });
    }
  }

  async function refreshRevisions() {
    if (!hostForCurrent || !contentTypeForCurrent) return;
    try {
      revisions = await api.listSiteProfileRevisions(hostForCurrent, contentTypeForCurrent);
    } catch {
      revisions = [];
    }
  }

  async function onDebugClick(c: { field: DebugField; html: string }) {
    debugCapture = c;
  }

  // Lazy-load: bottom sentinel asks for more. The backend looks at the
  // active site_profile's load_more_strategy and either clicks the
  // load-more button, scrolls the WebView, or navigates to next page.
  let lazyLoadInFlight = $state(false);
  let lazyLoadCount = $state(0);
  let moreAvailable = $state(true);
  let lastPostCountBeforeLoad = $state(0);
  const LAZY_LOAD_CAP = 8; // upper bound per thread session to keep cost sane

  async function onNeedMore() {
    if (!thread || !moreAvailable) return;
    if (lazyLoadInFlight) return;
    if (lazyLoadCount >= LAZY_LOAD_CAP) {
      moreAvailable = false;
      return;
    }
    lazyLoadInFlight = true;
    lazyLoadCount += 1;
    lastPostCountBeforeLoad = posts?.length ?? 0;
    try {
      await api.loadMorePosts(thread.id);
      // posts_ready will fire when the re-scrape completes; the listener
      // updates `posts` automatically. The $effect below detects whether
      // new posts actually arrived and flips moreAvailable to false if not.
    } catch {} finally {
      // Release in-flight after a short delay so the IntersectionObserver
      // sees the new bottom and doesn't immediately re-fire.
      setTimeout(() => { lazyLoadInFlight = false; }, 1500);
    }
  }

  // After each posts update, if we recently asked for more and the count
  // didn't grow, the strategy is exhausted — stop showing the spinner.
  $effect(() => {
    if (!posts) return;
    if (lazyLoadCount === 0) return;
    if (!lazyLoadInFlight && posts.length === lastPostCountBeforeLoad && lazyLoadCount > 0) {
      moreAvailable = false;
    }
  });

  async function onDebugSubmit(field: DebugField, complaint: string) {
    if (!hostForCurrent) {
      refineError = "no host context";
      return;
    }
    // Fallback when the posts_ready listener never set contentTypeForCurrent
    // (e.g. cached posts loaded on mount, or host-rewrite mismatch).
    if (!contentTypeForCurrent) {
      try {
        const profiles = await api.getSiteProfile(hostForCurrent);
        if (profiles.length > 0) contentTypeForCurrent = profiles[0].content_type;
      } catch {}
    }
    if (!contentTypeForCurrent) {
      refineError = "no profile yet — render the thread once before refining";
      return;
    }
    refineError = null;
    refineSubmitting = true;
    try {
      const r = await api.refineSiteProfile(
        hostForCurrent,
        contentTypeForCurrent,
        field,
        debugCapture?.html ?? "",
        complaint
      );
      // The refine call may produce a NEW content_type if the LLM decides this
      // page actually belongs to a different shape — track that.
      contentTypeForCurrent = r.content_type;
      // Re-trigger extraction with the new selectors by reopening the webview.
      if (thread) {
        renderMode = "webview";
        await api.openThreadWebview(thread.source_url, {
          threadId: thread.id,
          platformHint: forum?.kind,
        });
      }
      await refreshRevisions();
      debugCapture = null;
    } catch (e) {
      refineError = String(e);
    } finally {
      refineSubmitting = false;
    }
  }

  function cancelDebug() {
    debugCapture = null;
    refineError = null;
  }

  async function undoLast() {
    if (!hostForCurrent || !contentTypeForCurrent) return;
    const ok = await api.undoSiteProfileRevision(hostForCurrent, contentTypeForCurrent);
    if (ok && thread) {
      renderMode = "webview";
      await api.openThreadWebview(thread.source_url, {
        threadId: thread.id,
        platformHint: forum?.kind,
      });
      await refreshRevisions();
    }
  }

  async function retryLearn() {
    if (!hostForCurrent || !thread) return;
    learnState = "learning";
    try {
      // Drop any cached profile for this host so the next scrape forces a
      // fresh Anthropic learn, then re-open the webview to trigger the scrape.
      await api.relearnSiteProfile(hostForCurrent);
    } catch {}
    renderMode = "webview";
    await api.openThreadWebview(thread.source_url, {
      threadId: thread.id,
      platformHint: forum?.kind,
    });
  }

  async function resetAll() {
    if (!hostForCurrent || !contentTypeForCurrent) return;
    const ok = await api.resetSiteProfile(hostForCurrent, contentTypeForCurrent);
    if (ok && thread) {
      renderMode = "webview";
      await api.openThreadWebview(thread.source_url, {
        threadId: thread.id,
        platformHint: forum?.kind,
      });
      await refreshRevisions();
    }
  }

  let currentIndex = $derived(
    thread ? siblings.findIndex((t) => t.id === thread!.id) : -1
  );
  let prevThread = $derived(
    currentIndex > 0 ? siblings[currentIndex - 1] : null
  );
  let nextThread = $derived(
    currentIndex >= 0 && currentIndex < siblings.length - 1
      ? siblings[currentIndex + 1]
      : null
  );

  function hostOf(url: string): string {
    try {
      const h = new URL(url).host.toLowerCase();
      // Reddit URLs get server-side rewritten to old.reddit.com before the
      // WebView opens. Align so hostForCurrent (from the stored thread URL)
      // and posts_ready.url (from the live WebView) compare equal.
      if (h === "reddit.com" || h === "www.reddit.com" || h === "new.reddit.com" || h === "np.reddit.com") {
        return "old.reddit.com";
      }
      return h;
    } catch {
      return "";
    }
  }

  function applyPreferenceFor(host: string) {
    const pref = renderPrefs[host];
    // If we have a posts payload already and pref isn't explicitly "webview", go native.
    if (posts && pref !== "webview") {
      switchToNative();
    } else if (pref === "native" && posts) {
      switchToNative();
    } else {
      // either no posts yet or pref forces webview
      renderMode = pref === "native" && posts ? "native" : "webview";
    }
  }

  async function switchToNative() {
    renderMode = "native";
    // Keep the webview alive (1x1) so lazy-load can drive it. Frontend
    // NativeThreadView covers the full content area visually.
    await api.shrinkThreadWebview();
  }
  async function switchToWebview() {
    renderMode = "webview";
    // The webview is kept alive (shrunken) when we go native, so most of
    // the time restoring is enough. If it doesn't exist (e.g. user toggled
    // from a fresh-cached-load that never opened it), open it.
    await api.restoreThreadWebview();
    if (!bootedWebview && thread) {
      await api.openThreadWebview(thread.source_url, {
        threadId: thread.id,
        platformHint: forum?.kind,
      });
      bootedWebview = true;
    }
  }
  async function toggleMode() {
    const next = renderMode === "native" ? "webview" : "native";
    await api.setRenderPreference(hostForCurrent, next);
    renderPrefs = { ...renderPrefs, [hostForCurrent]: next };
    if (next === "native") {
      if (!posts && thread) {
        // We haven't received posts yet; warm up by leaving webview open until they arrive.
        renderMode = "webview";
      } else {
        await switchToNative();
      }
    } else {
      await switchToWebview();
    }
  }

  async function load(id: number) {
    loading = true;
    error = null;
    posts = null;
    try {
      const t = await api.getThread(id);
      if (!t) throw new Error("thread not found");
      thread = t;
      hostForCurrent = hostOf(t.source_url);
      const [f, list, cached, prefs, favIdx, lastRead] = await Promise.all([
        api.getForum(t.forum_id),
        api.getForumThreads(t.forum_id, 100, 0),
        api.getCachedPosts(t.id),
        api.getRenderPreferences(),
        api.listThreadFavoriteIndexes(t.id).catch(() => [] as number[]),
        api.getLastReadPostIndex(t.id).catch(() => null),
      ]);
      forum = f;
      siblings = list;
      renderPrefs = prefs ?? {};
      favoriteIndexes = new Set(favIdx);
      lastReadPostIndex = lastRead;

      const pref = renderPrefs[hostForCurrent];
      if (cached && cached.posts && cached.posts.length > 0) {
        posts = cached.posts;
        // Default to native when we already have posts AND user hasn't opted out
        if (pref !== "webview") {
          renderMode = "native";
          // Don't boot the webview at all — native render is the home.
          bootedWebview = true;
          return;
        }
      }

      // Otherwise boot the webview; scrape pipeline will emit posts_ready later.
      renderMode = "webview";
      learnState = "learning";
      await api.openThreadWebview(t.source_url, {
        threadId: t.id,
        platformHint: f?.kind,
      });
      bootedWebview = true;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function back() {
    // Best-effort: mark all loaded posts as read on the way out so the user
    // doesn't see a "New posts" divider next visit covering posts they just
    // saw. Highest post_index across the loaded payload.
    if (thread && posts && posts.length > 0 && renderMode === "native") {
      const highest = posts.reduce((max, p, i) => {
        const n = p.post_number ?? i + 1;
        return n > max ? n : max;
      }, 0);
      if (highest > (lastReadPostIndex ?? 0)) {
        api.setLastReadPostIndex(thread.id, highest).catch(() => {});
      }
    }
    void api.closeThreadWebview().finally(() => {
      // Prefer history.back so SvelteKit's snapshot restoration runs on the
      // previous route (e.g. forum-threads list scroll position). Only fall
      // back to an explicit goto when there's no prior history entry — i.e.
      // the thread page was the very first thing opened in this session.
      if (typeof window !== "undefined" && window.history.length > 1) {
        window.history.back();
      } else if (forum) {
        goto(`/forum/${forum.id}`);
      } else {
        goto("/");
      }
    });
  }

  async function gotoSibling(s: Thread | null) {
    if (!s) return;
    await goto(`/thread/${s.id}`);
  }

  async function openExternal() {
    if (!thread) return;
    try {
      await api.openExternal(thread.source_url);
    } catch {
      window.open(thread.source_url, "_blank");
    }
  }

  function isTypingTarget(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    if (!t) return false;
    const tag = t.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
  }

  function scrollToPost(n: number) {
    const el = document.getElementById(`post-${n}`);
    if (el) el.scrollIntoView({ block: "start", behavior: "smooth" });
  }

  function focusedPostNumber(): number | null {
    // The "current" post is the topmost one whose top is at or above the
    // viewport's top + a small fudge. Maps to NativeThreadView's id="post-N".
    if (!posts || posts.length === 0) return null;
    const fudge = 80;
    for (let i = posts.length - 1; i >= 0; i--) {
      const n = posts[i].post_number ?? i + 1;
      const el = document.getElementById(`post-${n}`);
      if (!el) continue;
      const top = el.getBoundingClientRect().top;
      if (top <= fudge) return n;
    }
    return posts[0].post_number ?? 1;
  }

  function keyHandler(e: KeyboardEvent) {
    if (isTypingTarget(e)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "Escape") {
      e.preventDefault();
      back();
    } else if (e.key === "ArrowLeft" && prevThread) {
      e.preventDefault();
      void gotoSibling(prevThread);
    } else if (e.key === "ArrowRight" && nextThread) {
      e.preventDefault();
      void gotoSibling(nextThread);
    } else if (renderMode === "native" && posts && posts.length > 0) {
      // Vim-style post-to-post nav inside a thread.
      if (e.key === "j") {
        e.preventDefault();
        const cur = focusedPostNumber() ?? (posts[0].post_number ?? 1);
        const idx = posts.findIndex((p, i) => (p.post_number ?? i + 1) === cur);
        if (idx >= 0 && idx + 1 < posts.length) {
          const next = posts[idx + 1];
          scrollToPost(next.post_number ?? idx + 2);
        }
      } else if (e.key === "k") {
        e.preventDefault();
        const cur = focusedPostNumber() ?? (posts[0].post_number ?? 1);
        const idx = posts.findIndex((p, i) => (p.post_number ?? i + 1) === cur);
        if (idx > 0) {
          const prev = posts[idx - 1];
          scrollToPost(prev.post_number ?? idx);
        }
      } else if (e.key === "g" && !e.shiftKey) {
        e.preventDefault();
        scrollToPost(posts[0].post_number ?? 1);
      } else if (e.key === "G" || (e.key === "g" && e.shiftKey)) {
        e.preventDefault();
        const last = posts[posts.length - 1];
        scrollToPost(last.post_number ?? posts.length);
      }
    }
  }

  // Advance the per-thread "last read post" watermark to the highest
  // post_index whose top is above the current viewport. Persists to DB
  // with light debouncing so we don't write on every scroll tick.
  let scrollWatermarkTimer: ReturnType<typeof setTimeout> | null = null;
  let dividerScrolledTo = $state(false);
  function maybeAdvanceWatermark() {
    if (!thread || !posts || posts.length === 0 || renderMode !== "native") return;
    if (scrollWatermarkTimer) return;
    scrollWatermarkTimer = setTimeout(() => {
      scrollWatermarkTimer = null;
      const fudge = 80;
      let highest = lastReadPostIndex ?? 0;
      for (let i = posts!.length - 1; i >= 0; i--) {
        const n = posts![i].post_number ?? i + 1;
        const el = document.getElementById(`post-${n}`);
        if (!el) continue;
        const top = el.getBoundingClientRect().top;
        if (top <= fudge) {
          if (n > highest) highest = n;
          break;
        }
      }
      if (highest > (lastReadPostIndex ?? 0)) {
        const newVal = highest;
        lastReadPostIndex = newVal;
        api.setLastReadPostIndex(thread!.id, newVal).catch(() => {});
      }
    }, 600);
  }
  function onThreadScroll() { maybeAdvanceWatermark(); }

  // On reopen, scroll to the "New posts" divider (if any) instead of the top,
  // so the user lands right where they left off.
  $effect(() => {
    void posts;
    if (renderMode !== "native") return;
    if (!posts || posts.length === 0) return;
    if (dividerScrolledTo) return;
    if (!lastReadPostIndex || lastReadPostIndex <= 0) return;
    if (typeof window === "undefined") return;
    // Only scroll if the URL hash didn't already pick a target.
    if (window.location.hash) return;
    requestAnimationFrame(() => {
      const el = document.querySelector("[data-testid=new-posts-divider]") as HTMLElement | null;
      if (el) {
        el.scrollIntoView({ block: "start" });
        dividerScrolledTo = true;
      }
    });
  });

  // Whenever native posts render, honour any #post-N hash in the URL by
  // scrolling that post into view. Used by /favorites → "View in thread →".
  let hashHonored = $state(false);
  $effect(() => {
    void posts;
    if (renderMode !== "native") return;
    if (!posts || posts.length === 0) return;
    if (hashHonored) return;
    if (typeof window === "undefined") return;
    const m = /^#post-(\d+)$/.exec(window.location.hash);
    if (!m) return;
    requestAnimationFrame(() => {
      const el = document.getElementById(`post-${m[1]}`);
      if (el) {
        el.scrollIntoView({ block: "start" });
        hashHonored = true;
      }
    });
  });

  onMount(() => {
    try {
      compactMode = window.localStorage.getItem("fb-compact-mode") === "1";
    } catch {
      // ignore — localStorage may be disabled
    }
    window.addEventListener("keydown", keyHandler);
    listen<PostsReadyPayload>("posts_ready", (e) => {
      const payload = e.payload;
      if (!thread || hostOf(payload.url) !== hostForCurrent) return;
      posts = payload.posts;
      contentTypeForCurrent = payload.content_type;
      learnState = "idle";
      const pref = renderPrefs[hostForCurrent];
      if (pref !== "webview") {
        void switchToNative();
      }
    }).then((un) => { postsReadyUnlisten = un; });
    listen<string>("posts_unavailable", (e) => {
      // Match by host so a stale event from a previous thread doesn't flip state.
      const evHost = (e.payload || "").toLowerCase();
      const remapped = evHost === "reddit.com" || evHost === "www.reddit.com" ? "old.reddit.com" : evHost;
      if (!hostForCurrent || remapped !== hostForCurrent) return;
      // Only mark failed if we don't already have posts (e.g. a stale earlier
      // miss on a host where a subsequent visit succeeded).
      if (!posts) learnState = "failed";
    }).then((un) => { postsUnavailableUnlisten = un; });
    listen<AvatarsResolvedPayload>("avatars_resolved", (e) => {
      const payload = e.payload;
      if (!posts || !hostForCurrent) return;
      if (hostOf(`https://${payload.host}/`) !== hostForCurrent && payload.host !== hostForCurrent) return;
      const byAuthor = new Map(payload.updates.map((u) => [u.author, u]));
      let any = false;
      const next = posts.map((p) => {
        const fresh = byAuthor.get(p.author);
        if (!fresh) return p;
        // Only patch fields that aren't already populated from thread-page
        // extraction. Thread-side data is more current to this page.
        const patched = { ...p };
        let changed = false;
        if (fresh.avatar_url && !patched.avatar_url) { patched.avatar_url = fresh.avatar_url; changed = true; }
        if (fresh.post_count && !patched.author_post_count) { patched.author_post_count = fresh.post_count; changed = true; }
        if (fresh.karma && !patched.author_karma) { patched.author_karma = fresh.karma; changed = true; }
        if (fresh.join_date && !patched.author_join_date) { patched.author_join_date = fresh.join_date; changed = true; }
        if (fresh.last_active && !patched.author_last_active) { patched.author_last_active = fresh.last_active; changed = true; }
        if (fresh.location && !patched.author_location) { patched.author_location = fresh.location; changed = true; }
        if (fresh.rank && !patched.author_rank) { patched.author_rank = fresh.rank; changed = true; }
        if (changed) any = true;
        return changed ? patched : p;
      });
      if (any) posts = next;
    }).then((un) => { avatarsResolvedUnlisten = un; });
    function onDocClick(e: MouseEvent) {
      // Dismiss the Debug dropdown when clicking anywhere outside it.
      if (!debugMenuOpen) return;
      const target = e.target as HTMLElement | null;
      if (target && target.closest(".tb-debug-wrap")) return;
      debugMenuOpen = false;
    }
    document.addEventListener("click", onDocClick);
    return () => {
      window.removeEventListener("keydown", keyHandler);
      document.removeEventListener("click", onDocClick);
    };
  });

  onDestroy(() => {
    if (postsReadyUnlisten) postsReadyUnlisten();
    if (postsUnavailableUnlisten) postsUnavailableUnlisten();
    if (avatarsResolvedUnlisten) avatarsResolvedUnlisten();
    // Always tear down the native webview when the route is unmounted (e.g.
    // when SPA navigation moves us off /thread/N).
    void api.closeThreadWebview();
  });

  $effect(() => {
    const id = Number(page.params.id);
    if (Number.isFinite(id)) {
      void load(id);
    }
  });

  function truncate(s: string, n: number): string {
    return s.length <= n ? s : s.slice(0, n - 1) + "…";
  }
</script>

<div class="fr-header" data-testid="thread-header">
  <div class="fr-brand-row">
    <a href="/" class="fr-brand" onclick={(e) => { e.preventDefault(); back(); }}>feedBulletin</a>
    <span class="fr-spacer"></span>
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <ThemeToggle />
  </div>
  <div class="thread-toolbar" data-testid="thread-toolbar">
    <button class="tb-btn" onclick={back} title="Back to forum (Esc)">&larr; Forum</button>
    <button class="tb-btn" onclick={() => gotoSibling(prevThread)} disabled={!prevThread} title="Previous (←)">&lsaquo; Prev</button>
    <button class="tb-btn" onclick={() => gotoSibling(nextThread)} disabled={!nextThread} title="Next (→)">Next &rsaquo;</button>

    <div class="tb-crumb">
      {#if forum}
        <a class="tb-crumb-mid" href={`/forum/${forum.id}`} onclick={(e) => { e.preventDefault(); back(); }}>{forum.title}</a>
      {/if}
      {#if thread}
        <span class="tb-crumb-sep">&rsaquo;</span>
        <span class="tb-crumb-title" title={decodeHtmlEntities(thread.title)}>{truncate(decodeHtmlEntities(thread.title), 80)}</span>
      {/if}
    </div>

    <span class="tb-spacer"></span>
    {#if learnState === "learning" && !posts}
      <span class="tb-stat tb-stat-learning" title="Asking Claude to learn this site's layout">
        [ Learning layout<span class="tb-dots">...</span> ]
      </span>
    {:else if learnState === "failed" && !posts}
      <button class="tb-stat tb-stat-failed" onclick={retryLearn} title="The selectors didn't extract any posts. Click to re-learn from scratch.">
        [ Couldn't extract — Retry ]
      </button>
    {/if}
    {#if renderMode === "native" && posts && posts.length > 0}
      <div class="tb-debug-wrap">
        <button
          class="tb-btn tb-debug"
          class:tb-debug-on={debugMode}
          onclick={toggleDebugMenu}
          title="Debug tools"
          aria-haspopup="menu"
          aria-expanded={debugMenuOpen}
        >🐛 Debug ▾</button>
        {#if debugMenuOpen}
          <div class="tb-debug-menu" role="menu">
            <button class="tb-debug-item" role="menuitem" onclick={pickInspect}>
              {debugMode ? "✓ " : "  "}Inspect element to refine
            </button>
            <button class="tb-debug-item" role="menuitem" onclick={pickUndo} disabled={!canUndo}>
              ↩ Undo last refinement
            </button>
            <button class="tb-debug-item" role="menuitem" onclick={pickReset} disabled={!canReset}>
              ⤺ Reset to original learned profile
            </button>
            <div class="tb-debug-sep"></div>
            <button class="tb-debug-item tb-debug-danger" role="menuitem" onclick={pickClearAll}>
              🗑 Clear ALL layout data for this site
            </button>
          </div>
        {/if}
      </div>
    {/if}
    {#if posts && posts.length > 0}
      <button
        class="tb-btn tb-toggle"
        class:tb-toggle-on={renderMode === "native"}
        onclick={toggleMode}
        title={renderMode === "native" ? "Showing native render. Click for original site." : "Showing original site. Click for native render."}
      >
        {renderMode === "native" ? "◉ Native" : "○ Native"}
      </button>
    {/if}
    {#if renderMode === "native" && posts && posts.length > 0}
      <button
        class="tb-btn tb-toggle"
        class:tb-toggle-on={compactMode}
        onclick={toggleCompact}
        title={compactMode ? "Compact mode on. Click to show avatars & signatures." : "Compact mode off. Click to hide avatars & signatures."}
      >
        {compactMode ? "▤ Compact" : "▦ Compact"}
      </button>
    {/if}
    <button class="tb-btn" onclick={openExternal} title="Open in browser">&uarr; Browser</button>
  </div>
</div>

{#if renderMode === "native" && posts}
  <div class="thread-native-scroll" onscroll={onThreadScroll}>
    <NativeThreadView
      {posts}
      title={thread?.title ?? ""}
      debug={debugMode}
      onDebugClick={onDebugClick}
      onNeedMore={onNeedMore}
      loadingMore={lazyLoadInFlight}
      {moreAvailable}
      threadId={thread?.id}
      threadSourceUrl={thread?.source_url ?? ""}
      {favoriteIndexes}
      onFavoritesChange={(next) => (favoriteIndexes = next)}
      compact={compactMode}
      {lastReadPostIndex}
      onOpenPostExternal={(p, i) => {
        if (!thread) return;
        const n = p.post_number ?? i + 1;
        const url = thread.source_url + (thread.source_url.includes("#") ? "" : `#post-${n}`);
        void api.openExternal(url);
      }}
    />
  </div>
{/if}

{#if debugCapture}
  <DebugComplaintDialog
    field={debugCapture.field}
    elementHtml={debugCapture.html}
    submitting={refineSubmitting}
    error={refineError}
    onSubmit={onDebugSubmit}
    onClose={cancelDebug}
  />
{/if}

{#if loading}
  <div class="thread-overlay-msg">Loading…</div>
{:else if error}
  <div class="thread-overlay-msg thread-overlay-error">{error}</div>
{:else if renderMode === "webview" && !bootedWebview}
  <div class="thread-overlay-msg">Opening thread…</div>
{:else if renderMode === "webview" && !posts}
  <div class="thread-overlay-msg thread-overlay-info">
    Learning the layout of this site… You'll be flipped to the native view automatically when it's ready.
  </div>
{/if}

<style>
  :global(html), :global(body), :global(.vb-page) {
    margin: 0;
    height: 100%;
    background: var(--vb-content-bg);
  }
  /* WHY: two-tier sticky header — feedBulletin brand on top (28px), thread nav strip
     below (40px). Total 68px. Native child webview is positioned at y=68 in Rust so it
     never overlaps. Both strips stay fixed while webview content scrolls beneath. */
  .fr-header {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1000;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
  }
  .fr-brand-row {
    height: 28px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: linear-gradient(
      to bottom,
      color-mix(in srgb, var(--vb-banner-grad-to) 100%, transparent),
      color-mix(in srgb, var(--vb-banner-grad-to) 60%, black)
    );
    color: var(--vb-banner-text);
    border-bottom: 1px solid rgba(255,255,255,0.05);
    font-size: 11px;
  }
  .fr-brand {
    color: var(--vb-banner-text);
    font-weight: bold;
    font-size: 12px;
    text-decoration: none;
    letter-spacing: 0.3px;
  }
  .fr-brand:hover { color: color-mix(in srgb, var(--vb-banner-text) 80%, transparent); }
  .fr-spacer { flex: 1 1 auto; }
  .thread-toolbar {
    height: 40px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: linear-gradient(
      to bottom,
      var(--vb-banner-grad-from),
      var(--vb-banner-grad-to)
    );
    color: var(--vb-banner-text);
    border-bottom: 1px solid color-mix(in srgb, var(--vb-banner-grad-to) 70%, black);
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
  }
  .tb-btn {
    background: var(--vb-cat-grad-from);
    color: var(--vb-cat-text);
    border: 1px solid var(--vb-banner-grad-to);
    padding: 4px 10px;
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
    line-height: 1.2;
  }
  .tb-btn:hover { background: color-mix(in srgb, var(--vb-cat-grad-from) 80%, white); }
  .tb-btn:disabled { opacity: 0.45; cursor: default; }
  .tb-crumb {
    margin-left: 12px;
    color: color-mix(in srgb, var(--vb-banner-text) 75%, transparent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex-shrink: 1;
  }
  .tb-crumb-mid {
    color: var(--vb-banner-text);
    font-weight: bold;
    text-decoration: none;
    cursor: pointer;
  }
  .tb-crumb-mid:hover { text-decoration: underline; }
  .tb-crumb-sep { margin: 0 6px; color: color-mix(in srgb, var(--vb-banner-text) 50%, transparent); }
  .tb-crumb-title { color: color-mix(in srgb, var(--vb-banner-text) 75%, transparent); }
  .tb-spacer { flex: 1 1 auto; }

  .thread-overlay-msg {
    position: fixed;
    top: 84px;
    left: 50%;
    transform: translateX(-50%);
    padding: 10px 16px;
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    color: var(--vb-meta);
    font-family: Verdana, Tahoma, sans-serif;
    font-size: 11px;
    z-index: 999;
    box-shadow: 0 2px 6px var(--vb-content-shadow);
  }
  .thread-overlay-error { color: var(--vb-pill-text); }
  .thread-overlay-info { color: var(--vb-meta); }

  .tb-toggle {
    background: var(--vb-banner-grad-from);
    border-color: var(--vb-banner-grad-to);
  }
  .tb-toggle-on {
    background: var(--vb-cat-grad-from);
    color: var(--vb-cat-text);
  }
  .tb-debug {
    background: #6b4f8a;
    border-color: #3d2a5a;
  }
  .tb-debug-on {
    background: #ff8c00;
    color: #fff;
    border-color: #b56500;
  }
  .tb-debug-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  /* Classic vB dropdown — square, navy bordered, white background. */
  .tb-debug-menu {
    position: absolute;
    top: calc(100% + 2px);
    right: 0;
    min-width: 260px;
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-border);
    box-shadow: 2px 4px 8px var(--vb-content-shadow);
    z-index: 2000;
    padding: 3px 0;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 11px;
  }
  .tb-debug-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 12px;
    background: transparent;
    color: var(--vb-link);
    border: 0;
    cursor: pointer;
    font: inherit;
    white-space: nowrap;
  }
  .tb-debug-item:hover:not(:disabled) {
    background: var(--vb-row-hover);
    color: var(--vb-link-hover);
  }
  .tb-debug-item:disabled {
    color: var(--vb-btn-disabled-text);
    cursor: default;
  }
  .tb-debug-sep {
    height: 1px;
    background: var(--vb-border);
    margin: 3px 0;
  }
  .tb-debug-danger { color: var(--vb-pill-text); }
  .tb-debug-danger:hover:not(:disabled) {
    background: var(--vb-pill-bg) !important;
    color: var(--vb-pill-text) !important;
  }
  /* Classic-vB status indicators — square 1px border, plain background,
     no animation, monospace-ish bracketed text. Sits inline with the
     toolbar buttons so it's always visible above the WebView. */
  .tb-stat {
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 8px;
    margin: 0 4px;
    font-family: "Courier New", Consolas, monospace;
    font-size: 11px;
    line-height: 1;
    border: 1px solid var(--vb-border);
    background: var(--vb-subbar-bg);
    color: var(--vb-link);
    box-sizing: border-box;
  }
  .tb-stat-learning {
    background: var(--vb-subbar-bg);
    color: var(--vb-link);
  }
  .tb-stat-failed {
    background: var(--vb-pill-bg);
    color: var(--vb-pill-text);
    border-color: var(--vb-pill-border);
    cursor: pointer;
    font-family: "Courier New", Consolas, monospace;
  }
  .tb-stat-failed:hover { background: color-mix(in srgb, var(--vb-pill-bg) 80%, black); }
  /* Subtle text-only cycle on the trailing dots — no glow, no scale, no pulse. */
  .tb-dots {
    display: inline-block;
    width: 1.2em;
    text-align: left;
    animation: tb-dots-cycle 1.2s steps(4, end) infinite;
  }
  @keyframes tb-dots-cycle {
    0%   { content: "."; }
    25%  { content: ".."; }
    50%  { content: "..."; }
    75%  { content: ""; }
    100% { content: "."; }
  }

  /* When we're in native mode, the child webview is closed and we need our own
     scroll container under the fixed 68px header. */
  .thread-native-scroll {
    position: fixed;
    top: 68px;
    left: 0;
    right: 0;
    bottom: 0;
    overflow-y: auto;
    background: var(--vb-content-bg);
  }
</style>
