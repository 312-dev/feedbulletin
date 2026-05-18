use crate::config::{ForumConfig, ForumKind};
use crate::cookies::{cookie_header_for, CookieCache};
use crate::db::ThreadInput;
use anyhow::Result;

pub mod generic_rss;
pub mod reddit;
pub mod xenforo;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub const REDDIT_USER_AGENT: &str =
    "ForumReader/0.1 by GraysonCAdams (local desktop client; +https://github.com/GraysonCAdams/forum-reader)";

pub async fn fetch_forum(
    client: &reqwest::Client,
    cfg: &ForumConfig,
    cookies: Option<&CookieCache>,
) -> Result<Vec<ThreadInput>> {
    let primary = cfg.resolved_source_url();
    let (ua, accept) = match cfg.kind {
        ForumKind::Reddit => (REDDIT_USER_AGENT, "application/json"),
        _ => (
            USER_AGENT,
            "application/rss+xml, application/atom+xml, application/xml;q=0.9, */*;q=0.8",
        ),
    };

    let candidates = candidate_urls(&primary);
    let mut last_err: Option<anyhow::Error> = None;
    for url in candidates {
        let cookie_header = cookies.and_then(|c| {
            url::Url::parse(&url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()))
                .and_then(|h| cookie_header_for(&h, c))
        });
        match http_get(client, &url, ua, accept, cookie_header.as_deref()).await {
            Ok(body) => {
                let parsed = match cfg.kind {
                    ForumKind::Reddit => reddit::parse(&body),
                    ForumKind::Xenforo => xenforo::parse(&body),
                    ForumKind::GenericRss => generic_rss::parse(&body),
                    _ => generic_rss::parse(&body),
                };
                match parsed {
                    Ok(v) => return Ok(v),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no candidate URLs succeeded")))
}

// WHY: a few feeds (LTH Forum) ship under multiple path schemes depending on board
// version; try fallbacks before bubbling a hard failure that paints "unavailable".
fn candidate_urls(primary: &str) -> Vec<String> {
    let mut out = vec![primary.to_string()];
    if primary.contains("lthforum.com") {
        out.push("https://www.lthforum.com/bb/feed.php?mode=topics".to_string());
        out.push("https://www.lthforum.com/feed/".to_string());
    }
    out
}

pub async fn http_get(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
    accept: &str,
    cookie_header: Option<&str>,
) -> Result<String> {
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent)
        .header(reqwest::header::ACCEPT, accept)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9");
    if let Some(h) = cookie_header {
        req = req.header(reqwest::header::COOKIE, h);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("http {} fetching {}", status.as_u16(), url);
    }
    let text = resp.text().await?;
    Ok(text)
}
