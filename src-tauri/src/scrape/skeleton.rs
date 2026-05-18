// DOM minifier — produces a structural skeleton of an HTML document suitable
// for sending to the Anthropic API for site-profile learning. Strips scripts,
// styles, base64 data URIs, comments, and most attribute noise; truncates long
// text nodes; collapses repeated runs of identical-shaped siblings (so we send
// the first ~N posts, not all 50, for cost control).
//
// The goal is a payload Claude can reason about structurally, not a faithful
// reproduction. We're paying tokens for HIERARCHY, not for content.

use scraper::{ElementRef, Html, Node};

/// Maximum characters per text node before truncation. Forum posts can be huge;
/// we only need enough text to confirm "yes there's a post here", not the body.
const TEXT_TRUNCATE_CHARS: usize = 80;

/// Maximum total length of the emitted skeleton. If we exceed this we trim
/// trailing siblings; sites with hundreds of posts on one page still produce
/// a tractable token count.
///
/// Raised from 48k to 180k after observing vBulletin 3.x sites (e90post,
/// bimmerpost) where the global nav/banner alone consumes ~40k of skeleton
/// before the first `<table id="post...">` row is reached. At 48k Claude
/// only ever saw the chrome and couldn't identify post selectors.
const MAX_SKELETON_LEN: usize = 180_000;

/// Attribute allow-list — everything else is dropped. Class and id are the
/// load-bearing structural signals; href/src let us reason about author URLs
/// and avatars; data-* keeps platform-specific markers (XenForo's `data-author`,
/// Discourse's `data-topic-id`, Reddit's `data-fullname`).
const KEEP_ATTRS: &[&str] = &[
    "class",
    "id",
    "href",
    "src",
    "alt",
    "title",
    "datetime",
    "role",
    "itemprop",
    "itemtype",
];

pub fn minify(html: &str) -> String {
    let doc = Html::parse_document(html);
    let mut out = String::with_capacity(8192);
    // Walk from the root; <head>, <script>, <style>, etc. are skipped inside walk().
    walk(doc.root_element(), &mut out, 0);
    if out.len() > MAX_SKELETON_LEN {
        out.truncate(MAX_SKELETON_LEN);
        out.push_str("\n<!-- truncated -->");
    }
    out
}

fn walk(el: ElementRef<'_>, out: &mut String, depth: usize) {
    if out.len() > MAX_SKELETON_LEN {
        return;
    }
    let tag = el.value().name();
    if matches!(
        tag,
        "script" | "style" | "noscript" | "svg" | "iframe" | "head" | "link" | "meta"
    ) {
        return;
    }

    let indent = "  ".repeat(depth.min(20));
    out.push('\n');
    out.push_str(&indent);
    out.push('<');
    out.push_str(tag);
    for (name, raw_val) in el.value().attrs() {
        if !KEEP_ATTRS.contains(&name) {
            continue;
        }
        let v = raw_val.trim();
        if v.is_empty() {
            continue;
        }
        // skip noisy base64 data URIs — they bloat tokens with no structural value
        if v.starts_with("data:") && v.len() > 64 {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"data:...\"");
            continue;
        }
        // truncate huge href/src
        let v_trim = if v.len() > 240 { &v[..240] } else { v };
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        for c in v_trim.chars() {
            match c {
                '"' => out.push('\''),
                '<' => out.push('('),
                '>' => out.push(')'),
                _ => out.push(c),
            }
        }
        out.push('"');
    }
    out.push('>');

    // children
    let mut child_count = 0usize;
    for child in el.children() {
        if out.len() > MAX_SKELETON_LEN {
            break;
        }
        match child.value() {
            Node::Element(_) => {
                if let Some(c) = ElementRef::wrap(child) {
                    walk(c, out, depth + 1);
                    child_count += 1;
                    // collapse very long sibling runs of structurally similar elements
                    // (forum-thread pages with 40+ posts) — keep first 8 + ellipsis.
                    if child_count > 8 && is_repetitive_sibling(c) {
                        out.push('\n');
                        out.push_str(&indent);
                        out.push_str("  <!-- ... more siblings ... -->");
                        break;
                    }
                }
            }
            Node::Text(t) => {
                let s = t.trim();
                if s.is_empty() {
                    continue;
                }
                let truncated: String = s.chars().take(TEXT_TRUNCATE_CHARS).collect();
                out.push_str(&truncated);
                if s.chars().count() > TEXT_TRUNCATE_CHARS {
                    out.push('…');
                }
            }
            _ => {}
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// Heuristic: an element is "repetitive" if it has a class attribute (likely
/// a post/comment row), used to decide whether to truncate a long sibling run.
/// We err toward truncation — the LLM doesn't need 50 posts to infer selectors.
fn is_repetitive_sibling(el: ElementRef<'_>) -> bool {
    el.value().attr("class").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_styles() {
        let html = r#"<html><body>
            <script>alert(1)</script>
            <style>.x{color:red}</style>
            <div class="post">hello</div>
        </body></html>"#;
        let out = minify(html);
        assert!(!out.contains("alert"));
        assert!(!out.contains("color:red"));
        assert!(out.contains(r#"class="post""#));
    }

    #[test]
    fn truncates_long_text_nodes() {
        let html = format!(
            "<html><body><div>{}</div></body></html>",
            "x".repeat(500)
        );
        let out = minify(&html);
        assert!(!out.contains(&"x".repeat(500)));
        assert!(out.contains('…'));
    }

    #[test]
    fn drops_unknown_attributes() {
        let html = r#"<html><body><div class="a" onclick="bad()" data-noise="x">hi</div></body></html>"#;
        let out = minify(html);
        assert!(out.contains(r#"class="a""#));
        assert!(!out.contains("onclick"));
        assert!(!out.contains("data-noise"));
    }
}
