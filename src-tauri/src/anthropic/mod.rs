// Anthropic Messages API client — the site-profile learner. Given a minified
// DOM skeleton of a thread page, asks Claude to derive CSS selectors for each
// per-post field. The response is constrained via tool-use so we get JSON in
// the SelectorSet shape directly, no regex-fishing needed.
//
// Reads `ANTHROPIC_API_KEY` from env at call time. If absent, returns an error
// the caller can downgrade into "webview-only mode for unknown hosts."

pub mod feeds_agent;

use crate::scrape::SelectorSet;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::{debug, info, warn};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 1024;
const TOOL_NAME: &str = "derive_site_profile";

#[derive(Debug, Clone, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    tools: Vec<Value>,
    tool_choice: Value,
    messages: Vec<Message<'a>>,
}

#[derive(Debug, Clone, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { name: String, input: Value },
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

pub struct LearnedProfile {
    pub selectors: SelectorSet,
    pub usage: Usage,
}

/// Call Anthropic to derive selectors for the given DOM skeleton.
///
/// `existing_content_types` is the list of content_type tags already cached
/// for this host. The model is told these have profiles already, so if the
/// current page matches one of them it should re-emit that exact tag — but
/// it should also fix its `content_probe` so future visits hit the cache.
/// Otherwise the model picks a NEW distinct tag.
pub async fn learn(
    host: &str,
    url: &str,
    dom_skeleton: &str,
    platform_hint: Option<&str>,
    existing_content_types: &[String],
) -> Result<LearnedProfile> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow!("ANTHROPIC_API_KEY not set"))?;

    let tool = tool_definition();
    let user_msg = build_user_prompt(host, url, dom_skeleton, platform_hint, existing_content_types);

    let req = MessagesRequest {
        model: MODEL,
        max_tokens: MAX_TOKENS,
        tools: vec![tool],
        tool_choice: json!({ "type": "tool", "name": TOOL_NAME }),
        messages: vec![Message {
            role: "user",
            content: &user_msg,
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building anthropic http client")?;

    let mut attempt = 0;
    let max_attempts = 3;
    let response_body: MessagesResponse = loop {
        attempt += 1;
        let resp = client
            .post(ANTHROPIC_URL)
            .header("x-api-key", &api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let parsed: MessagesResponse =
                    r.json().await.context("decoding anthropic response")?;
                break parsed;
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                warn!(host, %status, body=%truncate(&body, 240), attempt, "anthropic non-2xx");
                if retryable && attempt < max_attempts {
                    let backoff = Duration::from_millis(500 * (1 << attempt));
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                bail!("anthropic api {status}: {}", truncate(&body, 240));
            }
            Err(e) => {
                warn!(host, err=%e, attempt, "anthropic request failed");
                if attempt < max_attempts {
                    tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
                    continue;
                }
                bail!("anthropic request failed: {e}");
            }
        }
    };

    let usage = response_body.usage.clone().unwrap_or_default();
    debug!(host, ?usage, "anthropic usage");
    info!(host, "received site_profile from anthropic");

    let tool_input = response_body
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::ToolUse { name, input } if name == TOOL_NAME => Some(input.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            // surface what we DID get for debugging
            let text = response_body
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ");
            anyhow!(
                "anthropic did not return tool_use {TOOL_NAME}; stop_reason={:?}; text fallback={:?}",
                response_body.stop_reason,
                truncate(&text, 240)
            )
        })?;

    let selectors: SelectorSet = serde_json::from_value(tool_input)
        .context("parsing tool_use input as SelectorSet")?;

    Ok(LearnedProfile { selectors, usage })
}

fn tool_definition() -> Value {
    json!({
        "name": TOOL_NAME,
        "description": "Given a minified DOM skeleton of a forum-thread page, return CSS selectors that locate each post and its sub-fields (author, body, profile metadata, etc.), PLUS identify the page's content type and emit a CSS probe selector that uniquely indicates 'this is a page of THIS type'. The selectors will be applied to similar pages on future visits to the same host. The probe is consulted FIRST on each future visit — if it matches, we re-use this profile and skip the API call entirely. Profiles are cached per (host, content_type) tuple, so different shapes (self-text post / image post / gallery / video / link post) get separate profiles.\n\nIMPORTANT RULES:\n1. Pick a stable, short, kebab-case `content_type` like `self_text`, `image_post`, `link_post`, `gallery`, `video_embed`, `xenforo_text_thread`, `discourse_topic`. If the user supplies a list of existing content types for this host and the current page matches one of them, REUSE that exact tag.\n2. `content_probe` MUST be a CSS selector that matches on THIS page but would NOT match on a different content type on the same host. Examples: for a Reddit image post, `#siteTable .thing.link[data-domain^='i.']`; for a Reddit self-text, `#siteTable .thing.link.self`; for a Reddit gallery, `#siteTable .thing.link.gallery-link`. Verify in your head the probe is selective.\n3. If the page is a Reddit-style threaded comment tree, set traversal_mode=threaded_replies and populate op_selector + child_comment_selector (we only follow depth 1).\n4. Verify each selector returns the right shape against the skeleton before responding.",
        "input_schema": {
            "type": "object",
            "required": ["post_selector", "author_selector", "body_selector", "content_type", "content_probe"],
            "properties": {
                "content_type": {
                    "type": "string",
                    "description": "Short kebab-case tag identifying this page's content shape. Reuse an existing tag if the page matches one; otherwise pick a new distinct tag."
                },
                "content_probe": {
                    "type": "string",
                    "description": "CSS selector that returns >=1 match on THIS page but would NOT match on a different content_type on the same host. The dispatcher uses this to skip the API call on future visits."
                },
                "traversal_mode": {
                    "type": "string",
                    "enum": ["linear", "threaded_replies"],
                    "description": "linear for typical forums (XenForo, vBulletin, Discourse, phpBB). threaded_replies for Reddit-style hierarchical comment trees."
                },
                "post_selector": {
                    "type": "string",
                    "description": "Top-level post or top-level comment container. Must match every post on the page in DOM order."
                },
                "op_selector": {
                    "type": "string",
                    "description": "Only for threaded_replies — selects the original link/self-post that started the thread (separate from comments)."
                },
                "author_selector": {"type": "string", "description": "Author/username element within each post."},
                "author_url_selector": {"type": "string", "description": "<a> element whose href links to the author's profile."},
                "avatar_selector": {"type": "string", "description": "<img> for author avatar."},
                "author_rank_selector": {"type": "string", "description": "Member title / rank / trust level — e.g. 'Senior Member', 'Private First Class', 'New Member', 'Regular', a Discourse trust badge. On classic vBulletin (BimmerPost, bimmerforums, e90post): look for `.usertitle` or the rank text rendered just below the username in the author sidebar TD. STRONGLY ENCOURAGED — emit a selector whenever there's ANY label text under the username."},
                "author_post_count_selector": {"type": "string", "description": "Total post count for the author. On XenForo: `dl.message-userExtras dd` or `.userStats` blocks. On classic vBulletin: a `<div class='smallfont'>` inside the sidebar TD with text like 'Posts: 6' — or a labelled stats block where the label cell says 'Posts' and value is a number. STRONGLY ENCOURAGED — verify by looking for the literal text 'Posts' / 'Post Count' nearby."},
                "author_join_date_selector": {"type": "string", "description": "Account creation date. On vBulletin: text like 'Join Date: Dec 2023' rendered inline in the sidebar; pick the element whose text starts with 'Join Date' (often the parent of a sibling-style block). STRONGLY ENCOURAGED."},
                "author_location_selector": {"type": "string", "description": "Author location — 'Location: SoCal'. On vBulletin: same sidebar TD as join date. STRONGLY ENCOURAGED — populate if visible."},
                "timestamp_selector": {"type": "string", "description": "When the post was made. Prefer an element with a datetime attribute."},
                "like_count_selector": {"type": "string", "description": "Per-post LIKES / UPVOTES / REACTIONS / THANKS count. Examples: old.reddit `.score.unvoted`; XenForo `.reactionsBar-link` or `.reaction-count`; Discourse `.discourse-reactions-counter-anim` or the post's `data-likes` attribute; IPB `.cReact_listToggle` count badge; later vBulletin's 'Thanks' counter. ONLY emit when the site clearly has a like/upvote/thanks system on individual posts. LEAVE UNSET on classic forums without such a feature (most vBulletin 3.x boards like BimmerPost don't have it)."},
                "body_selector": {"type": "string", "description": "Container for the post's PROSE content (text, inline links, formatting). The inner_html of this element gets sanitized and rendered. Examples: old.reddit `.usertext-body .md`, XenForo `.bbWrapper`, vBulletin `.postcontent`. If the post has inline media that lives INSIDE this container (XenForo, vBulletin where images are embedded in BBCode), the body_selector alone is fine. If media lives in a SIBLING container (old.reddit's `.expando` is a sibling of `.usertext`, not a child), do NOT widen body_selector — instead populate extra_media_selectors with the sibling's selector."},
                "op_body_selector": {"type": "string", "description": "Optional override for the OP's body, used WHEN AND ONLY WHEN the OP has a different DOM shape than comments. Most relevant on Reddit: link/image/video/gallery posts have no `.usertext-body` on the OP — its body is `.entry p.title`. Set op_body_selector to that title selector so the OP renders as the clickable headline, and leave body_selector pointing at `.entry .usertext-body .md` for comments. Leave empty on self_text posts and on any platform where OP and comments share the same body shape (XenForo, vBulletin, Discourse, phpBB, etc)."},
                "signature_selector": {"type": "string", "description": "Author's signature — the small block beneath the post body that's repeated across all of that author's posts (often after a horizontal rule or in a smaller font). Common in vBulletin, phpBB, XenForo. Look for `.signature`, `.sig`, `<div>` with class containing 'sig', or text consistently following an <hr> at the end of post bodies. Populate if visible."},
                "post_number_selector": {"type": "string", "description": "Post-position indicator like '#5' or 'Post #5'."},
                "quote_block_selector": {"type": "string", "description": "If the post quotes an earlier post, the element that carries the source post number (often a data-source attribute)."},
                "child_comment_selector": {"type": "string", "description": "Reddit-style: from each top-level comment's subtree, the FIRST descendant that is itself a comment is treated as the top reply. Deeper replies are dropped."},
                "child_comment_author_selector": {"type": "string"},
                "child_comment_body_selector": {"type": "string"},
                "pagination_next": {"type": "string", "description": "<a> element pointing at the next page of this thread, if multi-page."},
                "media_expand_selectors": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "CSS selectors for buttons that REVEAL hidden inline media when clicked: image-preview expanders, video-preview expanders, NSFW blur reveals, gallery openers. Examples: on old.reddit, `.expando-button.collapsed` (image/video previews on link posts and comments). On Discourse, `.lightbox` thumbnails sometimes have collapse states. On most plain forums this is EMPTY because images render inline by default. The dispatcher auto-clicks all matches once per thread session, then re-scrapes. ONLY emit selectors that EXPAND existing-in-DOM media — NOT 'show more comments' buttons (use load_more_selector for those)."
                },
                "extra_media_selectors": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "CSS selectors (relative to each post container) for SIBLING or COUSIN elements whose content should be APPENDED to body_html. Use ONLY when inline media lives OUTSIDE body_selector AND would not otherwise be visible.\n\nCRITICAL RULE: each selector here must match an element that is NOT a descendant of, ancestor of, or the same node as body_selector. Overlapping selectors duplicate the post's text on render.\n\nVerification before emitting: mentally walk the DOM — find the element body_selector points to, then find the element your candidate selector points to. If one is contained in the other, DO NOT include it. Prefer to TIGHTEN your body_selector or REMOVE this entry over including an overlap.\n\nLeave empty when body_selector already captures all media (most XenForo/vBulletin/Discourse forums). Only populate when the media container is a true sibling or cousin of the body — e.g. a preview/attachment block rendered next to (not inside) the post content."
                },
                "load_more_strategy": {
                    "type": "string",
                    "enum": ["click_button", "infinite_scroll", "next_page", "none"],
                    "description": "How does this site reveal more comments/posts beyond what's currently rendered? click_button: there's a button to expand more comments inline (Reddit's '.morecomments', Discourse's 'Load more', XenForo's 'Show ignored content'). infinite_scroll: more loads automatically when you scroll to the bottom. next_page: classic forum pagination — there's a 'Next page' link (also set pagination_next). none: this page already shows everything (rare; only if you really can't find a mechanism)."
                },
                "load_more_selector": {
                    "type": "string",
                    "description": "CSS selector for the 'load more comments' / 'show more replies' button. ONLY used when load_more_strategy == 'click_button'. Pick the OUTERMOST such button that, when clicked, expands the most content."
                },
                "author_profile_url_template": {
                    "type": "string",
                    "description": "URL template for an individual user's profile page or profile API endpoint. Use `{author}` as the username placeholder — URL-encoded and substituted at fetch time. The fetch runs INSIDE the live WebView via JS `fetch()`, so it: (a) uses the user's authenticated cookies automatically, and (b) is same-origin which avoids CORS. **USE A RELATIVE PATH OR SAME-ORIGIN URL** matching the host of the thread page. EXAMPLES: Reddit (page is on old.reddit.com): '/user/{author}/about.json'. Discourse: '/u/{author}.json'. XenForo: '/members/{author}/'. vBulletin: '/member.php?username={author}'. PREFER root-relative paths over absolute URLs. SET this when you can identify the convention from existing profile links in the thread DOM. LEAVE EMPTY if avatars are already in the rendered thread (avatar_selector covers them) OR you can't infer a pattern."
                },
                "author_avatar_format": {
                    "type": "string",
                    "enum": ["json", "html"],
                    "description": "How to parse the response from author_profile_url_template. 'json' for endpoints that return JSON (Reddit's about.json, Discourse's .json). 'html' for endpoints that return an HTML profile page (XenForo /members/, vBulletin member.php). Set only when author_profile_url_template is set."
                },
                "author_avatar_locator": {
                    "type": "string",
                    "description": "How to find the avatar URL within the profile response. When author_avatar_format='json', use dot-notation JSON path: e.g. 'data.icon_img' (Reddit), 'data.snoovatar_img' (Reddit fancy avatar), 'user.avatar_template' (Discourse — note this often has a {size} placeholder; pick the larger one). When author_avatar_format='html', use a CSS selector for the avatar `<img>` element on the profile page: e.g. '.avatarBg img', '.userBanner img', '.message-avatar img', 'img.avatar'. Set only when author_profile_url_template is set."
                },
                "author_post_count_locator": {
                    "type": "string",
                    "description": "Same shape as author_avatar_locator (JSON dot-path OR CSS selector depending on author_avatar_format). Locates the user's TOTAL POST/COMMENT COUNT on their profile. Reddit JSON: 'data.total_karma' isn't post count — for Reddit there's no clean post count field, omit this. Discourse JSON: 'user.post_count'. XenForo HTML: '.message-userExtras dd' (find the dd inside dl labelled 'Messages' or 'Posts'). vBulletin HTML: usually a sidebar div labelled 'Posts: NNN'. Populate when the profile exposes it."
                },
                "author_karma_locator": {
                    "type": "string",
                    "description": "Locator for total karma / reputation / likes received. Reddit JSON: 'data.total_karma'. Discourse JSON: 'user.likes_received' or 'user.badge_count'. XenForo: '.reactionScore', '.reactionsBar-link span'. Populate when the platform has a karma/rep concept on the profile."
                },
                "author_join_date_locator": {
                    "type": "string",
                    "description": "Account creation / join date. Reddit JSON: 'data.created_utc' (Unix timestamp). Discourse JSON: 'user.created_at' (ISO). XenForo HTML: text in '.message-userDetails .userTitle' or stats blocks labelled 'Joined'. vBulletin HTML: 'Join Date: ...' sidebar text."
                },
                "author_last_active_locator": {
                    "type": "string",
                    "description": "Last seen / last active marker. Reddit JSON: usually no field — omit. Discourse JSON: 'user.last_seen_at'. XenForo HTML: '.lastSeen' or 'Last Activity:' labelled element. vBulletin: 'Last Activity: ...' sidebar text."
                },
                "author_location_locator": {
                    "type": "string",
                    "description": "User's location/city from their profile. Reddit JSON: no field. Discourse JSON: 'user.location'. XenForo HTML: a .userBanner-name sibling div or labelled stat. vBulletin HTML: 'Location: ...' sidebar."
                },
                "author_rank_locator": {
                    "type": "string",
                    "description": "Profile-page version of author_rank_selector — the user's rank/trust-level/member-title shown on their profile rather than per-comment. Reddit JSON: no field (Reddit has no ranks). Discourse JSON: 'user.trust_level' (returns 0-4; renderer will display it raw) or 'user.title'. XenForo HTML: '.userBanner-name' or member title element. vBulletin: usertitle text in sidebar."
                },
                "platform_guess": {"type": "string", "description": "Your best guess at the platform name (xenforo / vbulletin / discourse / phpbb / smf / ipb / reddit / other)."},
                "notes": {"type": "string", "description": "Brief notes about anything unusual — paywalls, JS-only rendering, etc."}
            }
        }
    })
}

/// Agentic self-critique pass. Called once immediately after a fresh `learn`.
/// We hand the model BACK its own selectors PLUS a small sample of what those
/// selectors produced when applied to the live page. The model decides whether
/// the output looks like real forum post data — usernames that look like
/// usernames, distinct sidebar fields, no JavaScript bleeding through — and
/// returns either the same SelectorSet (verdict OK) or a corrected one.
///
/// Returns `Ok(None)` when the model judges the original selectors are fine.
pub async fn critique(
    host: &str,
    url: &str,
    current_selectors: &str,
    extracted_sample_json: &str,
    dom_skeleton: &str,
) -> Result<Option<LearnedProfile>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow!("ANTHROPIC_API_KEY not set"))?;

    let tool = critique_tool_definition();
    let user_msg = build_critique_prompt(host, url, current_selectors, extracted_sample_json, dom_skeleton);

    let req = MessagesRequest {
        model: MODEL,
        max_tokens: MAX_TOKENS,
        tools: vec![tool],
        tool_choice: json!({ "type": "tool", "name": "critique_site_profile" }),
        messages: vec![Message {
            role: "user",
            content: &user_msg,
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building anthropic critique client")?;

    let resp = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .await
        .context("anthropic critique request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("anthropic critique {status}: {}", truncate(&body, 240));
    }

    let response_body: MessagesResponse = resp
        .json()
        .await
        .context("decoding anthropic critique response")?;
    let usage = response_body.usage.clone().unwrap_or_default();

    let tool_input = response_body
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::ToolUse { name, input } if name == "critique_site_profile" => {
                Some(input.clone())
            }
            _ => None,
        })
        .ok_or_else(|| anyhow!("critique returned no tool_use"))?;

    // The tool_use payload has shape { verdict: "ok"|"fix", reasons: "...", selectors: SelectorSet? }
    let verdict = tool_input
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("ok");
    if verdict == "ok" {
        tracing::info!(host, "critique verdict: ok");
        return Ok(None);
    }

    let reasons = tool_input
        .get("reasons")
        .and_then(|v| v.as_str())
        .unwrap_or("(no reasons)");
    tracing::info!(host, %reasons, "critique requested selector fixes");

    let sel_value = tool_input
        .get("selectors")
        .cloned()
        .ok_or_else(|| anyhow!("critique verdict=fix but no selectors returned"))?;
    let selectors: SelectorSet = serde_json::from_value(sel_value)
        .context("parsing critique selectors as SelectorSet")?;

    Ok(Some(LearnedProfile { selectors, usage }))
}

fn critique_tool_definition() -> Value {
    json!({
        "name": "critique_site_profile",
        "description": "You'll be shown the CSS selectors you previously generated for a forum thread page AND a JSON sample of the posts they actually extracted. Decide whether the extraction looks right. Common failure modes to watch for: (a) author/body containing JavaScript or boilerplate that bled in from siblings; (b) the same value appearing for multiple distinct sidebar fields (post_count, join_date, location all = 'Senior Member' means they're all hitting the rank element); (c) empty fields where you should have populated them; (d) selectors that match too many elements per post (>1) leading to duplicated text. If the output looks correct, set verdict='ok' and omit `selectors`. If there's a problem, set verdict='fix', describe in `reasons`, and return a corrected full SelectorSet.",
        "input_schema": {
            "type": "object",
            "required": ["verdict"],
            "properties": {
                "verdict": {
                    "type": "string",
                    "enum": ["ok", "fix"],
                    "description": "Is the extracted sample acceptable as-is?"
                },
                "reasons": {
                    "type": "string",
                    "description": "Plain-English explanation of what looks wrong (or 'looks good')."
                },
                "selectors": {
                    "type": "object",
                    "description": "If verdict='fix', the corrected SelectorSet — SAME shape as derive_site_profile's output, INCLUDING content_type and content_probe. Only present when verdict='fix'.",
                    "properties": {
                        "content_type": {"type": "string"},
                        "content_probe": {"type": "string"},
                        "traversal_mode": {"type": "string", "enum": ["linear", "threaded_replies"]},
                        "post_selector": {"type": "string"},
                        "op_selector": {"type": "string"},
                        "author_selector": {"type": "string"},
                        "author_url_selector": {"type": "string"},
                        "avatar_selector": {"type": "string"},
                        "author_rank_selector": {"type": "string"},
                        "author_post_count_selector": {"type": "string"},
                        "author_join_date_selector": {"type": "string"},
                        "author_location_selector": {"type": "string"},
                        "timestamp_selector": {"type": "string"},
                        "like_count_selector": {"type": "string"},
                        "body_selector": {"type": "string"},
                        "op_body_selector": {"type": "string"},
                        "signature_selector": {"type": "string"},
                        "post_number_selector": {"type": "string"},
                        "quote_block_selector": {"type": "string"},
                        "child_comment_selector": {"type": "string"},
                        "child_comment_author_selector": {"type": "string"},
                        "child_comment_body_selector": {"type": "string"},
                        "pagination_next": {"type": "string"},
                        "media_expand_selectors": {"type": "array", "items": {"type": "string"}},
                        "extra_media_selectors": {"type": "array", "items": {"type": "string"}},
                        "load_more_strategy": {"type": "string"},
                        "load_more_selector": {"type": "string"},
                        "author_profile_url_template": {"type": "string"},
                        "author_avatar_format": {"type": "string"},
                        "author_avatar_locator": {"type": "string"},
                        "author_post_count_locator": {"type": "string"},
                        "author_karma_locator": {"type": "string"},
                        "author_join_date_locator": {"type": "string"},
                        "author_last_active_locator": {"type": "string"},
                        "author_location_locator": {"type": "string"},
                        "author_rank_locator": {"type": "string"},
                        "platform_guess": {"type": "string"},
                        "notes": {"type": "string"}
                    }
                }
            }
        }
    })
}

fn build_critique_prompt(
    host: &str,
    url: &str,
    current_selectors: &str,
    extracted_sample_json: &str,
    dom_skeleton: &str,
) -> String {
    format!(
        "You generated a SelectorSet for the forum host **{host}** (URL {url}). Below is the SelectorSet, then a sample of what those selectors extracted when applied to the live page, then the minified DOM skeleton.

Your job: look at the extracted sample. Does it look like real forum-thread post data — distinct usernames with no surrounding noise, distinct sidebar fields (post count, join date, location are all DIFFERENT values per author), non-empty body for each post, no JavaScript or template strings leaking through, no post duplicated? If yes, return verdict='ok'. If something is off, return verdict='fix' with a corrected full SelectorSet and a short reason.

CURRENT SELECTORS:
```json
{current_selectors}
```

EXTRACTED SAMPLE (first few posts, JSON):
```json
{extracted_sample_json}
```

DOM SKELETON:
{dom_skeleton}
"
    )
}

/// Refine an existing SelectorSet using the user's complaint about a specific
/// element. Called from debugger mode after the user clicks a wrong-looking
/// piece of the native render and types what's off.
///
/// Arguments:
///   - `host`: the registrable domain the profile is for.
///   - `current_selectors`: the active SelectorSet as JSON.
///   - `element_html`: outerHTML of the element the user clicked (a few KB max).
///   - `field_name`: which Post field the user was pointing at (`author`,
///     `body`, `timestamp`, etc.) — gives the LLM a concrete target.
///   - `complaint`: free-text explanation of what's wrong.
pub async fn refine(
    host: &str,
    current_selectors: &str,
    element_html: &str,
    field_name: &str,
    complaint: &str,
) -> Result<LearnedProfile> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow!("ANTHROPIC_API_KEY not set"))?;

    let tool = tool_definition();
    let user_msg = build_refine_prompt(host, current_selectors, element_html, field_name, complaint);

    let req = MessagesRequest {
        model: MODEL,
        max_tokens: MAX_TOKENS,
        tools: vec![tool],
        tool_choice: json!({ "type": "tool", "name": TOOL_NAME }),
        messages: vec![Message {
            role: "user",
            content: &user_msg,
        }],
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("building anthropic http client")?;

    let resp = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .await
        .context("anthropic refine request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("anthropic refine {status}: {}", truncate(&body, 240));
    }

    let response_body: MessagesResponse =
        resp.json().await.context("decoding anthropic refine response")?;
    let usage = response_body.usage.clone().unwrap_or_default();

    let tool_input = response_body
        .content
        .iter()
        .find_map(|b| match b {
            ContentBlock::ToolUse { name, input } if name == TOOL_NAME => Some(input.clone()),
            _ => None,
        })
        .ok_or_else(|| anyhow!("anthropic refine returned no tool_use"))?;

    let selectors: SelectorSet = serde_json::from_value(tool_input)
        .context("parsing refine tool_use input as SelectorSet")?;

    Ok(LearnedProfile { selectors, usage })
}

fn build_refine_prompt(
    host: &str,
    current_selectors: &str,
    element_html: &str,
    field_name: &str,
    complaint: &str,
) -> String {
    format!(
        "The user is using a vBulletin-style native render of a forum thread, and reports a problem with the **{field_name}** field. Your previous SelectorSet for host **{host}** was:

```json
{current_selectors}
```

The user clicked this element in the native render — it's what they saw rendered as the **{field_name}**:

```html
{element_html}
```

Their complaint: \"{complaint}\"

Return a NEW SelectorSet that fixes the {field_name} field while keeping every other selector that's still correct unchanged. Use the same tool schema. If the complaint also implicates other fields, fix those too. Do NOT clear selectors that aren't related to the complaint."
    )
}

fn build_user_prompt(
    host: &str,
    url: &str,
    dom: &str,
    hint: Option<&str>,
    existing_content_types: &[String],
) -> String {
    let existing = if existing_content_types.is_empty() {
        "(none — this is the first page we've seen on this host)".to_string()
    } else {
        existing_content_types.join(", ")
    };
    let guide = platform_guide(hint);
    format!(
        "You are analyzing a minified DOM skeleton from a forum-thread page. Identify CSS selectors that locate each post and its sub-fields, AND identify this page's content_type with a unique content_probe selector. The selectors will be applied to similar pages on future visits to this host; the probe is the cache key that lets us skip future API calls.

Host: {host}
URL: {url}
Platform hint (from feed config — may be 'unknown' or unreliable): {hint_str}
Existing content_types already cached for this host: {existing}

If THIS page matches one of those existing tags, re-use the SAME tag and fix the probe so future similar pages hit the cache. Otherwise invent a new distinct kebab-case tag.

{guide}

DOM SKELETON FOLLOWS — text nodes are truncated, scripts/styles/most attributes stripped:
{dom}
",
        hint_str = hint.unwrap_or("unknown")
    )
}

/// Platform-specific "field guide" injected into the user prompt. Each guide
/// gives the model strong priors about a platform's DOM conventions, common
/// pitfalls, and profile endpoints — WITHOUT us writing platform-specific
/// extraction code. The agent still derives the actual selectors from the
/// rendered DOM; this is reference material that helps it not miss things.
fn platform_guide(hint: Option<&str>) -> &'static str {
    match hint.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("reddit") => REDDIT_GUIDE,
        Some("xenforo") => XENFORO_GUIDE,
        Some("vbulletin") => VBULLETIN_GUIDE,
        Some("discourse") => DISCOURSE_GUIDE,
        Some("phpbb") => PHPBB_GUIDE,
        Some("ipb") => IPB_GUIDE,
        Some("smf") => SMF_GUIDE,
        _ => GENERIC_GUIDE,
    }
}

const REDDIT_GUIDE: &str = r#"PLATFORM FIELD GUIDE — REDDIT (old.reddit.com layout):

DOM ANATOMY:
- OP container (self-text or link post): `#siteTable > .thing.link` (`.self` for text, `.gallery-link` for gallery, etc.)
- Comments tree: `.commentarea > .sitetable.nestedlisting > .thing.comment` (top-level), nested under `.child .sitetable.listing .thing.comment` recursively
- Each `.thing` has `.entry` (author/body/votes) and `.child` (nested replies)
- Body container: `.entry .usertext-body .md`
- Author link: `.tagline > .author`
- Score: `.entry .tagline .score.unvoted` (Reddit hides this for very-new comments with "•")
- Timestamp: `.tagline time` (use the `datetime` attribute for ISO)

CRITICAL — post_selector MUST EXCLUDE THE OP:
The OP is a `.thing.link`. Comments are `.thing.comment`. If your post_selector matches `.thing` it ALSO matches the OP and the OP renders twice (once via op_selector, once via post_selector). REQUIRED: scope post_selector under `.commentarea` AND restrict to `.thing.comment`. Recommended exact value: `.commentarea .thing.comment`. This naturally matches BOTH top-level AND nested comments (which is what our ancestry-based renderer wants — it figures out the thread depth from the DOM tree). DO NOT use a direct-child selector like `.commentarea > .sitetable > .thing.comment` — that would miss nested replies.

child_comment_selector: LEAVE EMPTY. Our renderer now derives reply relationships from DOM ancestry (each comment quotes its nearest matching ancestor) so the old child_comment_selector field is unused.

CRITICAL PITFALL — SELF POSTS (body duplication):
The OP self-text post nests its body INSIDE a `.expando` div: `.thing.link.self > .entry > div > form > .usertext > .usertext-body > .md`.
So `.entry .usertext-body .md` matches the body that IS already inside `.expando`.
Do NOT include `.expando` in extra_media_selectors for self-post content types — it duplicates the body. ONLY use `.expando` in extra_media_selectors for LINK content types (image_post, video_post, gallery) where the expando contains preview media that's not in `.usertext-body`.

CONTENT TYPES TO DISTINGUISH:
- self_text     → probe like `#siteTable .thing.link.self`
- image_post    → probe like `#siteTable .thing.link[data-domain^="i."]` or `.thing.link.gallery-link`
- link_post     → probe like `#siteTable .thing.link:not(.self):not([data-domain^="i."])`
- video_post    → probe like `.thing.link[data-domain="v.redd.it"]` or `.thing.link[data-domain*="youtu"]`

CRITICAL — OP BODY vs COMMENT BODY (on link/image/video/gallery content types):
On Reddit, the OP and comments have DIFFERENT body shapes:
  - Comments ALWAYS use `.entry .usertext-body .md` (prose).
  - OPs of self-text posts also use `.entry .usertext-body .md`.
  - OPs of link/image/video/gallery posts have NO `.usertext-body` — their "body" is `.entry p.title` (the clickable title `<a class="title may-blank">` carries the article URL).

Use `body_selector` for comments (the common shape) and `op_body_selector` to override for the OP only. The extractor uses op_body_selector for whichever element op_selector matches, and body_selector for everything else.

Per content type:
  - self_text:
      body_selector = `.entry .usertext-body .md`
      op_body_selector = (leave empty — OP uses the same shape as comments)
      extra_media_selectors = [] (body already has self-text)

  - link_post:
      body_selector = `.entry .usertext-body .md`  (for comments)
      op_body_selector = `.entry p.title`  (for the link-title OP)
      extra_media_selectors = []  (no media to embed)

  - image_post:
      body_selector = `.entry .usertext-body .md`
      op_body_selector = `.entry p.title`
      extra_media_selectors = `[".expando .media-preview-content img"]`  (target the actual image element, NOT the whole .expando which also contains "Previous / Next / Back to Grid View" gallery navigation that bleeds through as bullet-list text). If a single `img` selector doesn't work for galleries, try `.expando .gallery-tile img` or similar — but ALWAYS scope down to the image, never the whole .expando container.

  - gallery (multi-image): same as image_post — target image elements only. NEVER include the gallery's navigation chrome (Previous/Next/Back to Grid View list items) — those would render as broken bullet-list nav inside the post.

  - video_post (v.redd.it):
      body_selector = `.entry .usertext-body .md`
      op_body_selector = `.entry p.title` ONLY.
      extra_media_selectors = []  — DO NOT include .expando. v.redd.it .expando is an HTML5 `<video>` player whose controls ("Replay Video", "Pause", "1080p HD", "Fullscreen") render as broken stray text once detached from Reddit's JS runtime. The user can click the title link to view natively.

MEDIA EXPANDERS: `.expando-button.collapsed` (clicking expands an .expando preview). ONLY useful on link/image/video content types. NEVER on self_text (would duplicate body).

LAZY-LOAD MORE COMMENTS: `.morecomments a` (click button strategy).
PAGINATION: there isn't classic pagination — comments use .morecomments inline. Set load_more_strategy=click_button, load_more_selector=".morecomments a", leave pagination_next empty.

LIKES/UPVOTES: like_count_selector = `.entry .tagline .score.unvoted`. Score is shown as "37 points" — our parser extracts the integer.

PROFILE ENRICHMENT (Reddit's about.json):
- author_profile_url_template = `/user/{author}/about.json`  (root-relative so the WebView fetch is same-origin)
- author_avatar_format = `json`
- author_avatar_locator = `data.icon_img`  (basic Snoo) OR `data.snoovatar_img` (custom)
- author_post_count_locator: Reddit doesn't expose a raw post count. LEAVE EMPTY rather than using karma — karma is shown in author_karma_locator.
- author_karma_locator = `data.total_karma`
- author_join_date_locator = `data.created_utc`  (Unix seconds float, e.g. 1738883427.0 — frontend formats via fmtSmartDate)
- author_last_active_locator: Reddit doesn't expose this; LEAVE EMPTY
- author_location_locator: Reddit has no location field; LEAVE EMPTY
- author_rank_locator: Reddit has no ranks; LEAVE EMPTY"#;

const XENFORO_GUIDE: &str = r#"PLATFORM FIELD GUIDE — XENFORO 2.x:

DOM ANATOMY:
- Posts: `article.message`
- Author sidebar: `.message-userDetails` containing username + `.userTitle` (rank)
- Body container: `.bbWrapper` (inline media lives here, no sibling expando needed → leave extra_media_selectors empty)
- Timestamp: `time.u-dt` (use `datetime` attribute)
- Likes/reactions: `.reactionsBar-link` or attribute `data-reactions`
- Signature: `.message-signature`

PROFILE: `/members/{author}/` returns HTML.
- author_avatar_format = `html`
- author_avatar_locator = `.avatarBg img`, `.userBanner img`, or `img.avatar`
- author_post_count_locator: look for `dl.pairs--justified` items labelled 'Messages'
- author_join_date_locator: same `dl.pairs` items labelled 'Joined'
- author_rank_locator: `.userTitle`"#;

const VBULLETIN_GUIDE: &str = r##"PLATFORM FIELD GUIDE — VBULLETIN 3/4 (classic):

DOM ANATOMY (this is a TABLE-based layout):
- Posts are typically `table[id^="post"]` — a single post is wrapped in a 2-cell table
- Left cell `td.alt2`: author sidebar (avatar, username, usertitle, join date, posts, location)
- Right cell `td.alt1`: post body
- Body container inside the right cell: `div[id^="post_message_"]`
- Post-number cell at top: `td.thead` containing "#NN"
- Timestamp text is in a sidebar div near the top of the post row

CRITICAL PITFALL — vbmenu_register SCRIPTS:
vBulletin emits `<script>vbmenu_register("postmenu_NNNNN", true);</script>` IMMEDIATELY after the username `<a>` element. A naive author_selector like `.bigusername` element-text picks up that script content too, polluting the captured author name.
To avoid this: use selectors that scope tightly to JUST the username `<a>` element, NOT its parent that might contain the script. Example: `a.bigusername` directly, not `td.alt2 > div`.

PROFILE: `/member.php?u={user_id}` (numeric) OR `/member.php?username={author}` (name-based, if site supports it).
Many vBulletin installs expose `?username={author}` — TRY THAT FIRST. Set:
- author_profile_url_template = `/member.php?username={author}`
- author_avatar_format = `html`
- author_avatar_locator = `.member_avatar img` or `img.avatar`
- author_post_count_locator: scan sidebar for `.smallfont` text starting with "Posts:" or labelled 'Posts'
- author_join_date_locator: same sidebar, look for "Join Date:" / "Joined:"
- author_location_locator: "Location:"
- author_rank_locator: `.usertitle` or the text element immediately under username

LIKES: classic vBulletin doesn't have likes per-post. Some installs add "Thanks" plugins — look for `.post_thanks_box` count. Often leave like_count_selector EMPTY."##;

const DISCOURSE_GUIDE: &str = r#"PLATFORM FIELD GUIDE — DISCOURSE:

DOM ANATOMY:
- Posts: `article[data-post-id]` or `.topic-post article`
- Body: `.cooked` (rendered Markdown HTML, includes inline media)
- Author username: `.first.username a`
- Avatar: `.main-avatar img`
- Timestamp: `.post-date a` (with title= ISO attribute, or text content)
- Likes: `[data-likes]` attribute on the post, or `.discourse-reactions-counter-anim`
- Trust level / title: `.user-card-title` or `.title`

PROFILE: `/u/{author}.json` returns JSON.
- author_avatar_format = `json`
- author_avatar_locator = `user.avatar_template` (often contains a {size} placeholder — picks first non-placeholder match if any)
- author_post_count_locator = `user.post_count`
- author_karma_locator = `user.likes_received`
- author_join_date_locator = `user.created_at`
- author_last_active_locator = `user.last_seen_at`
- author_location_locator = `user.location`
- author_rank_locator = `user.title` (or `user.trust_level` if title is empty — that returns 0-4 number)"#;

const PHPBB_GUIDE: &str = r#"PLATFORM FIELD GUIDE — PHPBB:

DOM ANATOMY:
- Posts: `div.post`
- Sidebar: `dl.postprofile`
- Username: `.username` (links to /memberlist.php?mode=viewprofile&u={id})
- Body: `.content`
- Timestamp: `.author` text content (parse Posted: prefix)
- Avatar: `dl.postprofile img.avatar`

PROFILE: `/memberlist.php?mode=viewprofile&u={user_id}` — phpBB uses NUMERIC user IDs, not usernames, in URLs.
This makes author_profile_url_template hard to construct from just the author name. If you can extract the numeric id from the author's link in the thread (href like `?u=42`), you'd need to use that. Otherwise LEAVE author_profile_url_template EMPTY for phpBB.

LIKES: phpBB doesn't have native likes. Leave like_count_selector EMPTY."#;

const IPB_GUIDE: &str = r#"PLATFORM FIELD GUIDE — INVISION POWER BOARD (IPB):

DOM ANATOMY:
- Posts: `article.cPost` or `[data-controller='core.front.core.post']`
- Author panel: `.cAuthorPane`
- Username: `.cAuthorPane_author`
- Body: `[data-role='commentContent']`
- Avatar: `.cAuthorPane_photo img`
- Likes: `.cReact_listToggle` count badge

PROFILE: `/profile/{user_id}-{author}/` — uses an id-prefixed slug. Hard to construct without id. If author links in the thread have a clean slug, use it; otherwise leave profile URL empty."#;

const SMF_GUIDE: &str = r#"PLATFORM FIELD GUIDE — SIMPLE MACHINES FORUM (SMF):

DOM ANATOMY:
- Posts: usually a `div.windowbg` or `div.post_wrapper`
- Sidebar: `.poster` block
- Username: `.poster h4 a`
- Body: `.inner` or `.post`
- Timestamp: `.smalltext` near the post header

PROFILE: `/index.php?action=profile;u={user_id}` — numeric id again. Same challenge as phpBB."#;

const GENERIC_GUIDE: &str = r#"PLATFORM FIELD GUIDE — UNKNOWN PLATFORM:

No platform-specific guidance is available. Inspect the DOM carefully and apply general forum-thread heuristics:
- Look for repeated structures with classes/attributes hinting at "post", "message", "comment"
- Author is usually in a sidebar or header with class containing "user", "author", "poster"
- Body is the largest text-containing region inside each post
- Timestamps often live in a `<time>` element with a `datetime` attribute
- Profile URL: scan for hrefs like /user/, /member/, /u/, /profile/ near the username and infer the template
- If you can't identify any of these reliably, leave optional fields empty rather than guessing"#;

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
