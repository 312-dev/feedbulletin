<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { decodeHtmlEntities, fmtSmartDate, fmtVbPostTimestamp, type Post, type DebugField } from "$lib/types";

  // Per-(url|author) tint cache so the same avatar isn't re-sampled on every
  // render pass. Keyed by avatar URL when sampling worked; falls back to
  // author name when CORS / load errors force a hash-derived pastel.
  const avatarTintCache = new Map<string, string>();

  // Avatar URLs whose <img> failed to load (404, CORS block, network error,
  // whatever) get added here. Subsequent renders treat the URL as missing,
  // which routes that post through the silhouette + geopattern fallback
  // instead of leaving the browser's broken-image glyph on screen.
  // Reddit's AutoModerator is the canonical case: its `icon_img` field
  // sometimes resolves to a URL that 404s while every other Snoo loads fine.
  let failedAvatars = $state<Set<string>>(new Set());

  function markAvatarFailed(url: string) {
    if (failedAvatars.has(url)) return;
    // Reassign so Svelte 5's $state proxy sees the change (Set mutations
    // alone don't trigger reactivity).
    failedAvatars = new Set([...failedAvatars, url]);
  }

  // ─── Nested-quote depth limiter ────────────────────────────────────────
  // Forum threads frequently quote 4+ levels deep (post replies quote ⇒ quote
  // ⇒ quote ⇒ original). Stacking that many <blockquote>s with backgrounds
  // and left-borders eats vertical space and turns posts into a wall of
  // chrome. We collapse any chain deeper than MAX_QUOTE_DEPTH down to its
  // innermost MAX_QUOTE_DEPTH levels — keeping the most relevant context
  // (what's being directly replied to) and dropping the older ancestors.
  // An inline "[…older replies elided]" marker tells the reader context
  // was trimmed.
  const MAX_QUOTE_DEPTH = 2;

  function trimDeepQuotes(html: string, maxDepth = MAX_QUOTE_DEPTH): string {
    if (typeof document === "undefined") return html;
    if (!html || !html.includes("<blockquote")) return html;
    const tmpl = document.createElement("template");
    tmpl.innerHTML = html;

    // Max blockquote-depth reachable from below `el` (counts blockquote
    // children's own depth, recursively). 0 if no nested blockquote.
    function depthBelow(el: Element): number {
      let max = 0;
      for (const child of Array.from(el.children)) {
        const sub = depthBelow(child);
        max = Math.max(max, child.tagName === "BLOCKQUOTE" ? 1 + sub : sub);
      }
      return max;
    }

    // For each chain root (a blockquote with no blockquote ancestor in this
    // fragment), if its total chain depth exceeds maxDepth, replace it with
    // the inner subtree at depth (chainDepth - maxDepth) — i.e. step down
    // chainDepth - maxDepth levels and graft what we find.
    //
    // Step-down rule when a blockquote has multiple direct blockquote
    // children: take the deepest. That's the most-recent ancestor in a
    // typical reply chain (XF/VB serialize each new quote inside the prior
    // one, so deepest = most recent).
    function deepestBlockquoteChild(bq: Element): Element | null {
      let best: Element | null = null;
      let bestDepth = -1;
      for (const child of Array.from(bq.children)) {
        if (child.tagName !== "BLOCKQUOTE") continue;
        const d = 1 + depthBelow(child);
        if (d > bestDepth) {
          bestDepth = d;
          best = child;
        }
      }
      return best;
    }

    function trimRoot(bq: Element) {
      const chainDepth = 1 + depthBelow(bq);
      if (chainDepth <= maxDepth) return;
      let toReplace = bq;
      let levelsToSkip = chainDepth - maxDepth;
      let inner: Element | null = bq;
      while (levelsToSkip > 0 && inner) {
        inner = deepestBlockquoteChild(inner);
        levelsToSkip--;
      }
      if (!inner) return; // shouldn't happen if depth math is right
      const marker = document.createElement("div");
      marker.className = "vb-quote-elided";
      marker.textContent = "[…older replies elided]";
      toReplace.parentNode?.insertBefore(marker, toReplace);
      toReplace.parentNode?.replaceChild(inner, toReplace);
    }

    // Collect chain roots first (snapshot — trim mutates the tree).
    const roots: Element[] = [];
    for (const bq of Array.from(tmpl.content.querySelectorAll("blockquote"))) {
      let p: Element | null = bq.parentElement;
      let hasBqAncestor = false;
      while (p) {
        if (p.tagName === "BLOCKQUOTE") { hasBqAncestor = true; break; }
        p = p.parentElement;
      }
      if (!hasBqAncestor) roots.push(bq);
    }
    for (const root of roots) trimRoot(root);

    return tmpl.innerHTML;
  }

  /** djb2 hash → unsigned 32-bit int. Shared by pastel + pattern. */
  function hashName(name: string): number {
    let h = 5381;
    for (let i = 0; i < name.length; i++) {
      h = ((h << 5) + h + name.charCodeAt(i)) & 0xffffffff;
    }
    return Math.abs(h);
  }

  /** Stable djb2 hash-derived HSL color. Saturated enough that the
   * user-to-user variation is obvious; light enough to read as a bg. */
  function pastelFromName(name: string): string {
    return `hsl(${hashName(name) % 360}, 55%, 76%)`;
  }

  /** Stable per-name `background-image` value with one of 7 shape-based
   * inline-SVG patterns. All NON-LINEAR — no stripes or hatching that read
   * as harsh in a small bg. Hand-rolled so we control exactly what renders. */
  function patternFromName(name: string): string {
    const h = hashName(name || "anon");
    const kind = h % 7;
    const tile = 18 + ((h >> 3) % 12); // 18..29 px
    const hue = h % 360;
    const accent = `hsl(${hue}, 55%, 48%)`;
    const t = tile;
    const c = t / 2;

    let svg = "";
    switch (kind) {
      case 0: // polka dots
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><circle cx='${c}' cy='${c}' r='${(t * 0.2).toFixed(1)}' fill='${accent}' opacity='0.35'/></svg>`;
        break;
      case 1: // diamonds (rotated squares)
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><path d='M${c},2 L${t - 2},${c} L${c},${t - 2} L2,${c} Z' fill='${accent}' opacity='0.28'/></svg>`;
        break;
      case 2: // hexagons (offset two per tile)
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t * 2}'><path d='M${c},3 L${t - 3},${t * 0.27} L${t - 3},${t * 0.73} L${c},${t - 3} L3,${t * 0.73} L3,${t * 0.27} Z' fill='${accent}' opacity='0.28'/></svg>`;
        break;
      case 3: // scallops / arcs
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><path d='M0,${c} A${c},${c} 0 0 1 ${t},${c}' fill='none' stroke='${accent}' stroke-width='2' opacity='0.32'/></svg>`;
        break;
      case 4: // plus signs
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><path d='M${c - 3},${c} h6 M${c},${c - 3} v6' stroke='${accent}' stroke-width='2' opacity='0.35' stroke-linecap='round'/></svg>`;
        break;
      case 5: // concentric rings
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><circle cx='${c}' cy='${c}' r='${(t * 0.35).toFixed(1)}' stroke='${accent}' stroke-width='1.5' fill='none' opacity='0.28'/><circle cx='${c}' cy='${c}' r='${(t * 0.15).toFixed(1)}' fill='${accent}' opacity='0.35'/></svg>`;
        break;
      default: // triangles
        svg = `<svg xmlns='http://www.w3.org/2000/svg' width='${t}' height='${t}'><path d='M0,${t} L${c},0 L${t},${t} Z' fill='${accent}' opacity='0.28'/></svg>`;
        break;
    }
    return `url("data:image/svg+xml;utf8,${encodeURIComponent(svg)}")`;
  }

  function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    const l = (max + min) / 2;
    let h = 0, s = 0;
    if (max !== min) {
      const d = max - min;
      s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
      h /= 6;
    }
    return [h, s, l];
  }

  /** Sample a 16×16 downscale of the image, histogram-bin colors at 3 bits
   * per channel, ignore near-grayscale pixels (which wash out into mud), pick
   * the dominant bin, and emit it as a muted pastel HSL string so the
   * background reads as ambient framing rather than vibrant accent. */
  function sampleDominantPastel(img: HTMLImageElement): string {
    const canvas = document.createElement("canvas");
    const SIZE = 16;
    canvas.width = SIZE;
    canvas.height = SIZE;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no 2d ctx");
    ctx.drawImage(img, 0, 0, SIZE, SIZE);
    const data = ctx.getImageData(0, 0, SIZE, SIZE).data; // throws on CORS

    const buckets = new Map<number, { count: number; r: number; g: number; b: number }>();
    for (let i = 0; i < data.length; i += 4) {
      const a = data[i + 3];
      if (a < 40) continue;
      const r = data[i], g = data[i + 1], b = data[i + 2];
      const max = Math.max(r, g, b), min = Math.min(r, g, b);
      if (max - min < 20) continue; // skip near-gray
      const key = ((r >> 5) << 6) | ((g >> 5) << 3) | (b >> 5);
      const e = buckets.get(key);
      if (e) { e.count++; e.r += r; e.g += g; e.b += b; }
      else buckets.set(key, { count: 1, r, g, b });
    }
    if (buckets.size === 0) throw new Error("no chromatic pixels");
    let best = { count: 0, r: 0, g: 0, b: 0 };
    for (const e of buckets.values()) {
      if (e.count > best.count) best = e;
    }
    const r = best.r / best.count;
    const g = best.g / best.count;
    const b = best.b / best.count;
    const [hue] = rgbToHsl(r, g, b);
    return `hsl(${Math.round(hue * 360)}, 55%, 76%)`;
  }

  function applyAvatarTint(img: HTMLImageElement, author: string) {
    const key = img.currentSrc || img.src || author;
    let color = avatarTintCache.get(key);
    if (!color) {
      try {
        color = sampleDominantPastel(img);
      } catch {
        color = pastelFromName(author || key);
      }
      avatarTintCache.set(key, color);
    }
    // Pattern + tint live on the avatar element itself.
    img.style.setProperty("--avatar-tint", color);
  }

  type DebugClick = { field: DebugField; html: string };

  let {
    posts,
    title = "",
    debug = false,
    onDebugClick = undefined,
    onNeedMore = undefined,
    loadingMore = false,
    moreAvailable = true,
    threadId = undefined,
    threadSourceUrl = "",
    favoriteIndexes = undefined,
    onFavoritesChange = undefined,
    compact = false,
    onOpenPostExternal = undefined,
    lastReadPostIndex = null,
  }: {
    posts: Post[];
    /** Thread title rendered as a classic-vB title bar above the OP. */
    title?: string;
    debug?: boolean;
    onDebugClick?: (e: DebugClick) => void;
    /** Fired when the bottom-sentinel scrolls into view. */
    onNeedMore?: () => void;
    /** True while a lazy-load fetch is in flight. Footer only renders when true. */
    loadingMore?: boolean;
    /** False when the host has confirmed there's nothing more to load
     * (no load_more_strategy on this profile, or the last attempt brought
     * back zero new posts). Suppresses both the sentinel and the footer. */
    moreAvailable?: boolean;
    /** Thread id used to persist favorites. Star is disabled when undefined. */
    threadId?: number;
    /** Source URL snapshotted into each favorite row. */
    threadSourceUrl?: string;
    /** Set of post_index values (post_number ?? i+1) that are currently favorited. */
    favoriteIndexes?: Set<number>;
    /** Called after a successful favorite/unfavorite so the parent can refresh. */
    onFavoritesChange?: (next: Set<number>) => void;
    /** Compact mode hides avatars + signatures for denser long-thread reading. */
    compact?: boolean;
    /** Open a specific post on the original source site (best-effort `#post-N`). */
    onOpenPostExternal?: (p: Post, i: number) => void;
    /** Last post_index the user has read in this thread. Draws a "New posts ↓"
     * divider above the first unseen reply when set. */
    lastReadPostIndex?: number | null;
  } = $props();

  function postIndexOf(p: Post, i: number): number {
    return p.post_number ?? i + 1;
  }

  /** Strip HTML tags + collapse whitespace, then truncate to ~200 chars for
   * the favorites-list excerpt. */
  function excerptFromHtml(html: string): string {
    const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
    if (text.length <= 200) return text;
    return text.slice(0, 200).trimEnd() + "…";
  }

  async function toggleFavorite(p: Post, i: number) {
    if (threadId === undefined || !favoriteIndexes) return;
    const idx = postIndexOf(p, i);
    const next = new Set(favoriteIndexes);
    const wasOn = favoriteIndexes.has(idx);
    if (wasOn) {
      next.delete(idx);
      onFavoritesChange?.(next);
      try {
        await api.unfavoritePost(threadId, idx);
      } catch {
        const revert = new Set(next);
        revert.add(idx);
        onFavoritesChange?.(revert);
      }
    } else {
      next.add(idx);
      onFavoritesChange?.(next);
      try {
        await api.favoritePost(threadId, idx, {
          hasPostNumber: p.post_number !== undefined && p.post_number !== null,
          author: p.author ?? null,
          timestampRaw: p.timestamp ?? null,
          bodyHtml: p.body_html ?? "",
          bodyExcerpt: excerptFromHtml(p.body_html ?? ""),
          threadTitle: title || "Untitled thread",
          threadSourceUrl: threadSourceUrl,
        });
      } catch {
        const revert = new Set(next);
        revert.delete(idx);
        onFavoritesChange?.(revert);
      }
    }
  }

  let sentinel = $state<HTMLElement | null>(null);
  let lastTrigger = 0;

  // After each posts update, promote lazy-load attributes on rendered <img>
  // elements so the browser actually fetches them. We can't do this in Rust
  // because ammonia's attribute_filter lets us mutate values but not add new
  // attributes, so we do the src promotion DOM-side once the HTML is in the
  // page. Cheap: only walks images whose `src` is empty.
  $effect(() => {
    void posts; // re-run when posts change
    if (typeof document === "undefined") return;
    requestAnimationFrame(() => {
      const imgs = document.querySelectorAll<HTMLImageElement>(
        ".vb-post-content img"
      );
      for (const img of imgs) {
        if (img.src && img.src !== window.location.href) continue;
        const alt = img.getAttribute("data-src")
          || img.getAttribute("data-lazy-src")
          || img.getAttribute("data-original")
          || img.getAttribute("data-href");
        if (alt) img.src = alt;
      }
    });
  });

  onMount(() => {
    if (!onNeedMore || !sentinel) return;
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        if (!moreAvailable) continue; // host says nothing more to fetch
        const now = Date.now();
        // Debounce: don't re-fire within 4s of the last load — gives the
        // backend time to bring new content in and re-render.
        if (now - lastTrigger < 4000) continue;
        lastTrigger = now;
        onNeedMore?.();
      }
    }, { root: null, rootMargin: "400px", threshold: 0 });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  /** Walk up from target finding the nearest ancestor with data-field;
   * fall back to "post" if none found within the thread container. */
  function handleClick(ev: MouseEvent) {
    // Anchor handling FIRST — even when not in debug mode, native-view links
    // would otherwise navigate the SPA away from /thread/[id] and there's no
    // way back. Internal "#post-N" anchors keep working (smooth-scroll);
    // external http(s) links route through api.openExternal so the user's
    // default browser handles them.
    if (!debug) {
      const anchor = (ev.target as HTMLElement | null)?.closest?.("a") as HTMLAnchorElement | null;
      if (anchor) {
        const raw = anchor.getAttribute("href") || "";
        if (raw.startsWith("#")) {
          // In-document anchor — let the host route resolve it manually so we
          // scroll inside .thread-native-scroll, not the document.
          ev.preventDefault();
          const id = raw.slice(1);
          const scope = (ev.currentTarget as HTMLElement | null) ?? document;
          const target = (scope as ParentNode).querySelector?.(`[id="${CSS.escape(id)}"]`);
          target?.scrollIntoView?.({ behavior: "smooth", block: "start" });
          return;
        }
        if (/^https?:\/\//i.test(raw) || /^mailto:/i.test(raw)) {
          ev.preventDefault();
          void api.openExternal(raw).catch(() => window.open(raw, "_blank"));
          return;
        }
        // javascript: / empty / unknown → block (don't let it navigate the SPA)
        ev.preventDefault();
        return;
      }
      return;
    }

    // Debug mode: capture the clicked element for refinement.
    ev.preventDefault();
    ev.stopPropagation();
    let el = ev.target as HTMLElement | null;
    let field: DebugField = "post";
    while (el && !el.classList.contains("vb-thread")) {
      const f = el.getAttribute("data-field");
      if (f) { field = f as DebugField; break; }
      el = el.parentElement;
    }
    // Use the element that actually had data-field, or the click target if none.
    const target = (el && el.getAttribute("data-field")) ? el : (ev.target as HTMLElement);
    let html = target.outerHTML || "";
    if (html.length > 6000) html = html.slice(0, 6000) + "…";
    onDebugClick?.({ field, html });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="vb-thread"
  class:vb-thread-debug={debug}
  class:vb-thread-compact={compact}
  onclick={handleClick}
  onauxclick={handleClick}
  onkeydown={(e) => { if (debug && (e.key === "Enter" || e.key === " ")) handleClick(e as unknown as MouseEvent); }}
  role={debug ? "button" : undefined}
  tabindex={debug ? 0 : undefined}
>
  {#each posts as p, i (i)}
    {@const idx = postIndexOf(p, i)}
    {#if lastReadPostIndex != null && lastReadPostIndex > 0 && i > 0 && idx > lastReadPostIndex && postIndexOf(posts[i - 1], i - 1) <= lastReadPostIndex}
      <div class="vb-new-divider" data-testid="new-posts-divider">
        <span>New posts ↓</span>
      </div>
    {/if}
    <article class="vb-post" data-field="post" id={`post-${p.post_number ?? i + 1}`}>
      <aside class="vb-author-panel">
        {#if p.avatar_url && !failedAvatars.has(p.avatar_url)}
          <img
            class="vb-avatar"
            data-field="avatar"
            src={p.avatar_url}
            alt=""
            loading="lazy"
            crossorigin="anonymous"
            style={`--avatar-tint: ${pastelFromName(p.author || "unknown")}; --avatar-pattern: ${patternFromName(p.author || "unknown")}`}
            onload={(e) => applyAvatarTint(e.currentTarget as HTMLImageElement, p.author)}
            onerror={() => markAvatarFailed(p.avatar_url!)}
          />
        {:else}
          <!-- Silhouette placeholder. Bg rect is transparent so the avatar
               element's pattern shows through. -->
          <div
            class="vb-avatar vb-avatar-fallback"
            data-field="avatar"
            aria-hidden="true"
            style={`--avatar-tint: ${pastelFromName(p.author || "unknown")}; --avatar-pattern: ${patternFromName(p.author || "unknown")}`}
          >
            <svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg" width="100%" height="100%">
              <defs>
                <filter id="soft" x="-10%" y="-10%" width="120%" height="120%">
                  <feGaussianBlur stdDeviation="0.8" />
                </filter>
              </defs>
              <g fill="#5e6b80" filter="url(#soft)">
                <circle cx="50" cy="36" r="16" />
                <path d="M20,92 C20,68 34,58 50,58 C66,58 80,68 80,92 Z" />
              </g>
            </svg>
          </div>
        {/if}
        <div class="vb-author-name" data-field="author">
          {#if p.author_url}
            <a href={p.author_url} data-field="author_url" target="_blank" rel="noopener noreferrer">{p.author}</a>
          {:else}
            {p.author || "—"}
          {/if}
        </div>
        {#if p.author_rank}
          <div class="vb-author-rank" data-field="author_rank">{p.author_rank}</div>
        {/if}
        <dl class="vb-author-meta">
          {#if p.author_post_count}
            <dt>Posts:</dt><dd data-field="author_post_count">{p.author_post_count}</dd>
          {/if}
          {#if p.author_karma}
            <dt>Karma:</dt><dd data-field="author_karma">{p.author_karma}</dd>
          {/if}
          {#if p.author_join_date}
            <dt>Joined:</dt><dd data-field="author_join_date">{fmtSmartDate(p.author_join_date)}</dd>
          {/if}
          {#if p.author_last_active}
            <dt>Last on:</dt><dd data-field="author_last_active">{fmtSmartDate(p.author_last_active)}</dd>
          {/if}
          {#if p.author_location}
            <dt>Location:</dt><dd data-field="author_location">{p.author_location}</dd>
          {/if}
        </dl>
      </aside>

      <section class="vb-post-body">
        <header class="vb-post-head">
          {#if p.timestamp}
            <span class="vb-post-ts" data-field="timestamp">{fmtVbPostTimestamp(p.timestamp)}</span>
          {/if}
          <span class="vb-post-head-right">
            <span class="vb-post-num" data-field="post_number">#{p.post_number ?? i + 1}</span>
            {#if onOpenPostExternal}
              <button
                type="button"
                class="vb-post-ext"
                title="Open this post in the source site"
                onclick={(e) => { e.stopPropagation(); onOpenPostExternal?.(p, i); }}
              >
                <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
                  <path d="M14 3h7v7M21 3l-9 9M5 7h6M5 12v7h7" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
              </button>
            {/if}
            {#if threadId !== undefined}
              {@const isFav = favoriteIndexes?.has(postIndexOf(p, i)) ?? false}
              <button
                type="button"
                class="vb-post-fav"
                class:active={isFav}
                aria-pressed={isFav}
                title={isFav ? "Remove from favorites" : "Add to favorites (keeps this thread cached)"}
                onclick={(e) => { e.stopPropagation(); void toggleFavorite(p, i); }}
              >
                {#if isFav}
                  <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                    <path d="M12 2.5l2.92 6.34 6.95.66-5.2 4.7 1.5 6.8L12 17.5 5.83 21l1.5-6.8-5.2-4.7 6.95-.66z" fill="currentColor"/>
                  </svg>
                {:else}
                  <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                    <path d="M12 2.5l2.92 6.34 6.95.66-5.2 4.7 1.5 6.8L12 17.5 5.83 21l1.5-6.8-5.2-4.7 6.95-.66z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
                  </svg>
                {/if}
              </button>
            {/if}
          </span>
        </header>
        {#if i === 0 && title && title.trim().length > 0}
          <h1 class="vb-post-title">{decodeHtmlEntities(title)}</h1>
        {/if}
        {#if p.quote_of_post_number}
          <div class="vb-post-replyto">
            <a href={`#post-${p.quote_of_post_number}`}>↪ in reply to #{p.quote_of_post_number}</a>
          </div>
        {/if}
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        <div class="vb-post-content" data-field="body">{@html trimDeepQuotes(p.body_html)}</div>
        {#if p.signature_html}
          <div class="vb-post-sig" data-field="signature">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html p.signature_html}
          </div>
        {/if}
        {#if p.like_count !== undefined && p.like_count !== null}
          <div class="vb-post-likes" data-field="like_count">
            {#if p.like_count > 0}
              <svg class="vb-likes-thumb" viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
                <!-- Classic chunky thumbs-up silhouette, vB-blue. -->
                <path d="M2 10h4v11H2zM21.7 10.4c-.5-.5-1.1-.7-1.7-.7h-5.4l.8-3.7c.1-.5 0-1-.3-1.4-.3-.4-.7-.6-1.2-.6-.6 0-1.1.3-1.4.8L8 9.7V21h11.2c.8 0 1.5-.5 1.7-1.3l1.5-7.6c.2-.6 0-1.2-.7-1.7z" fill="currentColor"/>
              </svg>
              <strong>{p.like_count.toLocaleString()}</strong>
              {p.like_count === 1 ? "like" : "likes"}
            {:else}
              <span class="vb-likes-zero">0 likes</span>
            {/if}
          </div>
        {/if}
      </section>
    </article>
  {/each}

  {#if onNeedMore && moreAvailable}
    <div class="vb-load-more-sentinel" bind:this={sentinel} aria-hidden="true"></div>
    {#if loadingMore}
      <div class="vb-load-more-status">[ Loading more posts<span class="vb-load-dots">...</span> ]</div>
    {/if}
  {/if}
</div>

<style>
  /* Thread title rendered INSIDE the OP's post body, above the prose.
     Subtle bold heading with a dashed underline — classic vBulletin OP
     subject styling, not a banner. */
  .vb-post-title {
    margin: 4px 0 10px 0;
    padding: 0 0 8px 0;
    border-bottom: 1px dashed var(--vb-post-title-underline);
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 13px;
    font-weight: bold;
    color: var(--vb-page-text);
    line-height: 1.3;
  }

  /* Classic vB v3/v4 post layout — 160px author panel left, content right. */
  .vb-thread {
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
    color: var(--vb-page-text);
    background: var(--vb-content-bg);
    padding: 0 0 24px 0;
  }
  .vb-post {
    display: grid;
    grid-template-columns: 160px 1fr;
    border: 1px solid var(--vb-border);
    margin: 8px 12px 0 12px;
    background: var(--vb-content-bg);
  }
  .vb-author-panel {
    background: linear-gradient(to right, var(--vb-subbar-bg), var(--vb-row-hover));
    border-right: 1px solid var(--vb-border);
    padding: 10px 8px;
    text-align: center;
  }
  /* Avatar element carries its own per-user inline-SVG pattern + tint.
     Pattern is visible behind transparent avatars (silhouette + alpha PNGs)
     and behind any white-space in the 100x100 box. background-size is auto
     so the SVG tile repeats naturally — `cover` would scale a single tile
     to fill 100x100 and lose the motif. */
  .vb-avatar {
    display: block;
    width: 100px;
    height: 100px;
    margin: 0 auto 6px auto;
    border: 1px solid var(--vb-border);
    background-color: var(--avatar-tint, var(--vb-content-bg));
    background-image: var(--avatar-pattern, none);
    background-size: auto;
    background-repeat: repeat;
    transition: background-color 0.4s ease;
  }
  /* Subtle 2006-feel pass for real avatar <img>s — light JPEG-ish softness
     and warm tint without going lossy. The silhouette fallback has its
     own baked look and isn't touched. */
  .vb-avatar:not(.vb-avatar-fallback) {
    filter: contrast(0.96) saturate(0.88) brightness(0.98) sepia(0.04);
    image-rendering: -webkit-optimize-contrast;
  }
  /* Default 2006-style silhouette placeholder when no avatar_url is known.
     Per-user hash-derived tint via --avatar-tint so users without custom
     avatars don't all look identical. */
  /* Silhouette placeholder. SVG's bg rect renders normally (radial gradient)
     so the silhouette is visible against the patterned panel. */
  .vb-avatar-fallback {
    width: 100px;
    height: 100px;
    background: transparent;
    filter: saturate(0.7) brightness(0.98);
  }
  .vb-avatar-fallback svg {
    display: block;
  }
  .vb-author-name {
    font-weight: bold;
    font-size: 13px;
    word-wrap: break-word;
  }
  .vb-author-name a {
    color: var(--vb-link);
    text-decoration: none;
  }
  .vb-author-name a:hover { text-decoration: underline; }
  .vb-author-rank {
    color: var(--vb-muted);
    font-size: 10px;
    margin: 2px 0 6px 0;
  }
  .vb-author-meta {
    display: grid;
    grid-template-columns: auto auto;
    gap: 2px 6px;
    font-size: 10px;
    color: var(--vb-meta);
    text-align: left;
    margin: 6px 0 0 0;
  }
  .vb-author-meta dt {
    font-weight: bold;
  }
  .vb-author-meta dd { margin: 0; }

  .vb-post-body {
    padding: 10px 14px 0 14px;
    min-width: 0; /* allow content to shrink for wrapping */
    /* WHY: flex column so .vb-post-likes can margin-top:auto pin to the
       bottom of the full row height (grid items stretch to row height,
       so a short body still reaches the author-panel's bottom). */
    display: flex;
    flex-direction: column;
  }
  .vb-post-head {
    display: flex;
    align-items: baseline;
    padding-bottom: 6px;
    margin-bottom: 8px;
    border-bottom: 1px dashed var(--vb-post-title-underline);
    color: var(--vb-meta);
    font-size: 10px;
  }
  .vb-post-ts { color: var(--vb-very-muted); }
  .vb-post-num { color: var(--vb-very-muted); }
  .vb-post-head-right {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .vb-post-fav {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--vb-very-muted);
    cursor: pointer;
    border-radius: 3px;
    line-height: 0;
    transition: color 80ms ease, background-color 80ms ease;
  }
  .vb-post-fav:hover {
    color: #D4A017;
    background: rgba(212, 160, 23, 0.10);
  }
  .vb-post-fav.active {
    color: #D4A017;
  }
  .vb-post-fav:focus-visible {
    outline: 1px solid var(--vb-link);
    outline-offset: 1px;
  }
  .vb-post-ext {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--vb-very-muted);
    cursor: pointer;
    border-radius: 3px;
    line-height: 0;
    transition: color 80ms ease, background-color 80ms ease;
  }
  .vb-post-ext:hover {
    color: var(--vb-link);
    background: color-mix(in srgb, var(--vb-link) 12%, transparent);
  }
  .vb-post-ext:focus-visible {
    outline: 1px solid var(--vb-link);
    outline-offset: 1px;
  }
  .vb-new-divider {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 6px 0;
    background: color-mix(in srgb, var(--vb-link) 8%, transparent);
    border-top: 1px dashed var(--vb-link);
    border-bottom: 1px dashed var(--vb-link);
    font-family: Verdana, Tahoma, sans-serif;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 1px;
    color: var(--vb-link);
  }
  .vb-new-divider::before,
  .vb-new-divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: color-mix(in srgb, var(--vb-link) 40%, transparent);
  }
  .vb-post-replyto {
    font-size: 10px;
    color: var(--vb-very-muted);
    margin-bottom: 4px;
  }
  .vb-post-replyto a { color: var(--vb-link); text-decoration: none; }
  .vb-post-replyto a:hover { text-decoration: underline; }

  .vb-post-content {
    font-size: 13px;
    line-height: 1.5;
    color: var(--vb-page-text);
    word-wrap: break-word;
  }
  .vb-post-content :global(p) { margin: 0 0 0.8em 0; }
  .vb-post-content :global(a) { color: var(--vb-link); }
  .vb-post-content :global(a:hover) { text-decoration: underline; }
  .vb-post-content :global(img) {
    max-width: 100%;
    height: auto;
    display: inline-block;
    vertical-align: middle;
    border: 0;
  }
  /* Classic vBulletin-style image embed framing for any image inside the
     extra-media block (Reddit galleries, image-post previews, etc).
     Block layout, thin gray border, 8px gap stack, max width within reading
     measure so portraits don't dominate the post. */
  .vb-post-content :global(.vb-extra-media) {
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px dashed var(--vb-post-title-underline);
  }
  .vb-post-content :global(.vb-extra-media img) {
    display: block;
    margin: 8px 0;
    max-width: 560px;
    width: auto;
    height: auto;
    border: 1px solid var(--vb-border);
    padding: 4px;
    background: var(--vb-content-bg);
    box-shadow: 0 1px 2px var(--vb-content-shadow);
  }
  /* If the agent's extra-media selector accidentally pulls in <ul>/<ol>
     gallery navigation lists (Previous / Next / Back to Grid View on Reddit),
     hide them — they're useless outside the source page's JS. */
  .vb-post-content :global(.vb-extra-media ul),
  .vb-post-content :global(.vb-extra-media ol) {
    display: none;
  }
  /* Embedded videos (mp4, webm, reddit video). */
  .vb-post-content :global(video) {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 8px 0;
    background: #000;
    border: 1px solid var(--vb-border);
  }
  /* YouTube / Vimeo / Twitch / etc embeds — keep a stable aspect ratio.
     Most forums use 16:9 video embeds. */
  .vb-post-content :global(iframe) {
    display: block;
    margin: 8px 0;
    width: 100%;
    max-width: 640px;
    aspect-ratio: 16 / 9;
    border: 1px solid var(--vb-border);
    background: var(--vb-blockquote-bg);
  }
  .vb-post-content :global(figure) {
    margin: 8px 0;
  }
  .vb-post-content :global(figure img),
  .vb-post-content :global(figure video) {
    width: 100%;
    max-width: 640px;
  }
  .vb-post-content :global(pre),
  .vb-post-content :global(code) {
    font-family: Consolas, "Liberation Mono", Menlo, monospace;
    background: var(--vb-blockquote-bg);
    border: 1px solid var(--vb-border);
    padding: 1px 4px;
    border-radius: 2px;
  }
  .vb-post-content :global(pre) {
    padding: 8px;
    overflow-x: auto;
  }

  /* Likes footer pinned to the bottom of the post row.
     `margin-top: auto` inside the flex-column body pushes this to the
     bottom of the row regardless of body length. Muted gray bar — should
     read as ambient metadata, not a primary UI element. */
  .vb-post-likes {
    margin: 14px -14px 0 -14px;
    padding: 3px 14px;
    background: var(--vb-row-bg-b);
    border-top: 1px solid var(--vb-border);
    text-align: left;
    font-size: 10px;
    color: var(--vb-muted);
    line-height: 1.4;
    margin-top: auto;
  }
  .vb-post-likes strong {
    color: var(--vb-meta);
    font-weight: 600;
  }
  .vb-likes-thumb {
    color: var(--vb-muted);
    vertical-align: -2px;
    margin-right: 4px;
  }
  .vb-likes-zero {
    color: var(--vb-very-muted);
    font-style: italic;
  }

  /* Classic vB signature — separator above, smaller muted text below. */
  .vb-post-sig {
    margin: 14px 0 0 0;
    padding-top: 8px;
    border-top: 1px dashed var(--vb-post-title-underline);
    font-size: 11px;
    color: var(--vb-muted);
    line-height: 1.4;
    font-style: italic;
  }
  .vb-post-sig :global(a) { color: var(--vb-link); }
  .vb-post-sig :global(img) {
    max-width: 100%;
    height: auto;
    vertical-align: middle;
  }

  /* Inline marker placed by trimDeepQuotes() in place of the chain root when
     the original chain was deeper than MAX_QUOTE_DEPTH. Reads as a quiet
     italic note so it doesn't compete with the quotes it's introducing. */
  .vb-post-content :global(.vb-quote-elided) {
    font-size: 10px;
    font-style: italic;
    color: var(--vb-very-muted);
    margin: 6px 0 -4px 0;
    padding-left: 6px;
    border-left: 2px dotted var(--vb-blockquote-border);
  }

  /* Classic vB quote box — applies to BOTH source-embedded quotes (XenForo /
     vBulletin / phpBB) AND our synthesized vb-reply (Reddit depth-1 child). */
  .vb-post-content :global(blockquote) {
    margin: 8px 0;
    padding: 8px 10px;
    background: var(--vb-blockquote-bg);
    border: 1px solid var(--vb-blockquote-border);
    border-left: 3px solid var(--vb-cat-grad-from);
    font-size: 12px;
    color: var(--vb-page-text);
  }
  .vb-post-content :global(blockquote.vb-reply) {
    /* Slightly warmer tinted variant for synthesized Reddit replies. */
    background: color-mix(in srgb, var(--vb-blockquote-bg) 80%, #d8c890);
    border-color: color-mix(in srgb, var(--vb-blockquote-border) 60%, #a6924f);
    border-left-color: #a6924f;
  }
  /* Reply quote-box at the top of a reply post — references the parent.
     Slightly darker top stripe with bold "Originally Posted by..." cite. */
  .vb-post-content :global(blockquote.vb-quote) {
    background: var(--vb-blockquote-bg);
    border: 1px solid var(--vb-blockquote-border);
    border-top: 2px solid var(--vb-cat-grad-from);
    border-left: 1px solid var(--vb-blockquote-border);
    margin: 0 0 12px 0;
  }
  .vb-post-content :global(blockquote.vb-quote cite) {
    display: block;
    background: var(--vb-subbar-bg);
    margin: -8px -10px 6px -10px;
    padding: 3px 10px;
    font-size: 10px;
    font-weight: bold;
    color: var(--vb-subbar-text);
    font-style: normal;
    border-bottom: 1px solid var(--vb-blockquote-border);
  }
  .vb-post-content :global(blockquote.vb-quote cite a) {
    color: var(--vb-link);
    text-decoration: none;
  }
  .vb-post-content :global(blockquote.vb-quote cite a:hover) {
    text-decoration: underline;
  }
  .vb-post-content :global(blockquote.vb-quote p) {
    margin: 0;
    font-size: 12px;
    color: var(--vb-page-text);
  }
  .vb-post-content :global(blockquote cite) {
    display: block;
    font-style: normal;
    font-weight: bold;
    color: var(--vb-muted);
    margin-bottom: 4px;
    font-size: 11px;
  }

  @media (max-width: 720px) {
    .vb-post { grid-template-columns: 90px 1fr; }
    .vb-avatar { max-width: 72px; max-height: 72px; }
    .vb-author-meta { font-size: 9px; }
  }

  /* Compact mode: narrow left rail (no avatar, just author name), no sigs,
     tighter paddings. Useful for long threads where avatars + sigs become
     visual noise. Toggle is persisted on the thread page. */
  .vb-thread-compact :global(.vb-post) {
    grid-template-columns: 110px 1fr;
  }
  .vb-thread-compact :global(.vb-avatar),
  .vb-thread-compact :global(.vb-author-extras),
  .vb-thread-compact :global(.vb-post-sig) {
    display: none;
  }
  .vb-thread-compact :global(.vb-post-left) {
    padding: 8px 8px 4px 8px;
  }
  .vb-thread-compact :global(.vb-post-right) {
    padding: 8px 12px;
  }

  /* Bottom-of-thread sentinel + classic vB "loading more…" footer.
     The sentinel is invisible; the status text is always shown but only
     visible if the user is at the bottom while a backend fetch is in flight. */
  .vb-load-more-sentinel {
    width: 100%;
    height: 1px;
  }
  .vb-load-more-status {
    padding: 10px 14px;
    margin: 12px 0;
    text-align: center;
    font-family: "Courier New", Consolas, monospace;
    font-size: 11px;
    color: var(--vb-subbar-text);
    opacity: 0.7;
  }
  .vb-load-dots {
    display: inline-block;
    width: 1.2em;
    text-align: left;
  }

  /* Debugger mode — every element with data-field gets a hover outline,
     and clicks are captured for refinement. */
  .vb-thread-debug { cursor: crosshair; }
  .vb-thread-debug :global([data-field]:hover) {
    outline: 2px solid #ff8c00 !important;
    outline-offset: 1px;
    background: rgba(255, 200, 100, 0.10) !important;
    cursor: crosshair;
  }
  .vb-thread-debug :global(a) { pointer-events: none; }
</style>
