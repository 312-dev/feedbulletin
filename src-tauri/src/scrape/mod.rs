// Native-render scrape pipeline. Receives DOM samples from the embedded
// child webview (via window.ipc.postMessage), applies cached site profiles
// to extract structured posts, and emits `posts_ready` to the frontend.
//
// Phase 1 (this file): wiring + linear/threaded extraction with builtin
// profiles. Anthropic-driven profile learning lives in `crate::anthropic`
// and is invoked from `handle_scrape_sample` when no profile exists for a
// host.

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod extract;
pub mod seed;
pub mod skeleton;

fn default_content_type() -> String {
    "default".to_string()
}

/// One forum post rendered in our vBulletin-style native view. All metadata
/// fields are optional because no two platforms expose the same set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Post {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_number: Option<u32>,

    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,

    // Author panel metadata — populated where the platform exposes it on the
    // post page. Stored as opaque strings so we never lose locale-specific
    // formatting ("4,212 posts" / "Joined Jun 2009" / "München, DE").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_rank: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_post_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_join_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_location: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    /// Sanitized HTML — ammonia keeps blockquote, cite, code, img, a, etc.
    /// On Reddit (threaded_replies mode) this also contains the synthesized
    /// `<blockquote class="vb-reply">` wrapping the top child comment.
    pub body_html: String,

    /// Optional post signature — the small block beneath the body that's
    /// rendered separately (different font, often a divider above) in
    /// classic vB threads. Sanitized HTML, may contain links and images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_html: Option<String>,

    /// Number of likes / upvotes / reactions for this post. `None` means the
    /// site has no detected like-system (extractor renders no footer).
    /// `Some(n)` always renders a footer, even when n == 0. Negative votes
    /// or hidden scores ("•") are normalized to Some(0) so the footer is
    /// consistent across the thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub like_count: Option<i64>,

    /// Karma / reputation / likes-received total. Filled from the author's
    /// profile page by the enrichment task; not extracted from the thread
    /// itself (most platforms don't show this per-comment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_karma: Option<String>,

    /// Last-seen / last-active marker from the author's profile. Same
    /// enrichment path as karma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_last_active: Option<String>,

    /// vBulletin/XenForo quote-blocks frequently reference an earlier post
    /// number; we surface it as a back-link anchor in the native view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_of_post_number: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraversalMode {
    /// XenForo, vBulletin, Discourse, phpBB — flat list of posts in DOM order.
    #[default]
    Linear,
    /// Reddit — tree of comments. We walk depth 1 only: OP via `op_selector`,
    /// each top-level comment via `post_selector`, the first immediate child
    /// reply (via `child_comment_selector`) is appended to the parent as an
    /// inline blockquote.
    ThreadedReplies,
}

/// CSS-selector set for one (host, content_type). Persisted as JSON in
/// `site_profiles.selectors_json`. Every selector is optional except
/// `post_selector` + `author_selector` + `body_selector` (validated on insert).
/// Unknown future fields are ignored via `#[serde(default)]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectorSet {
    #[serde(default)]
    pub traversal_mode: TraversalMode,

    /// A short kebab-case tag describing the page shape, e.g. `self_text`,
    /// `image_post`, `link_post`, `gallery`, `video_embed`, `xenforo_text_thread`.
    /// Used as the second half of the (host, content_type) cache key. The LLM
    /// is free to invent new tags; the dispatcher only cares about uniqueness
    /// within a host.
    #[serde(default = "default_content_type")]
    pub content_type: String,

    /// CSS selector that uniquely matches THIS content type on the live page.
    /// On future visits to the same host, the dispatcher runs each cached
    /// profile's probe; first match wins and skips the API call. If no probe
    /// matches, the learner is invoked again to produce a profile for the new
    /// content shape.
    #[serde(default)]
    pub content_probe: String,

    pub post_selector: String,
    #[serde(default)]
    pub op_selector: Option<String>,

    pub author_selector: String,
    #[serde(default)]
    pub author_url_selector: Option<String>,
    #[serde(default)]
    pub avatar_selector: Option<String>,

    #[serde(default)]
    pub author_rank_selector: Option<String>,
    #[serde(default)]
    pub author_post_count_selector: Option<String>,
    #[serde(default)]
    pub author_join_date_selector: Option<String>,
    #[serde(default)]
    pub author_location_selector: Option<String>,

    #[serde(default)]
    pub timestamp_selector: Option<String>,

    /// Optional override for the OP/first-post body when its DOM shape
    /// differs from comments/replies. Example: on Reddit link posts, the
    /// OP's "body" is `.entry p.title` (the article link) but comments use
    /// `.entry .usertext-body .md` (prose). When set, the extractor uses
    /// this for whichever element op_selector matches and the regular
    /// body_selector for everything else.
    #[serde(default)]
    pub op_body_selector: Option<String>,

    /// Per-post like / upvote / reaction count. Examples:
    /// `.score.unvoted` (old.reddit), `.reaction-count` (XenForo),
    /// `.discourse-reactions-counter` (Discourse), `.like-count` (IPB).
    /// When this selector is set, the renderer shows a "★ N likes" footer
    /// on every post (with "0 likes" when the count parses to 0 or less).
    /// When unset, no footer renders — interpreted as "site has no like system."
    #[serde(default)]
    pub like_count_selector: Option<String>,

    pub body_selector: String,

    /// Optional — the post's signature block (typically below the body,
    /// separated by a divider in classic vB).
    #[serde(default)]
    pub signature_selector: Option<String>,

    #[serde(default)]
    pub post_number_selector: Option<String>,
    #[serde(default)]
    pub quote_block_selector: Option<String>,

    /// Reddit-style. First match within each parent post is taken; rest dropped.
    #[serde(default)]
    pub child_comment_selector: Option<String>,
    #[serde(default)]
    pub child_comment_author_selector: Option<String>,
    #[serde(default)]
    pub child_comment_body_selector: Option<String>,

    #[serde(default)]
    pub pagination_next: Option<String>,

    /// CSS selectors for buttons that, when clicked, reveal inline media
    /// (image previews, video previews, NSFW blurs, gallery expanders) that
    /// would otherwise be hidden in the captured DOM. Common example:
    /// old.reddit's `.expando-button.collapsed`. The dispatcher auto-clicks
    /// all matches once per thread session after the first scrape, then
    /// re-scrapes to capture the expanded content.
    #[serde(default)]
    pub media_expand_selectors: Vec<String>,

    /// EXTRA CSS selectors (relative to each post container) whose inner_html
    /// gets appended to the rendered body_html. Use when inline media lives
    /// in a SIBLING of the main body container — e.g. on old.reddit the
    /// `.expando` div with the image preview is a sibling of `.usertext`,
    /// not a child, so a body_selector like `.usertext-body .md` would
    /// otherwise lose the image. Empty by default; populate when needed.
    #[serde(default)]
    pub extra_media_selectors: Vec<String>,

    /// How the page reveals more comments/posts when the user reaches the
    /// bottom of the currently rendered set. The dispatcher uses this to
    /// pick what JS to inject when lazy-load triggers.
    ///
    ///   - "click_button"     → click `load_more_selector` (Reddit's "load more comments")
    ///   - "infinite_scroll"  → window.scrollTo(0, document.body.scrollHeight)
    ///   - "next_page"        → navigate to `pagination_next` URL
    ///   - "none"             → no lazy-load mechanism available (default)
    #[serde(default = "default_load_more_strategy")]
    pub load_more_strategy: String,

    /// CSS selector for the "load more" / "show more replies" button.
    /// Only meaningful when `load_more_strategy == "click_button"`.
    #[serde(default)]
    pub load_more_selector: Option<String>,

    /// URL pattern for a single user's profile. `{author}` is replaced with
    /// the post's author handle (URL-encoded). Examples:
    ///   Reddit:    `https://www.reddit.com/user/{author}/about.json`
    ///   Discourse: `https://community.example.com/u/{author}.json`
    ///   XenForo:   `https://forum.example.com/members/{author}/`
    /// Used together with `author_avatar_format` + `author_avatar_locator`
    /// to enrich rendered posts with avatars from off-thread profile pages.
    /// All three must be set together; if any is empty, enrichment skips.
    #[serde(default)]
    pub author_profile_url_template: Option<String>,

    /// Tells the enrichment fetcher how to parse the response from
    /// `author_profile_url_template`. `"json"` → JSON dot-path locator,
    /// `"html"` → CSS selector locator. Set only when the template is set.
    #[serde(default)]
    pub author_avatar_format: Option<String>,

    /// Path to the avatar URL within the profile response:
    ///   format=json: dot-notation path, e.g. `data.icon_img` or `user.avatar_template`
    ///   format=html: CSS selector for the avatar `<img>` element,
    ///                e.g. `.message-avatar img`, `.userBanner img`,
    ///                `img.avatar`
    #[serde(default)]
    pub author_avatar_locator: Option<String>,

    /// Same format/shape as `author_avatar_locator` but for additional
    /// per-author metadata pulled from their profile page. All optional —
    /// the agent populates what's available on each platform. Values are
    /// stored as opaque strings (we don't try to parse them).
    #[serde(default)]
    pub author_post_count_locator: Option<String>,
    #[serde(default)]
    pub author_karma_locator: Option<String>,
    #[serde(default)]
    pub author_join_date_locator: Option<String>,
    #[serde(default)]
    pub author_last_active_locator: Option<String>,
    #[serde(default)]
    pub author_location_locator: Option<String>,
    #[serde(default)]
    pub author_rank_locator: Option<String>,

    #[serde(default)]
    pub platform_guess: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_load_more_strategy() -> String { "none".to_string() }

/// Decoded payload from the injected scrape script in the child webview.
#[derive(Debug, Clone, Deserialize)]
pub struct ScrapeSample {
    pub kind: String,
    pub host: String,
    pub url: String,
    pub html: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostsReadyPayload {
    pub host: String,
    pub url: String,
    pub posts: Vec<Post>,
    pub source: String, // "anthropic" | "manual"
    pub content_type: String,
}

/// Parse a raw message string from `Webview::on_message_received` into a typed
/// `ScrapeSample`. Returns `None` for unrelated chatter (the webview may post
/// other kinds in the future).
pub fn decode_message(raw: &str) -> Option<ScrapeSample> {
    let sample: ScrapeSample = serde_json::from_str(raw).ok()?;
    if sample.kind != "scrape_sample" {
        return None;
    }
    Some(sample)
}

/// Validate that a freshly applied profile produced sensible output. Used by
/// the calling code to decide whether to bump success_count or fail_count and
/// schedule a re-learn.
///
/// Tiered rules:
///   - 0 posts → invalid (extraction completely missed)
///   - > 500 posts → invalid (post_selector too greedy, catching nav rows)
///   - 1 post → accept as long as the post has either an author OR a body.
///     This is the legitimate "link post with no comments yet" / fresh thread
///     case — Reddit link OPs have no body, and the thread may have zero
///     replies, but that's still a valid extraction we should render.
///   - 2+ posts → require ≥80% to have an author AND ≥60% to have a body.
///     Body threshold is looser because some posts in the wild are
///     image-only / quote-only / deleted-with-stub which legitimately have
///     empty `.usertext-body .md`.
pub fn posts_look_valid(posts: &[Post]) -> bool {
    if posts.is_empty() {
        return false;
    }
    if posts.len() > 500 {
        return false;
    }
    let count = posts.len();
    let with_author = posts.iter().filter(|p| !p.author.is_empty()).count();
    let with_body = posts.iter().filter(|p| p.body_html.trim().len() >= 4).count();

    if count == 1 {
        return with_author >= 1 || with_body >= 1;
    }
    let author_threshold = (count as f64 * 0.80).ceil() as usize;
    let body_threshold = (count as f64 * 0.60).ceil() as usize;
    with_author >= author_threshold && with_body >= body_threshold
}

/// Convenience: parse a JSON string to `SelectorSet` (e.g. from `site_profiles.selectors_json`).
pub fn parse_selectors(json: &str) -> Result<SelectorSet> {
    Ok(serde_json::from_str(json)?)
}

/// Overlay `override_` onto `base`. For required string fields, take override
/// only when non-empty. For Option fields, take override only when Some. This
/// prevents a critique that omits a field from clobbering a working selector.
pub fn merge_selectors(base: &SelectorSet, over: &SelectorSet) -> SelectorSet {
    fn pick_str(b: &str, o: &str) -> String {
        if o.trim().is_empty() { b.to_string() } else { o.to_string() }
    }
    fn pick_opt(b: &Option<String>, o: &Option<String>) -> Option<String> {
        match o {
            Some(v) if !v.trim().is_empty() => Some(v.clone()),
            _ => b.clone(),
        }
    }
    SelectorSet {
        traversal_mode: if matches!(over.traversal_mode, TraversalMode::Linear)
            && !matches!(base.traversal_mode, TraversalMode::Linear)
        {
            base.traversal_mode
        } else {
            over.traversal_mode
        },
        content_type: pick_str(&base.content_type, &over.content_type),
        content_probe: pick_str(&base.content_probe, &over.content_probe),
        post_selector: pick_str(&base.post_selector, &over.post_selector),
        op_selector: pick_opt(&base.op_selector, &over.op_selector),
        author_selector: pick_str(&base.author_selector, &over.author_selector),
        author_url_selector: pick_opt(&base.author_url_selector, &over.author_url_selector),
        avatar_selector: pick_opt(&base.avatar_selector, &over.avatar_selector),
        author_rank_selector: pick_opt(&base.author_rank_selector, &over.author_rank_selector),
        author_post_count_selector: pick_opt(&base.author_post_count_selector, &over.author_post_count_selector),
        author_join_date_selector: pick_opt(&base.author_join_date_selector, &over.author_join_date_selector),
        author_location_selector: pick_opt(&base.author_location_selector, &over.author_location_selector),
        timestamp_selector: pick_opt(&base.timestamp_selector, &over.timestamp_selector),
        op_body_selector: pick_opt(&base.op_body_selector, &over.op_body_selector),
        like_count_selector: pick_opt(&base.like_count_selector, &over.like_count_selector),
        body_selector: pick_str(&base.body_selector, &over.body_selector),
        signature_selector: pick_opt(&base.signature_selector, &over.signature_selector),
        post_number_selector: pick_opt(&base.post_number_selector, &over.post_number_selector),
        quote_block_selector: pick_opt(&base.quote_block_selector, &over.quote_block_selector),
        child_comment_selector: pick_opt(&base.child_comment_selector, &over.child_comment_selector),
        child_comment_author_selector: pick_opt(&base.child_comment_author_selector, &over.child_comment_author_selector),
        child_comment_body_selector: pick_opt(&base.child_comment_body_selector, &over.child_comment_body_selector),
        pagination_next: pick_opt(&base.pagination_next, &over.pagination_next),
        media_expand_selectors: if over.media_expand_selectors.is_empty() {
            base.media_expand_selectors.clone()
        } else {
            over.media_expand_selectors.clone()
        },
        extra_media_selectors: if over.extra_media_selectors.is_empty() {
            base.extra_media_selectors.clone()
        } else {
            over.extra_media_selectors.clone()
        },
        load_more_strategy: if over.load_more_strategy.trim().is_empty() || over.load_more_strategy == "none" {
            base.load_more_strategy.clone()
        } else {
            over.load_more_strategy.clone()
        },
        load_more_selector: pick_opt(&base.load_more_selector, &over.load_more_selector),
        author_profile_url_template: pick_opt(&base.author_profile_url_template, &over.author_profile_url_template),
        author_avatar_format: pick_opt(&base.author_avatar_format, &over.author_avatar_format),
        author_avatar_locator: pick_opt(&base.author_avatar_locator, &over.author_avatar_locator),
        author_post_count_locator: pick_opt(&base.author_post_count_locator, &over.author_post_count_locator),
        author_karma_locator: pick_opt(&base.author_karma_locator, &over.author_karma_locator),
        author_join_date_locator: pick_opt(&base.author_join_date_locator, &over.author_join_date_locator),
        author_last_active_locator: pick_opt(&base.author_last_active_locator, &over.author_last_active_locator),
        author_location_locator: pick_opt(&base.author_location_locator, &over.author_location_locator),
        author_rank_locator: pick_opt(&base.author_rank_locator, &over.author_rank_locator),
        platform_guess: pick_opt(&base.platform_guess, &over.platform_guess),
        notes: pick_opt(&base.notes, &over.notes),
    }
}

/// Average number of populated fields per post. Used to compare an
/// original-vs-critique extraction and decide whether to keep the critique.
pub fn field_density(posts: &[Post]) -> f64 {
    if posts.is_empty() {
        return 0.0;
    }
    let total: usize = posts
        .iter()
        .map(|p| {
            let mut n = 0;
            if !p.author.is_empty() { n += 1; }
            if !p.body_html.trim().is_empty() { n += 1; }
            if p.author_url.is_some() { n += 1; }
            if p.avatar_url.is_some() { n += 1; }
            if p.author_rank.is_some() { n += 1; }
            if p.author_post_count.is_some() { n += 1; }
            if p.author_join_date.is_some() { n += 1; }
            if p.author_location.is_some() { n += 1; }
            if p.timestamp.is_some() { n += 1; }
            if p.signature_html.is_some() { n += 1; }
            n
        })
        .sum();
    total as f64 / posts.len() as f64
}

/// End-to-end pipeline:
///   1. Walk every cached profile for `host`; run its `content_probe` against
///      the live DOM. First hit wins → apply its selectors.
///   2. If no probe matches: minify DOM, ask Anthropic to derive selectors
///      for the page shape (passing the list of existing content_types so the
///      model is steered toward a distinct tag), persist profile.
///   3. Apply selectors to the rendered HTML → `Vec<Post>`.
///   4. Validate; bump success/fail counters on the chosen profile.
///   5. Cache posts; return (posts, source, content_type).
///
/// Returns `Ok(None)` only when no probe matched AND the learner can't
/// produce a working profile (no API key, network failure, malformed tool
/// response, validation failure). The caller should treat `None` as
/// "fall back to webview-only mode for now."
pub async fn handle_sample(
    pool: &sqlx::SqlitePool,
    host: &str,
    url: &str,
    html: &str,
    thread_id: Option<i64>,
    platform_hint: Option<&str>,
) -> Result<Option<PostsReadyPayload>> {
    let existing_profiles = crate::db::list_site_profiles_for_host(pool, host).await?;

    // Try probes in order: first profile whose content_probe matches this DOM
    // is the one we apply. Skip profiles with empty probes (legacy / fallback).
    let matched = existing_profiles
        .iter()
        .find(|p| !p.content_probe.trim().is_empty() && extract::probe_matches(html, &p.content_probe));

    let (mut selectors, source, mut content_type, just_learned) = if let Some(row) = matched {
        let selectors = parse_selectors(&row.selectors_json)?;
        tracing::info!(host, content_type=%row.content_type, "matched cached profile via probe");
        (selectors, row.source.clone(), row.content_type.clone(), false)
    } else {
        let existing_types: Vec<String> =
            existing_profiles.iter().map(|p| p.content_type.clone()).collect();
        tracing::info!(
            host,
            existing_types = ?existing_types,
            "no matching probe — invoking anthropic learner"
        );
        let skeleton = skeleton::minify(html);
        let learned = match crate::anthropic::learn(host, url, &skeleton, platform_hint, &existing_types).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(host, err=%e, "anthropic learn failed");
                return Ok(None);
            }
        };
        let mut sel = learned.selectors;
        if sel.content_type.trim().is_empty() {
            sel.content_type = "default".to_string();
        }
        let selectors_json = serde_json::to_string(&sel)?;
        crate::db::upsert_site_profile(
            pool,
            host,
            &sel.content_type,
            &sel.content_probe,
            &selectors_json,
            "anthropic",
        )
        .await?;
        let ct = sel.content_type.clone();
        (sel, "anthropic".to_string(), ct, true)
    };

    let mut posts = extract::apply(html, &selectors, Some(url))?;

    // AGENTIC SELF-CRITIQUE: only on a fresh learn (not a cache hit), AND
    // only when the initial extraction looks shaky. Gating on quality keeps
    // the typical first-visit at 1 API call instead of 2 — we only spend a
    // critique call when there's reason to believe selectors need a fix.
    //
    // SAFETY: merge over original (don't replace) so any field the critique
    // didn't explicitly correct keeps working. Then re-extract and compare
    // field density — if the critique made things sparser, revert.
    let extraction_looks_clean = {
        let count = posts.len();
        let with_author = posts.iter().filter(|p| !p.author.is_empty()).count();
        let with_body = posts
            .iter()
            .filter(|p| p.body_html.trim().chars().count() >= 8)
            .count();
        let density = field_density(&posts);
        // Three signals must all be green:
        //   • >=3 posts (single-post extraction often means body_selector
        //     matched the wrong scope)
        //   • EVERY post has both author and body populated
        //   • average populated fields/post >= 3 (author + body + at least
        //     one sidebar field like timestamp/rank/post-count)
        count >= 3 && with_author == count && with_body == count && density >= 3.0
    };
    if just_learned && !posts.is_empty() && !extraction_looks_clean {
        tracing::info!(
            host,
            post_count = posts.len(),
            "extraction looks borderline; running critique pass"
        );
        let sample = posts.iter().take(3).collect::<Vec<_>>();
        let sample_json = serde_json::to_string_pretty(&sample).unwrap_or_else(|_| "[]".into());
        let current_json = serde_json::to_string(&selectors).unwrap_or_default();
        let skel = skeleton::minify(html);
        match crate::anthropic::critique(host, url, &current_json, &sample_json, &skel).await {
            Ok(Some(corrected)) => {
                let merged = merge_selectors(&selectors, &corrected.selectors);
                let merged_posts = extract::apply(html, &merged, Some(url))?;
                let before_density = field_density(&posts);
                let after_density = field_density(&merged_posts);
                let before_count = posts.len();
                let after_count = merged_posts.len();

                // Keep the critique only if it didn't shrink the post count
                // meaningfully AND average field density didn't drop > 30%.
                let post_count_ok = after_count >= (before_count as f64 * 0.7).floor() as usize;
                let density_ok = after_density >= before_density * 0.7;

                if post_count_ok && density_ok {
                    let new_json = serde_json::to_string(&merged)?;
                    let parent_rev = crate::db::get_site_profile(pool, host, &content_type)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|r| r.current_revision_id);
                    let _ = crate::db::insert_profile_revision(
                        pool,
                        host,
                        &merged.content_type,
                        &merged.content_probe,
                        &new_json,
                        "anthropic-critique",
                        parent_rev,
                        Some("self-critique correction"),
                    )
                    .await;
                    content_type = merged.content_type.clone();
                    selectors = merged;
                    posts = merged_posts;
                    tracing::info!(
                        host,
                        before_count,
                        after_count,
                        before_density,
                        after_density,
                        "critique applied"
                    );
                } else {
                    tracing::info!(
                        host,
                        before_count,
                        after_count,
                        before_density,
                        after_density,
                        "critique would have regressed extraction; reverting"
                    );
                }
            }
            Ok(None) => tracing::debug!(host, "critique: selectors look fine"),
            Err(e) => tracing::warn!(host, err=%e, "critique pass failed; sticking with original selectors"),
        }
    } else if just_learned && posts.is_empty() {
        // 0 posts is never "clean" — it's a hard miss. Could be a JS-rendered
        // page that hadn't hydrated when we scraped, or a wholly wrong
        // post_selector. Surface it distinctly so it doesn't look like success.
        tracing::warn!(host, "anthropic learn returned profile but extraction yielded 0 posts (likely JS-rendered site captured before hydration)");
    } else if just_learned {
        tracing::info!(
            host,
            post_count = posts.len(),
            "extraction looks clean; skipping critique pass to save API call"
        );
    }

    if posts_look_valid(&posts) {
        let _ = crate::db::bump_profile_success(pool, host, &content_type).await;
        if let Some(tid) = thread_id {
            if let Ok(json) = serde_json::to_string(&posts) {
                let _ = crate::db::cache_posts(pool, tid, &json).await;
            }
        }
        Ok(Some(PostsReadyPayload {
            host: host.to_string(),
            url: url.to_string(),
            posts,
            source,
            content_type,
        }))
    } else {
        let _ = crate::db::bump_profile_fail(pool, host, &content_type, "validation failed").await;
        tracing::warn!(host, content_type, post_count = posts.len(), "extraction did not pass validation");
        // After 3 failures of a profile that's never succeeded, drop just THIS
        // (host, content_type) so the next visit can re-learn — leave sibling
        // profiles for the same host (different content shapes) intact.
        if let Some(row) = existing_profiles.iter().find(|p| p.content_type == content_type) {
            if row.fail_count + 1 >= 3 && row.fail_count >= row.success_count {
                tracing::info!(host, content_type, "deleting persistently failing profile for re-learn");
                let _ = crate::db::delete_site_profile_one(pool, host, &content_type).await;
            }
        }
        Ok(None)
    }
}
