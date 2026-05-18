use crate::db::ThreadInput;
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use quick_xml::events::Event;
use quick_xml::reader::Reader;

#[derive(Default, Debug)]
struct ItemBuf {
    title: Option<String>,
    link: Option<String>,
    author: Option<String>,
    dc_creator: Option<String>,
    author_uri: Option<String>,
    pubdate: Option<String>,
    description: Option<String>,
    slash_comments: Option<i64>,
}

impl ItemBuf {
    fn into_thread(self) -> Option<ThreadInput> {
        let title = self.title.unwrap_or_default();
        let link = self.link.unwrap_or_default();
        if title.trim().is_empty() && link.trim().is_empty() {
            return None;
        }
        let author = self
            .dc_creator
            .or(self.author)
            .map(|a| strip_email_prefix(&a).to_string());
        // WHY: derive Discourse profile URL if the link host follows discourse's
        // /t/.../<num> shape and a username is known. Atom feeds may also include
        // an explicit <author><uri> we should prefer.
        let op_author_url = self
            .author_uri
            .filter(|u| !u.is_empty())
            .or_else(|| author.as_deref().and_then(|a| discourse_user_url(&link, a)));
        let pubdate = self.pubdate.as_deref().and_then(parse_pubdate);
        let description_html = self.description.clone();
        let excerpt = self
            .description
            .map(|d| strip_html(&d))
            .map(|d| truncate_chars(d.trim(), 280))
            .filter(|s| !s.is_empty());
        // WHY: when a feed exposes a reply count (XenForo <slash:comments>, Discourse
        // "X posts - Y participants" small-tag inside description), promote it to
        // hot_score so the Hot sort has a real signal here.
        let discourse_posts = description_html.as_deref().and_then(extract_discourse_post_count);
        let reply_count = self.slash_comments.or(discourse_posts);
        let hot_score = reply_count.map(|n| n as f64);
        Some(ThreadInput {
            source_url: link,
            title,
            op_author: author,
            op_author_url,
            pubdate,
            reply_count,
            excerpt,
            hot_score,
            ..Default::default()
        })
    }
}

// WHY: Discourse description HTML contains "<small>N post(s) - M participants</small>"
// — we promote N into reply_count + hot_score so Hot sort works on HA/Fairphone/Actual Budget.
fn extract_discourse_post_count(s: &str) -> Option<i64> {
    let lower = s.to_ascii_lowercase();
    let posts_idx = lower.find(" post")?;
    let prefix = &s[..posts_idx];
    let num: String = prefix
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || c.is_whitespace())
        .collect();
    let digits: String = num.chars().rev().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok()
}

// WHY: Discourse instances expose stable user profiles at https://<host>/u/<name>.
// Many feeds we ingest (Home Assistant, Fairphone) are Discourse — detect via /t/ in path.
fn discourse_user_url(link: &str, author: &str) -> Option<String> {
    let parsed = url::Url::parse(link).ok()?;
    let host = parsed.host_str()?;
    let path = parsed.path();
    // Heuristic: Discourse thread links contain "/t/" segments. Bogleheads/phpBB don't.
    if !path.contains("/t/") {
        return None;
    }
    let scheme = parsed.scheme();
    Some(format!("{}://{}/u/{}", scheme, host, author))
}

pub fn parse(body: &str) -> Result<Vec<ThreadInput>> {
    // Defensive sniff: many cloudflare-protected sites return 200 OK with
    // an HTML challenge page that the strict XML parser will choke on.
    let trimmed = body.trim_start();
    if trimmed.starts_with("<!DOCTYPE html") || trimmed.starts_with("<html") {
        anyhow::bail!("upstream returned HTML, not a feed (likely cloudflare-protected)");
    }
    parse_rss_like(body, false)
}

pub(crate) fn parse_rss_like(body: &str, capture_slash: bool) -> Result<Vec<ThreadInput>> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut items: Vec<ThreadInput> = Vec::new();
    let mut in_item = false;
    let mut in_entry = false;
    let mut cur: ItemBuf = ItemBuf::default();
    let mut path: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let name = String::from_utf8_lossy(&name_bytes).to_string();
                path.push(name.clone());
                match name.as_str() {
                    "item" => {
                        in_item = true;
                        cur = ItemBuf::default();
                    }
                    "entry" => {
                        in_entry = true;
                        cur = ItemBuf::default();
                    }
                    "link" if in_entry => {
                        // Atom <link href="..."/>
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == b"href" {
                                if let Ok(val) = a.unescape_value() {
                                    cur.link = Some(val.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let name = String::from_utf8_lossy(&name_bytes).to_string();
                if (in_entry || in_item) && name == "link" {
                    for a in e.attributes().flatten() {
                        if a.key.as_ref() == b"href" {
                            if let Ok(val) = a.unescape_value() {
                                cur.link = Some(val.to_string());
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if !in_item && !in_entry {
                    continue;
                }
                let txt = match t.unescape() {
                    Ok(s) => s.to_string(),
                    Err(_) => continue,
                };
                let cur_tag = path.last().cloned().unwrap_or_default();
                match cur_tag.as_str() {
                    "title" => append(&mut cur.title, &txt),
                    "link" => append(&mut cur.link, &txt),
                    // WHY: atom <author> wraps <name>; capture text of <name> as author.
                    "name" if path.iter().any(|p| p == "author") => {
                        append(&mut cur.author, &txt)
                    }
                    "author" => append(&mut cur.author, &txt),
                    "uri" if path.iter().any(|p| p == "author") => {
                        append(&mut cur.author_uri, &txt)
                    }
                    "dc:creator" => append(&mut cur.dc_creator, &txt),
                    "pubDate" | "published" | "updated" => append(&mut cur.pubdate, &txt),
                    "description" | "summary" | "content" | "content:encoded" => {
                        append(&mut cur.description, &txt)
                    }
                    "slash:comments" if capture_slash => {
                        if let Ok(n) = txt.trim().parse::<i64>() {
                            cur.slash_comments = Some(n);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::CData(t)) => {
                if !in_item && !in_entry {
                    continue;
                }
                let txt = String::from_utf8_lossy(t.as_ref()).to_string();
                let cur_tag = path.last().cloned().unwrap_or_default();
                match cur_tag.as_str() {
                    "title" => append(&mut cur.title, &txt),
                    "description" | "summary" | "content" | "content:encoded" => {
                        append(&mut cur.description, &txt)
                    }
                    "dc:creator" => append(&mut cur.dc_creator, &txt),
                    "author" => append(&mut cur.author, &txt),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let name = String::from_utf8_lossy(&name_bytes).to_string();
                if name == "item" || name == "entry" {
                    if let Some(t) = std::mem::take(&mut cur).into_thread() {
                        items.push(t);
                    }
                    in_item = false;
                    in_entry = false;
                }
                path.pop();
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("xml parse error at pos {}: {:?}", reader.buffer_position(), e),
            _ => {}
        }
        buf.clear();
    }
    Ok(items)
}

fn append(slot: &mut Option<String>, txt: &str) {
    match slot {
        Some(s) => s.push_str(txt),
        None => *slot = Some(txt.to_string()),
    }
}

pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut count = 0;
    let mut end = s.len();
    for (i, _) in s.char_indices() {
        if count == max_chars {
            end = i;
            break;
        }
        count += 1;
    }
    if end < s.len() {
        format!("{}…", &s[..end])
    } else {
        s.to_string()
    }
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn strip_email_prefix(a: &str) -> &str {
    if let Some((_, rest)) = a.split_once('(') {
        if let Some((name, _)) = rest.split_once(')') {
            return name.trim();
        }
    }
    a.trim()
}

fn parse_pubdate(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return Some(Utc.from_utc_datetime(&ndt).timestamp());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&ndt).timestamp());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_rss() {
        let body = r#"<?xml version="1.0"?>
<rss><channel><title>foo</title>
<item><title>Hello</title><link>https://x/1</link>
<author>bob</author><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
<description>hi there</description></item>
</channel></rss>"#;
        let r = parse(body).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Hello");
        assert_eq!(r[0].source_url, "https://x/1");
        assert_eq!(r[0].op_author.as_deref(), Some("bob"));
        assert!(r[0].pubdate.is_some());
    }

    #[test]
    fn parses_atom() {
        let body = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<entry><title>A</title><link href="https://x/a"/>
<author><name>alice</name></author>
<updated>2024-01-01T00:00:00Z</updated>
<summary>s</summary></entry>
</feed>"#;
        let r = parse(body).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "A");
        assert_eq!(r[0].source_url, "https://x/a");
    }
}
