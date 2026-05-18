// In-app cookie cache. WKWebView persists its own cookies per-host across app
// runs (cookies are written to the WKHTTPCookieStorage backing store), so for
// most practical login flows we don't need to do anything ourselves — sign in
// once inside the app and the session stays alive.
//
// What lives here:
//   - `StoredCookie` / `CookieCache` types — keep the shape so existing call
//     sites (cookie_header_for, webview_cookies_for_url) still compile.
//   - SQLite-backed load/save helpers — currently unused at runtime; reserved
//     for if we later snapshot WKWebView cookies into our own DB for backup
//     or cross-machine sync.
//
// What was removed: Chrome cookie import via the `rookie` crate. The app now
// tracks its own cookies natively; the user signs in inside the embedded
// WebView and authentication persists from then on.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: Option<i64>,
}

pub type CookieCache = Arc<RwLock<HashMap<String, Vec<StoredCookie>>>>;

pub fn empty_cache() -> CookieCache {
    Arc::new(RwLock::new(HashMap::new()))
}

pub async fn save_to_db(pool: &SqlitePool, cookies: &[StoredCookie]) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM cookies").execute(&mut *tx).await?;
    for c in cookies {
        sqlx::query(
            "INSERT OR REPLACE INTO cookies(host,name,value,path,expires,secure,http_only,same_site,imported_at) \
             VALUES(?,?,?,?,?,?,?,?,?)",
        )
        .bind(&c.domain)
        .bind(&c.name)
        .bind(&c.value)
        .bind(&c.path)
        .bind(c.expires)
        .bind(c.secure as i64)
        .bind(c.http_only as i64)
        .bind(Option::<String>::None)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn load_from_db(pool: &SqlitePool) -> Result<Vec<StoredCookie>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>, i64, i64)>(
        "SELECT host,name,value,path,expires,secure,http_only FROM cookies",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| StoredCookie {
            domain: r.0,
            name: r.1,
            value: r.2,
            path: r.3,
            expires: r.4,
            secure: r.5 != 0,
            http_only: r.6 != 0,
        })
        .collect())
}

pub fn group_by_root_domain(cookies: Vec<StoredCookie>) -> HashMap<String, Vec<StoredCookie>> {
    let mut map: HashMap<String, Vec<StoredCookie>> = HashMap::new();
    for c in cookies {
        let key = c.domain.trim_start_matches('.').to_string();
        map.entry(key).or_default().push(c);
    }
    map
}

/// Build a `Cookie:` header value matching the given host. Returns None when
/// our cache is empty for this host — WKWebView still sends its own cookies
/// with every fetch, so this is only useful when *we* (Rust-side) make HTTP
/// requests through reqwest for feed polling.
pub fn cookie_header_for(host: &str, cache: &CookieCache) -> Option<String> {
    let map = cache.read().ok()?;
    let mut parts: Vec<String> = Vec::new();
    for (domain, cookies) in map.iter() {
        if host_matches(host, domain) {
            for c in cookies {
                parts.push(format!("{}={}", c.name, c.value));
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn host_matches(host: &str, cookie_domain: &str) -> bool {
    let h = host.trim_start_matches("www.");
    let d = cookie_domain.trim_start_matches('.').trim_start_matches("www.");
    h == d || h.ends_with(&format!(".{d}"))
}

/// Convert all cookies in the cache that match a URL's host into the `cookie` crate's
/// owned Cookie type so Tauri can install them into the WebView.
pub fn webview_cookies_for_url(url: &url::Url, cache: &CookieCache) -> Vec<cookie::Cookie<'static>> {
    let host = match url.host_str() {
        Some(h) => h.to_string(),
        None => return Vec::new(),
    };
    let map = match cache.read() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for (domain, cookies) in map.iter() {
        if !host_matches(&host, domain) {
            continue;
        }
        for c in cookies {
            let mut builder = cookie::Cookie::build((c.name.clone(), c.value.clone()))
                .domain(domain.clone())
                .path(c.path.clone())
                .secure(c.secure)
                .http_only(c.http_only);
            if let Some(exp) = c.expires {
                if let Ok(odt) = time::OffsetDateTime::from_unix_timestamp(exp) {
                    builder = builder.expires(cookie::Expiration::DateTime(odt));
                }
            }
            out.push(builder.build());
        }
    }
    out
}
