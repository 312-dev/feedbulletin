use crate::db::ThreadInput;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Listing {
    data: ListingData,
}

#[derive(Debug, Deserialize)]
struct ListingData {
    children: Vec<Child>,
}

#[derive(Debug, Deserialize)]
struct Child {
    data: Post,
}

#[derive(Debug, Deserialize)]
struct Post {
    id: String,
    title: String,
    author: Option<String>,
    permalink: Option<String>,
    url: Option<String>,
    num_comments: Option<i64>,
    created_utc: Option<f64>,
    selftext: Option<String>,
    subreddit: Option<String>,
    score: Option<i64>,
    ups: Option<i64>,
}

pub fn parse(body: &str) -> Result<Vec<ThreadInput>> {
    // Reddit returns 200 + a non-Listing JSON payload for suspended/banned users
    // and a few other edge cases. Surface a clean error in that case rather than
    // a generic serde "expected struct Listing" message.
    let v: serde_json::Value = serde_json::from_str(body).context("reddit json parse")?;
    if let Some(reason) = v.get("reason").and_then(|x| x.as_str()) {
        anyhow::bail!("reddit refused: {}", reason);
    }
    if let Some(msg) = v.get("message").and_then(|x| x.as_str()) {
        let err = v.get("error").and_then(|x| x.as_i64()).unwrap_or(0);
        anyhow::bail!("reddit error {}: {}", err, msg);
    }
    let listing: Listing =
        serde_json::from_value(v).context("reddit response is not a Listing")?;
    let mut out = Vec::with_capacity(listing.data.children.len());
    for c in listing.data.children {
        let p = c.data;
        let permalink = p
            .permalink
            .clone()
            .map(|pl| format!("https://www.reddit.com{}", pl))
            .or_else(|| p.url.clone())
            .unwrap_or_else(|| format!("https://www.reddit.com/comments/{}", p.id));
        let excerpt = p
            .selftext
            .as_deref()
            .map(|s| crate::forums::generic_rss::truncate_chars(s.trim(), 280))
            .filter(|s| !s.is_empty());
        let author = p.author.filter(|a| a != "[deleted]");
        let author_url = author
            .as_deref()
            .map(|a| format!("https://www.reddit.com/user/{}/", a));
        // WHY: hot_score = score (preferred) or ups, with comment count folded in at
        // 0.1x weight so high-engagement threads ratchet above low-comment scores.
        let raw_score = p.score.or(p.ups);
        let hot_score = raw_score.map(|s| {
            let comments = p.num_comments.unwrap_or(0) as f64;
            (s as f64) + comments * 0.1
        });
        out.push(ThreadInput {
            source_url: permalink,
            title: p.title,
            op_author: author,
            op_author_url: author_url,
            pubdate: p.created_utc.map(|f| f as i64),
            reply_count: p.num_comments,
            excerpt,
            hot_score,
            ..Default::default()
        });
        // suppress unused warning for subreddit:
        let _ = &p.subreddit;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_listing() {
        let body = r#"{"kind":"Listing","data":{"children":[
            {"kind":"t3","data":{"id":"abc","title":"Hello","author":"alice","permalink":"/r/foo/comments/abc/hello/","num_comments":3,"created_utc":1700000000.0}}
        ]}}"#;
        let r = parse(body).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Hello");
        assert_eq!(r[0].op_author.as_deref(), Some("alice"));
        assert_eq!(r[0].reply_count, Some(3));
        assert_eq!(r[0].source_url, "https://www.reddit.com/r/foo/comments/abc/hello/");
    }
}
