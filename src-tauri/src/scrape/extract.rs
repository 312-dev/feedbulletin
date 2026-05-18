// Apply a cached SelectorSet to a rendered DOM to produce Vec<Post>.
//
// Two traversal modes:
//   - Linear (forums): every `post_selector` match is a Post in DOM order.
//   - ThreadedReplies (Reddit): `op_selector` produces the first Post (the OP).
//     Then each `post_selector` match (top-level comment) is a Post, with the
//     first descendant matching `child_comment_selector` (the top reply) folded
//     into the parent's `body_html` as a `<blockquote class="vb-reply">`.
//     Deeper replies are dropped — user wants the top response, not the tree.

use crate::scrape::{Post, SelectorSet, TraversalMode};
use anyhow::{anyhow, Context, Result};
use scraper::{ElementRef, Html, Selector};

/// Returns true if the given CSS selector matches at least one element in the
/// document. Used by the dispatcher to test whether a cached profile's
/// `content_probe` applies to the page in front of us.
pub fn probe_matches(html: &str, selector_raw: &str) -> bool {
    let doc = Html::parse_document(html);
    let Ok(sel) = Selector::parse(selector_raw) else {
        return false;
    };
    doc.select(&sel).next().is_some()
}

pub fn apply(html: &str, selectors: &SelectorSet, base_url: Option<&str>) -> Result<Vec<Post>> {
    let doc = Html::parse_document(html);
    let post_sel = parse_selector(&selectors.post_selector, "post_selector")?;

    let mut out = Vec::new();

    // op_body_selector lets the agent declare a different body shape for the
    // OP vs. comments — necessary on Reddit link/image/video where the OP
    // body is `.entry p.title` but comments use `.entry .usertext-body .md`.
    let mut op_selectors_owned: Option<SelectorSet> = None;
    if let Some(op_body) = selectors.op_body_selector.as_deref() {
        if !op_body.is_empty() {
            let mut s = selectors.clone();
            s.body_selector = op_body.to_string();
            op_selectors_owned = Some(s);
        }
    }
    let op_sels = op_selectors_owned.as_ref().unwrap_or(selectors);

    if matches!(selectors.traversal_mode, TraversalMode::ThreadedReplies) {
        // Threaded comments: iterate ALL matches of post_sel in document
        // order. Each comment quotes its nearest comment-ancestor (or the
        // OP if it's a true top-level comment). Because ancestors appear
        // before descendants in document order, we always have the parent's
        // info ready by the time we process a child — no recursion needed
        // and no risk of double-processing (each DOM node matches post_sel
        // at most once).
        use std::collections::{HashMap, HashSet};
        let mut post_num: u32 = 0;
        let mut info_by_node: HashMap<_, (u32, String, String)> = HashMap::new();
        // Nodes we've already rendered (OP, primarily) — skip them if they
        // also happen to match post_sel. Common on Reddit where `.thing`
        // post_selectors match BOTH the OP `.thing.link` and `.thing.comment`s.
        let mut skip_nodes: HashSet<_> = HashSet::new();

        if let Some(op_raw) = selectors.op_selector.as_deref() {
            let op_sel = parse_selector(op_raw, "op_selector")?;
            if let Some(op_el) = doc.select(&op_sel).next() {
                post_num += 1;
                // OP uses op_body_selector when set (different DOM shape than
                // comments — most relevant for Reddit link/image/video).
                let mut op_post = extract_one(op_el, op_sels, base_url);
                op_post.post_number = Some(post_num);
                skip_nodes.insert(op_el.id());
                for desc in op_el.select(&post_sel) {
                    skip_nodes.insert(desc.id());
                }
                out.push(op_post);
            }
        }

        for el in doc.select(&post_sel) {
            if skip_nodes.contains(&el.id()) {
                continue;
            }
            // Find the nearest ANCESTOR that also matched post_sel. That's
            // this comment's direct parent in the threaded view. If none,
            // this is a top-level reply to the OP — render it without a
            // quote box (no "Originally Posted by OP" preamble), since the
            // OP is right above and the relationship is obvious.
            let parent_info: Option<(u32, String, String)> = {
                let mut cur = el.parent();
                let mut found: Option<(u32, String, String)> = None;
                while let Some(node) = cur {
                    if let Some(parent_el) = ElementRef::wrap(node) {
                        if post_sel.matches(&parent_el) {
                            if let Some(info) = info_by_node.get(&parent_el.id()) {
                                found = Some(info.clone());
                                break;
                            }
                        }
                    }
                    cur = node.parent();
                }
                found
            };

            post_num += 1;
            let my_num = post_num;
            let mut post = extract_one(el, selectors, base_url);
            post.post_number = Some(my_num);

            if let Some((parent_num, parent_author, parent_body)) = parent_info {
                post.quote_of_post_number = Some(parent_num);
                let quote = render_vb_quote_preserving_nested(
                    &parent_author,
                    &parent_body,
                    parent_num,
                );
                post.body_html = format!("{}{}", quote, post.body_html);
            }

            info_by_node.insert(
                el.id(),
                (my_num, post.author.clone(), post.body_html.clone()),
            );
            out.push(post);
        }
    } else {
        // Some forum themes (notably BimmerPost / vB4 with the "first-post
        // highlight" widget) place the same `<table id="postNNNN">` in TWO
        // separate DOM containers — once in a "highlighted first post" box
        // at the top, and again in the main post list below. `post_sel`
        // matches both; without dedup we render the same post twice with
        // the same post_number, which looks alarming. We dedupe by:
        //   1. The post_number read out of the DOM (when present — vBulletin
        //      themes embed it as a stable per-post identifier).
        //   2. Author + first-N-chars-of-body fingerprint (fallback for
        //      sources that don't expose a post number).
        // Once a key has been seen, skip subsequent matches entirely.
        use std::collections::HashSet;
        let mut seen_numbers: HashSet<u32> = HashSet::new();
        let mut seen_fingerprints: HashSet<String> = HashSet::new();
        let mut idx: u32 = 0;
        for el in doc.select(&post_sel) {
            let mut post = extract_one(el, selectors, base_url);

            if let Some(n) = post.post_number {
                if !seen_numbers.insert(n) {
                    continue;
                }
            } else {
                let fp = format!(
                    "{}|{}",
                    post.author,
                    post.body_html.chars().take(120).collect::<String>(),
                );
                if !seen_fingerprints.insert(fp) {
                    continue;
                }
            }

            if post.post_number.is_none() {
                idx += 1;
                post.post_number = Some(idx);
            }
            out.push(post);
        }
    }

    Ok(out)
}

fn extract_one(el: ElementRef<'_>, s: &SelectorSet, base_url: Option<&str>) -> Post {
    let author = s
        .author_selector
        .as_str()
        .pipe(|sel| select_one_text(el, sel))
        .unwrap_or_default();

    let author_url = s
        .author_url_selector
        .as_deref()
        .and_then(|sel| select_one_attr(el, sel, "href"))
        .map(|u| absolutize(&u, base_url));

    let avatar_url = s
        .avatar_selector
        .as_deref()
        .and_then(|sel| select_one_attr(el, sel, "src"))
        .map(|u| absolutize(&u, base_url));

    let author_rank = s
        .author_rank_selector
        .as_deref()
        .and_then(|sel| select_one_text(el, sel));
    let author_post_count = s
        .author_post_count_selector
        .as_deref()
        .and_then(|sel| select_one_text(el, sel));
    let author_join_date = s
        .author_join_date_selector
        .as_deref()
        .and_then(|sel| select_one_text(el, sel));
    let author_location = s
        .author_location_selector
        .as_deref()
        .and_then(|sel| select_one_text(el, sel));

    let timestamp = s
        .timestamp_selector
        .as_deref()
        .and_then(|sel| {
            // prefer datetime attribute (Discourse, HTML5 <time>), fall back to text
            select_one_attr(el, sel, "datetime").or_else(|| select_one_text(el, sel))
        });

    // Like count: when selector is set the site has a like/upvote system. We
    // always emit Some(n) (even 0 / negative) so the renderer knows to show
    // the footer. None means "no like system on this site" → no footer.
    let like_count = s.like_count_selector.as_deref().map(|sel| {
        select_one_text(el, sel)
            .and_then(|t| parse_like_count(&t))
            .map(|n| n.max(0))
            .unwrap_or(0)
    });

    // Resolve the body element once so we can both extract its inner_html
    // AND check overlap against extra_media_selectors below.
    let body_el = Selector::parse(&s.body_selector)
        .ok()
        .and_then(|sel| el.select(&sel).next());
    let mut body_html = body_el
        .map(|b| sanitize_body(&b.inner_html(), base_url))
        .unwrap_or_default();

    // Append any sibling/cousin media containers the agent flagged (e.g.
    // old.reddit `.expando` is outside `.usertext-body` on link posts).
    // SKIP when the matched extra-media element overlaps with the body
    // element — common gotcha on Reddit SELF posts where `.expando` is a
    // descendant of the same `.entry` that holds the body, and would
    // duplicate the post content on render.
    for extra_sel_raw in &s.extra_media_selectors {
        let Ok(extra_sel) = Selector::parse(extra_sel_raw) else { continue };
        let Some(extra_el) = el.select(&extra_sel).next() else { continue };

        if let Some(b) = body_el {
            if elements_overlap(b, extra_el) {
                continue;
            }
        }

        // Strip <video> subtrees first — their control chrome ("Replay
        // Video", "1080p HD", "Fullscreen") survives ammonia as bare text
        // and the underlying media source is usually JS-driven or
        // cross-origin restricted. Drop the whole subtree; user clicks
        // through to the original site to watch.
        let pre = strip_unplayable_media(&extra_el.inner_html());
        let frag = sanitize_body(&pre, base_url);
        if !frag.trim().is_empty() {
            body_html.push_str(r#"<div class="vb-extra-media">"#);
            body_html.push_str(&frag);
            body_html.push_str("</div>");
        }
    }

    let signature_html = s
        .signature_selector
        .as_deref()
        .map(|sel| select_one_html(el, sel, base_url))
        .filter(|h| !h.trim().is_empty());

    let post_number = s
        .post_number_selector
        .as_deref()
        .and_then(|sel| select_one_text(el, sel))
        .and_then(|s| parse_post_number(&s));

    let quote_of_post_number = s
        .quote_block_selector
        .as_deref()
        .and_then(|sel| select_one_attr(el, sel, "data-source"))
        .and_then(|s| parse_post_number(&s));

    Post {
        post_number,
        author,
        author_url,
        avatar_url,
        author_rank,
        author_post_count,
        author_join_date,
        author_location,
        author_karma: None,
        author_last_active: None,
        timestamp,
        body_html,
        signature_html,
        like_count,
        quote_of_post_number,
    }
}

/// Parse a like/upvote count from arbitrary text. Handles:
///   - "37 points"               → 37   (Reddit format with trailing unit)
///   - "1,234"                   → 1234
///   - "1.2k" / "1.2K likes"     → 1200
///   - "3M views"                → 3_000_000
///   - "-5"                      → -5   (caller clamps to 0)
///   - "•" / "score hidden"      → None (caller treats as 0)
///   - "" / no number            → None
fn parse_like_count(raw: &str) -> Option<i64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_lowercase();
    if t == "•" || lower.contains("hidden") {
        return None;
    }

    // Walk the text, find the first numeric token. Accepts optional leading
    // sign, digits, comma-thousands-grouping, decimal point, then optional
    // k/m multiplier suffix. Anything else (units like "points") stops the
    // number scan.
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !is_number_lead(bytes[i] as char, i, bytes) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i;
    let mut number = String::new();
    if bytes[i] as char == '-' || bytes[i] as char == '+' {
        number.push(bytes[i] as char);
        i += 1;
    }
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
            i += 1;
        } else if c == ',' {
            // Thousands separator — skip without adding.
            i += 1;
        } else {
            break;
        }
    }
    if number.is_empty() || number == "-" || number == "+" {
        return None;
    }
    let _ = start;

    // Optional suffix: skip whitespace, then look for k or m as a multiplier.
    let mut multiplier = 1.0_f64;
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    if i < bytes.len() {
        let c = (bytes[i] as char).to_ascii_lowercase();
        if c == 'k' {
            multiplier = 1_000.0;
        } else if c == 'm' {
            multiplier = 1_000_000.0;
        }
    }

    let n: f64 = number.parse().ok()?;
    Some((n * multiplier).round() as i64)
}

fn is_number_lead(c: char, idx: usize, bytes: &[u8]) -> bool {
    if c.is_ascii_digit() {
        return true;
    }
    if c == '-' || c == '+' {
        // Only treat sign as a number lead when followed immediately by a digit.
        return bytes
            .get(idx + 1)
            .map(|b| (*b as char).is_ascii_digit())
            .unwrap_or(false);
    }
    false
}

/// Render the quote-box for a reply, preserving any leading vb-quote
/// blockquote from the parent's body so the conversation chain stays
/// visible. If the parent itself quoted its parent, that nested quote
/// is carried forward inside the new outer quote — classic vBulletin
/// nested-quote-chain rendering.
fn render_vb_quote_preserving_nested(
    parent_author: &str,
    parent_body_html: &str,
    parent_post_number: u32,
) -> String {
    let cite = if parent_author.trim().is_empty() {
        format!("<cite>Quote (post #{}):</cite>", parent_post_number)
    } else {
        format!(
            r##"<cite>Originally Posted by <a href="#post-{}">{}</a></cite>"##,
            parent_post_number,
            html_escape(parent_author),
        )
    };
    let (nested, rest) = split_leading_vb_quote(parent_body_html);
    let plain = plain_text_excerpt(&rest, 220);
    let inner = if !nested.is_empty() {
        format!("{}{}<p>{}</p>", cite, nested, html_escape(&plain))
    } else {
        format!("{}<p>{}</p>", cite, html_escape(&plain))
    };
    format!(r#"<blockquote class="vb-quote">{}</blockquote>"#, inner)
}

/// Extract any leading `<blockquote class="vb-quote">...</blockquote>` from
/// `html`, returning (nested_blockquote_html, rest). Uses a depth counter so
/// it correctly handles already-nested quotes (a top-level vb-quote that
/// itself contains a vb-quote inside).
fn split_leading_vb_quote(html: &str) -> (String, String) {
    let trimmed = html.trim_start();
    const MARKER: &str = r#"<blockquote class="vb-quote">"#;
    if !trimmed.starts_with(MARKER) {
        return (String::new(), html.to_string());
    }
    // WHY: byte-level scan to avoid UTF-8 boundary panics. The HTML can
    // contain multibyte chars (typographic apostrophes, em-dashes) that
    // straddle our 11/13-byte windows. We only ever compare against ASCII
    // tag literals, so a byte-array slice is safe and char-boundary-immune.
    let bytes = trimmed.as_bytes();
    let len = bytes.len();
    const OPEN: &[u8] = b"<blockquote";
    const CLOSE: &[u8] = b"</blockquote>";
    let mut i = MARKER.len();
    let mut depth = 1usize;
    while i < len {
        if i + OPEN.len() <= len && &bytes[i..i + OPEN.len()] == OPEN {
            depth += 1;
            i += OPEN.len();
        } else if i + CLOSE.len() <= len && &bytes[i..i + CLOSE.len()] == CLOSE {
            depth -= 1;
            i += CLOSE.len();
            if depth == 0 {
                // i is now on a char boundary (just past `>`, which is ASCII).
                return (trimmed[..i].to_string(), trimmed[i..].to_string());
            }
        } else {
            i += 1;
        }
    }
    (String::new(), html.to_string())
}

/// Strip HTML tags from a fragment and return a whitespace-collapsed excerpt
/// truncated to `max` characters. Used to build the quote-box body when
/// referencing the parent post — we want short, plain context, not nested
/// formatting / images / inception quotes.
fn plain_text_excerpt(html: &str, max: usize) -> String {
    let frag = Html::parse_fragment(html);
    let text: String = frag.root_element().text().collect::<Vec<_>>().join(" ");
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(max).collect();
        format!("{}…", truncated.trim_end())
    }
}

fn select_one_text(root: ElementRef<'_>, sel_raw: &str) -> Option<String> {
    let sel = Selector::parse(sel_raw).ok()?;
    let el = root.select(&sel).next()?;
    let text: String = el.text().collect::<Vec<_>>().join(" ").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn select_one_attr(root: ElementRef<'_>, sel_raw: &str, attr: &str) -> Option<String> {
    let sel = Selector::parse(sel_raw).ok()?;
    let el = root.select(&sel).next()?;
    el.value().attr(attr).map(|s| s.to_string())
}

fn select_one_html(root: ElementRef<'_>, sel_raw: &str, base_url: Option<&str>) -> String {
    let Ok(sel) = Selector::parse(sel_raw) else {
        return String::new();
    };
    let Some(el) = root.select(&sel).next() else {
        return String::new();
    };
    sanitize_body(&el.inner_html(), base_url)
}

/// Pre-sanitization scrub for fragments destined for extra_media_selectors.
/// HTML5 `<video>` players have a lot of nested control/text chrome
/// (`Replay Video`, `Pause`, quality labels, `Fullscreen`) that survives
/// ammonia and renders as broken stray text once stripped of its JS.
/// On most cross-origin sites the actual <source> URL is also blocked or
/// requires the host's runtime to function. Easier to drop the whole
/// `<video>` subtree — the user can click the title link to view it
/// natively.
fn strip_unplayable_media(html: &str) -> String {
    // Cheap regex-free, char-boundary-safe `<video>...</video>` stripper.
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 6 <= bytes.len() && bytes[i..i + 6].eq_ignore_ascii_case(b"<video") {
            // Find matching </video> with depth tracking (videos don't nest
            // in real DOM, but be robust). Falls through to char-by-char
            // copy if no close tag found.
            let close = find_close_tag(bytes, i + 6, b"video");
            if let Some(end) = close {
                i = end;
                continue;
            }
        }
        // Append the next full UTF-8 char and advance i past its byte length.
        let ch_end = next_char_boundary(html, i);
        out.push_str(&html[i..ch_end]);
        i = ch_end;
    }
    out
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    if i >= bytes.len() { return bytes.len(); }
    i += 1;
    while !s.is_char_boundary(i) && i < bytes.len() { i += 1; }
    i
}

fn find_close_tag(bytes: &[u8], from: usize, tag: &[u8]) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if i + 2 + tag.len() <= bytes.len()
            && bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2..i + 2 + tag.len()].eq_ignore_ascii_case(tag)
        {
            // skip until '>'
            let mut j = i + 2 + tag.len();
            while j < bytes.len() && bytes[j] != b'>' { j += 1; }
            if j < bytes.len() { return Some(j + 1); }
            return Some(bytes.len());
        }
        i += 1;
    }
    None
}

/// Ammonia allow-list tuned for forum content. Preserves:
///   - blockquote (vital for quote-replies + Reddit depth-1 reply rendering)
///   - code/pre/lists/tables (normal forum prose)
///   - img / video / source / audio (gallery + embedded media posts)
///   - iframe ONLY when its src host is in the allowlist (YouTube, Vimeo,
///     Reddit's video player, Imgur embeds, etc.) — protects against ad
///     networks injecting cross-origin iframes during scrape.
fn sanitize_body(html: &str, base_url: Option<&str>) -> String {
    let mut builder = ammonia::Builder::default();
    builder
        .tags(std::collections::HashSet::from([
            "p", "br", "hr", "strong", "em", "b", "i", "u", "s", "sub", "sup",
            "blockquote", "cite", "q", "code", "pre", "kbd", "samp",
            "ul", "ol", "li",
            "h1", "h2", "h3", "h4", "h5", "h6",
            "a", "img", "span", "div", "small",
            "table", "thead", "tbody", "tr", "th", "td", "caption",
            "figure", "figcaption",
            "video", "source", "audio", "iframe", "picture",
        ]))
        .add_tag_attributes("a", &["href", "title"])
        // data-src / data-lazy-src / data-original / data-href: common lazy-load
        // attributes used by forums and image hosts. We keep them through the
        // sanitize step; NativeThreadView promotes them to `src` after mount.
        .add_tag_attributes("img", &[
            "src", "srcset", "alt", "title", "width", "height", "loading",
            "data-src", "data-lazy-src", "data-original", "data-href",
        ])
        .add_tag_attributes("video", &["src", "poster", "controls", "width", "height", "muted", "loop", "playsinline", "preload"])
        .add_tag_attributes("source", &["src", "srcset", "type", "media"])
        .add_tag_attributes("audio", &["src", "controls", "preload"])
        .add_tag_attributes("iframe", &["src", "width", "height", "allowfullscreen", "frameborder", "title"])
        .add_tag_attributes("picture", &["class"])
        .add_tag_attributes("blockquote", &["cite", "class"])
        .add_tag_attributes("code", &["class"])
        .add_tag_attributes("pre", &["class"])
        .add_tag_attributes("span", &["class"])
        .add_tag_attributes("div", &["class"])
        .add_tag_attributes("figure", &["class"])
        // WHY: iframe is dangerous by default; only permit trusted embed hosts.
        // `attribute_filter` is element-aware: we drop the src (and thus
        // neutralize the iframe) when its host isn't on the allowlist.
        .attribute_filter(|tag, attr, value| {
            if tag == "iframe" && attr == "src" && !is_allowed_iframe_src(value) {
                None
            } else {
                Some(std::borrow::Cow::Borrowed(value))
            }
        });

    if let Some(b) = base_url.and_then(|u| url::Url::parse(u).ok()) {
        builder.url_relative(ammonia::UrlRelative::RewriteWithBase(b));
    }
    let cleaned = builder.clean(html).to_string();
    // Force lazy-loading on every <img> that doesn't already specify it.
    // Long threads with 50+ inline images would otherwise eagerly fetch
    // them all on render — for a 200-post BimmerPost thread that's tens
    // of MB and noticeable jank. `loading="lazy"` is opt-in browser
    // behavior and harmless if the renderer doesn't honor it.
    inject_lazy_loading(&cleaned)
}

/// Add `loading="lazy"` to every `<img` tag that doesn't already specify
/// a loading attribute. Byte-level scan to avoid pulling in another DOM
/// parse — the sanitizer just emitted this HTML so we trust its shape.
fn inject_lazy_loading(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let bytes = html.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<img" {
            // Locate the matching `>` (or `/>`), respecting attribute quoting.
            let mut j = i + 4;
            let mut in_str: Option<u8> = None;
            while j < bytes.len() {
                let c = bytes[j];
                match in_str {
                    Some(q) if c == q => in_str = None,
                    None if c == b'"' || c == b'\'' => in_str = Some(c),
                    None if c == b'>' => break,
                    _ => {}
                }
                j += 1;
            }
            if j >= bytes.len() {
                // Malformed; bail and copy the rest verbatim.
                out.push_str(&html[i..]);
                return out;
            }
            let tag = &html[i..=j];
            if tag.contains(" loading=") || tag.contains("\tloading=") {
                out.push_str(tag);
            } else {
                // Insert before the closing `>` (or `/>`).
                let insert_at = if tag.ends_with("/>") {
                    tag.len() - 2
                } else {
                    tag.len() - 1
                };
                out.push_str(&tag[..insert_at]);
                out.push_str(" loading=\"lazy\"");
                out.push_str(&tag[insert_at..]);
            }
            i = j + 1;
        } else {
            // Copy byte, but advance by char-boundary so UTF-8 stays intact.
            let ch_len = utf8_char_len(bytes[i]);
            out.push_str(&html[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xC0 { 1 }  // continuation byte — shouldn't appear here, treat as 1 to advance
    else if b < 0xE0 { 2 }
    else if b < 0xF0 { 3 }
    else { 4 }
}

/// Best-effort: returns true if the URL host is one of the embed providers we
/// trust to be iframe-safe. Conservative — we'd rather drop an unfamiliar
/// embed than render an ad/tracker iframe.
fn is_allowed_iframe_src(url: &str) -> bool {
    let host = match url::Url::parse(url) {
        Ok(u) => u.host_str().unwrap_or("").to_ascii_lowercase(),
        Err(_) => return false,
    };
    // Common forum-embed providers. We're conservative — the goal is to block
    // ad/tracker iframes that scrape pages occasionally carry while preserving
    // the kind of embedded content forum users actually post.
    let allow = [
        // video
        "youtube.com", "youtube-nocookie.com", "youtu.be",
        "vimeo.com",
        "twitch.tv", "clips.twitch.tv",
        "dailymotion.com",
        "streamable.com",
        "v.redd.it", "redditmedia.com",
        // images / gifs
        "imgur.com", "i.imgur.com",
        "gfycat.com", "redgifs.com",
        "i.stack.imgur.com",
        // social posts
        "twitter.com", "x.com", "platform.twitter.com",
        "instagram.com",
        "tiktok.com",
        "facebook.com",
        "reddit.com",
        // audio
        "soundcloud.com", "w.soundcloud.com",
        "bandcamp.com",
        "spotify.com", "open.spotify.com",
        // code & doc
        "codepen.io", "jsfiddle.net",
        "gist.github.com",
        // maps
        "google.com/maps", "maps.google.com",
        "openstreetmap.org",
    ];
    allow
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")) || host == h.trim_start_matches("www."))
}

fn parse_selector(raw: &str, label: &str) -> Result<Selector> {
    Selector::parse(raw).map_err(|e| anyhow!("invalid {label}: {e:?}"))
}

/// True when `a` and `b` are the same DOM node, OR one is an ancestor of the
/// other. Used to detect when an extra_media_selector overlaps with the
/// body_selector — appending an overlapping fragment would duplicate the
/// post text in the rendered view.
fn elements_overlap(a: ElementRef<'_>, b: ElementRef<'_>) -> bool {
    if a.id() == b.id() {
        return true;
    }
    fn is_ancestor(ancestor: ElementRef<'_>, descendant: ElementRef<'_>) -> bool {
        let target = ancestor.id();
        let mut cur = descendant.parent();
        while let Some(n) = cur {
            if n.id() == target {
                return true;
            }
            cur = n.parent();
        }
        false
    }
    is_ancestor(a, b) || is_ancestor(b, a)
}

fn absolutize(href: &str, base: Option<&str>) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Some(b) = base.and_then(|b| url::Url::parse(b).ok()) {
        if let Ok(joined) = b.join(href) {
            return joined.to_string();
        }
    }
    href.to_string()
}

fn parse_post_number(s: &str) -> Option<u32> {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrape::{SelectorSet, TraversalMode};

    fn xenforo_like_html() -> &'static str {
        r##"<!DOCTYPE html><html><body>
        <article class="message" data-author="alice">
          <div class="message-userDetails">
            <a class="username" href="/members/alice.42/"><span class="username-inner">Alice</span></a>
            <div class="userTitle">Senior Member</div>
          </div>
          <div class="message-content">
            <div class="bbWrapper">Hello world. <blockquote>quoted thing</blockquote></div>
          </div>
          <time datetime="2024-05-12T14:30:00Z">May 12, 2024</time>
        </article>
        <article class="message" data-author="bob">
          <div class="message-userDetails">
            <a class="username" href="/members/bob.99/"><span class="username-inner">Bob</span></a>
            <div class="userTitle">Regular</div>
          </div>
          <div class="message-content">
            <div class="bbWrapper">Reply body</div>
          </div>
          <time datetime="2024-05-12T15:00:00Z">May 12, 2024</time>
        </article>
        </body></html>"##
    }

    #[test]
    fn linear_extraction_preserves_post_order() {
        let s = SelectorSet {
            traversal_mode: TraversalMode::Linear,
            post_selector: "article.message".into(),
            author_selector: ".username-inner".into(),
            author_url_selector: Some(".username".into()),
            timestamp_selector: Some("time".into()),
            body_selector: ".bbWrapper".into(),
            author_rank_selector: Some(".userTitle".into()),
            ..Default::default()
        };
        let posts = apply(xenforo_like_html(), &s, Some("https://example.com")).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].author, "Alice");
        assert_eq!(posts[1].author, "Bob");
        assert_eq!(posts[0].author_rank.as_deref(), Some("Senior Member"));
        assert!(posts[0].body_html.contains("Hello world"));
        assert!(posts[0].body_html.contains("blockquote"));
        assert!(posts[0]
            .author_url
            .as_deref()
            .unwrap_or("")
            .starts_with("https://example.com/members/alice"));
        assert_eq!(
            posts[0].timestamp.as_deref(),
            Some("2024-05-12T14:30:00Z"),
            "datetime attr should win over text"
        );
    }

    #[test]
    fn linear_dedupes_post_repeated_in_first_post_highlight_widget() {
        // vBulletin 4 themes (notably BimmerPost / Bimmer Network) embed the
        // first post in BOTH a "highlighted first post" wrapper at the top of
        // the thread AND in the main post list below — same `<table
        // id="postNNN">` markup in two parent containers. Without dedup the
        // linear extractor would emit the same post twice. With dedup, the
        // first occurrence wins and the second is dropped.
        let html = r##"<!DOCTYPE html><html><body>
        <div id="firstpost_highlight">
          <table id="post1234">
            <tr><td class="thead"><div class="normal"><a href="#post1234">#1</a></div></td></tr>
            <tr>
              <td class="alt2"><a class="bigusername" href="/member.php?u=1">Alice</a></td>
              <td class="alt1"><div id="post_message_1234">Hello world from the OP.</div></td>
            </tr>
          </table>
        </div>
        <div id="posts">
          <table id="post1234">
            <tr><td class="thead"><div class="normal"><a href="#post1234">#1</a></div></td></tr>
            <tr>
              <td class="alt2"><a class="bigusername" href="/member.php?u=1">Alice</a></td>
              <td class="alt1"><div id="post_message_1234">Hello world from the OP.</div></td>
            </tr>
          </table>
          <table id="post1235">
            <tr><td class="thead"><div class="normal"><a href="#post1235">#2</a></div></td></tr>
            <tr>
              <td class="alt2"><a class="bigusername" href="/member.php?u=2">Bob</a></td>
              <td class="alt1"><div id="post_message_1235">Reply.</div></td>
            </tr>
          </table>
        </div>
        </body></html>"##;

        let s = SelectorSet {
            traversal_mode: TraversalMode::Linear,
            post_selector: "table[id^='post']".into(),
            author_selector: "td.alt2 a.bigusername".into(),
            body_selector: "div[id^='post_message_']".into(),
            post_number_selector: Some("td.thead div.normal a[href*='#post']".into()),
            ..Default::default()
        };
        let posts = apply(html, &s, None).unwrap();
        assert_eq!(posts.len(), 2, "expected 2 unique posts after dedup, got {}", posts.len());
        assert_eq!(posts[0].author, "Alice");
        assert_eq!(posts[0].post_number, Some(1));
        assert_eq!(posts[1].author, "Bob");
        assert_eq!(posts[1].post_number, Some(2));
    }

    #[test]
    fn linear_dedupes_by_fingerprint_when_post_number_missing() {
        // When the theme doesn't expose a per-post number, dedup falls back
        // to author + body-prefix fingerprint. Same source pattern as above:
        // one post repeated across two parent containers.
        let html = r##"<!DOCTYPE html><html><body>
        <div class="highlight">
          <article class="post"><span class="user">Alice</span><div class="body">Hello world.</div></article>
        </div>
        <div class="list">
          <article class="post"><span class="user">Alice</span><div class="body">Hello world.</div></article>
          <article class="post"><span class="user">Bob</span><div class="body">Different reply.</div></article>
        </div>
        </body></html>"##;
        let s = SelectorSet {
            traversal_mode: TraversalMode::Linear,
            post_selector: "article.post".into(),
            author_selector: "span.user".into(),
            body_selector: "div.body".into(),
            ..Default::default()
        };
        let posts = apply(html, &s, None).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].author, "Alice");
        assert_eq!(posts[1].author, "Bob");
    }

    #[test]
    fn extra_media_skipped_when_overlapping_body() {
        // Mirrors a real overlap case: body_selector targets a .md that is
        // a descendant of .expando, and extra_media_selectors targets the
        // same .expando. Without overlap detection, the body content would
        // be duplicated (once from the body selector, once from .expando's
        // inner_html which contains the .md).
        let html = r##"<html><body>
            <article class="post">
              <div class="entry">
                <div class="expando">
                  <div class="usertext-body"><div class="md"><p>OP text once</p></div></div>
                </div>
              </div>
            </article>
        </body></html>"##;
        let s = SelectorSet {
            traversal_mode: TraversalMode::Linear,
            post_selector: "article.post".into(),
            author_selector: ".entry".into(),
            body_selector: ".entry .usertext-body .md".into(),
            extra_media_selectors: vec![".expando".into()],
            ..Default::default()
        };
        let posts = apply(html, &s, None).unwrap();
        assert_eq!(posts.len(), 1);
        let occurrences = posts[0].body_html.matches("OP text once").count();
        assert_eq!(
            occurrences, 1,
            "expected body to appear once, got {occurrences} times: {}",
            posts[0].body_html
        );
        assert!(
            !posts[0].body_html.contains("vb-extra-media"),
            "overlapping extra_media (body inside .expando) should have been skipped"
        );
    }

    #[test]
    fn extra_media_appended_when_sibling_of_body() {
        // Link post: .expando is a sibling of .usertext, not a descendant.
        // SHOULD be appended.
        let html = r##"<html><body>
            <article class="post">
              <div class="entry">
                <div class="usertext"><div class="usertext-body"><div class="md"><p>link comment</p></div></div></div>
                <div class="expando"><img src="https://i.imgur.com/foo.jpg"></div>
              </div>
            </article>
        </body></html>"##;
        let s = SelectorSet {
            traversal_mode: TraversalMode::Linear,
            post_selector: "article.post".into(),
            author_selector: ".entry".into(),
            body_selector: ".usertext .md".into(),
            extra_media_selectors: vec![".expando".into()],
            ..Default::default()
        };
        let posts = apply(html, &s, None).unwrap();
        assert_eq!(posts.len(), 1);
        assert!(posts[0].body_html.contains("link comment"));
        assert!(
            posts[0].body_html.contains("vb-extra-media"),
            "sibling extra_media should have been appended: {}",
            posts[0].body_html
        );
        assert!(
            posts[0].body_html.contains("i.imgur.com/foo.jpg"),
            "imgur img should have been carried in via .expando: {}",
            posts[0].body_html
        );
    }

    #[test]
    fn parses_like_counts_from_realistic_text() {
        assert_eq!(parse_like_count("37 points"), Some(37));
        assert_eq!(parse_like_count("37 points 2 hours ago"), Some(37));
        assert_eq!(parse_like_count("1,234"), Some(1234));
        assert_eq!(parse_like_count("1.2k"), Some(1200));
        assert_eq!(parse_like_count("3M views"), Some(3_000_000));
        assert_eq!(parse_like_count("-5"), Some(-5));
        assert_eq!(parse_like_count("0 likes"), Some(0));
        assert_eq!(parse_like_count("•"), None);
        assert_eq!(parse_like_count("score hidden"), None);
        assert_eq!(parse_like_count(""), None);
        assert_eq!(parse_like_count("just text"), None);
    }

    #[test]
    fn body_sanitizer_strips_scripts_keeps_blockquote() {
        let html = sanitize_body(
            r#"hello<script>alert(1)</script><blockquote class="foo">q</blockquote>"#,
            None,
        );
        assert!(!html.contains("script"));
        assert!(html.contains("blockquote"));
        assert!(html.contains("foo"));
    }

    #[test]
    fn threaded_replies_fold_top_child_into_blockquote() {
        let html = r##"<html><body>
            <div id="siteTable">
              <div class="thing link">
                <a class="title">OP title</a>
                <div class="usertext-body"><div class="md"><p>OP body text</p></div></div>
                <p class="tagline"><a class="author" href="/user/op">op</a></p>
              </div>
            </div>
            <div class="commentarea">
              <div class="sitetable nestedlisting">
                <div class="thing comment" data-fullname="t1_a">
                  <div class="entry">
                    <p class="tagline"><a class="author" href="/user/alice">alice</a></p>
                    <div class="usertext-body"><div class="md"><p>top level comment</p></div></div>
                  </div>
                  <div class="child">
                    <div class="sitetable listing">
                      <div class="thing comment" data-fullname="t1_b">
                        <div class="entry">
                          <p class="tagline"><a class="author" href="/user/bob">bob</a></p>
                          <div class="usertext-body"><div class="md"><p>reply body</p></div></div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
        </body></html>"##;

        // post_selector matches BOTH top-level and nested comments (the
        // common real-world agent output for Reddit). The new
        // DOM-ancestry-based extractor handles nesting from this alone.
        let s = SelectorSet {
            traversal_mode: TraversalMode::ThreadedReplies,
            op_selector: Some("#siteTable > .thing.link".into()),
            post_selector: ".commentarea .thing.comment".into(),
            author_selector: ".tagline .author".into(),
            author_url_selector: Some(".tagline .author".into()),
            body_selector: ".usertext-body .md".into(),
            ..Default::default()
        };
        let posts = apply(html, &s, Some("https://old.reddit.com")).unwrap();
        // OP + 1 top-level comment + 1 nested reply = 3 posts
        assert_eq!(posts.len(), 3, "expected OP + top-level + child, got {posts:#?}");
        assert_eq!(posts[0].author, "op");
        assert_eq!(posts[1].author, "alice");
        assert_eq!(posts[2].author, "bob");

        // alice (top-level reply to OP) renders BARE — no quote-box, no
        // "Originally Posted by OP" preamble. The OP is right above, so the
        // relationship is obvious and the blockquote was just noise.
        assert_eq!(posts[1].quote_of_post_number, None);
        assert!(!posts[1].body_html.contains(r#"class="vb-quote""#));
        assert!(posts[1].body_html.contains("top level comment"));

        // bob (nested under alice) DOES quote alice — replies to non-OP
        // comments still get the quote-box so the chain is readable.
        assert_eq!(posts[2].quote_of_post_number, Some(2));
        assert!(posts[2].body_html.contains(r#"class="vb-quote""#));
        assert!(posts[2].body_html.contains("alice"));
        assert!(posts[2].body_html.contains("reply body"));
        // Alice no longer carries a quote-of-OP, so bob's quote-box is a
        // single level, not nested.
        let count = posts[2].body_html.matches("vb-quote").count();
        assert_eq!(count, 1, "expected single quote-box (alice has no quote-of-op to nest), got {count} in: {}", posts[2].body_html);
    }
}

