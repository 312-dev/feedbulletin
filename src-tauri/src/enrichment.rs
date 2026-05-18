// Author avatar enrichment, driven through the live WebView.
//
// WHY THE WEBVIEW INSTEAD OF REQWEST: most modern sites (Reddit since 2023,
// Discourse with auth, XenForo with login walls) lock down un-authenticated
// HTTP fetches behind 429/403. The WebView is already loaded at the thread
// page's origin and carries the user's cookies, so a JS `fetch()` from
// within it succeeds as a same-origin, authenticated request — for free.
//
// The flow:
//   1. We eval one batch of JS that fires off `fetch()` for every unique
//      author and parses the response with the agent-supplied locator,
//      writing the resulting avatar URL into `window.__fr_avatars[author]`.
//   2. We sleep briefly to let the network round-trips settle.
//   3. We eval `JSON.stringify(window.__fr_avatars)` with a callback that
//      receives the map, then dispatch through the cache and emit the
//      avatars_resolved event.

use crate::db;
use crate::scrape::{Post, SelectorSet};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::Duration;
use tracing::{debug, warn};
use url::form_urlencoded;

/// Avatars + sidebar metadata pulled from a user's profile page. All
/// optional — the agent populates whatever locators the platform exposes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_count: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub karma: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvatarUpdate {
    pub author: String,
    #[serde(flatten)]
    pub fields: ProfileFields,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvatarsResolvedPayload {
    pub host: String,
    pub updates: Vec<AvatarUpdate>,
}

/// Per-author work item passed into the WebView JS. Carries a map of
/// {field_name → locator} so the in-page JS can extract all configured
/// fields from one fetched profile response.
#[derive(Debug, Clone, Serialize)]
struct FetchItem {
    author: String,
    url: String,
    format: String,
    locators: std::collections::BTreeMap<String, String>,
}

/// What the WebView posts back. Map of author → {field_name: value-or-null}.
#[derive(Debug, Deserialize, Default)]
struct ProfileMap(
    std::collections::HashMap<String, std::collections::HashMap<String, Option<String>>>,
);

/// Set up the WebView state and dispatch the per-author fetches.
/// Returns the JS to eval. Each fetched profile yields ALL configured
/// fields (avatar + post_count + karma + join_date + last_active + location +
/// rank), keyed by field name, stashed in window.__fr_profiles[author].
pub fn build_dispatch_js(
    selectors: &SelectorSet,
    posts: &[Post],
    cached: &std::collections::HashMap<String, ProfileFields>,
) -> Option<String> {
    let template = selectors.author_profile_url_template.as_deref()?;
    let format = selectors.author_avatar_format.as_deref()?;
    if template.is_empty() || format.is_empty() {
        return None;
    }

    // Build map of locators. Only include fields the agent populated.
    let mut locators: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let pairs: &[(&str, Option<&String>)] = &[
        ("avatar_url", selectors.author_avatar_locator.as_ref()),
        ("post_count", selectors.author_post_count_locator.as_ref()),
        ("karma", selectors.author_karma_locator.as_ref()),
        ("join_date", selectors.author_join_date_locator.as_ref()),
        ("last_active", selectors.author_last_active_locator.as_ref()),
        ("location", selectors.author_location_locator.as_ref()),
        ("rank", selectors.author_rank_locator.as_ref()),
    ];
    for (k, v) in pairs {
        if let Some(s) = v {
            if !s.trim().is_empty() {
                locators.insert((*k).to_string(), s.to_string());
            }
        }
    }
    if locators.is_empty() {
        return None;
    }

    let mut unique: Vec<String> = posts
        .iter()
        .filter(|p| !p.author.trim().is_empty() && !cached.contains_key(&p.author))
        .map(|p| p.author.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique.sort();
    if unique.is_empty() {
        return None;
    }

    let items: Vec<FetchItem> = unique
        .into_iter()
        .map(|author| {
            let encoded: String =
                form_urlencoded::byte_serialize(author.as_bytes()).collect();
            FetchItem {
                url: template.replace("{author}", &encoded),
                author,
                format: format.to_string(),
                locators: locators.clone(),
            }
        })
        .collect();

    let items_json = serde_json::to_string(&items).ok()?;
    Some(format!(
        r#"
        (function() {{
          if (!window.__fr_profiles) window.__fr_profiles = {{}};
          const items = {items_json};
          items.forEach(function(it) {{
            (async function() {{
              const fields = {{}};
              try {{
                const r = await fetch(it.url, {{ credentials: 'include', headers: {{ 'Accept': it.format === 'json' ? 'application/json' : 'text/html' }} }});
                if (!r.ok) {{ window.__fr_profiles[it.author] = fields; return; }}
                const body = await r.text();
                let parsed = null;
                if (it.format === 'json') {{
                  try {{ parsed = JSON.parse(body); }} catch (e) {{}}
                }} else {{
                  try {{ parsed = new DOMParser().parseFromString(body, 'text/html'); }} catch (e) {{}}
                }}
                Object.entries(it.locators).forEach(function(pair) {{
                  const key = pair[0], locator = pair[1];
                  let val = null;
                  if (it.format === 'json' && parsed) {{
                    val = locator.split('.').reduce(function(o, k) {{
                      return (o != null && o[k] !== undefined) ? o[k] : null;
                    }}, parsed);
                  }} else if (parsed) {{
                    const el = parsed.querySelector(locator);
                    if (el) {{
                      if (key === 'avatar_url') {{
                        val = el.getAttribute('src') || el.getAttribute('data-src') || el.getAttribute('href') || null;
                      }} else {{
                        val = (el.textContent || '').trim() || null;
                      }}
                    }}
                  }}
                  if (typeof val === 'string') {{
                    val = val.replace(/&amp;/g, '&').replace(/&quot;/g, '"').trim();
                    if (key === 'avatar_url' && val && !/^https?:\/\//i.test(val)) {{
                      try {{ val = new URL(val, document.baseURI).href; }} catch (e) {{}}
                    }}
                  }}
                  if (val) fields[key] = String(val);
                }});
              }} catch (e) {{}}
              window.__fr_profiles[it.author] = fields;
            }})();
          }});
        }})();
        "#
    ))
}

/// JS we eval after a short wait to read the results map.
pub const READ_AVATARS_JS: &str = "JSON.stringify(window.__fr_profiles || {})";

/// Decode the JSON the WebView returns. Returns (author → field map).
pub fn decode_results(raw: &str) -> std::collections::HashMap<String, ProfileFields> {
    let inner: String = match serde_json::from_str(raw) {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    let parsed = match serde_json::from_str::<ProfileMap>(&inner) {
        Ok(m) => m.0,
        Err(_) => return Default::default(),
    };
    let mut out = std::collections::HashMap::new();
    for (author, m) in parsed {
        let mut f = ProfileFields::default();
        f.avatar_url = m.get("avatar_url").and_then(|v| v.clone());
        f.post_count = m.get("post_count").and_then(|v| v.clone());
        f.karma = m.get("karma").and_then(|v| v.clone());
        f.join_date = m.get("join_date").and_then(|v| v.clone());
        f.last_active = m.get("last_active").and_then(|v| v.clone());
        f.location = m.get("location").and_then(|v| v.clone());
        f.rank = m.get("rank").and_then(|v| v.clone());
        out.insert(author, f);
    }
    out
}

/// Persist results to the DB cache and produce the AvatarUpdate list to
/// emit to the frontend. Already-cached entries get emitted too so a single
/// frontend listener call covers everyone.
pub async fn persist_and_collect(
    pool: &SqlitePool,
    host: &str,
    fresh: &std::collections::HashMap<String, ProfileFields>,
    existing_cached: &std::collections::HashMap<String, ProfileFields>,
) -> Vec<AvatarUpdate> {
    let mut out = Vec::new();
    for (author, fields) in existing_cached {
        if has_any_field(fields) {
            out.push(AvatarUpdate {
                author: author.clone(),
                fields: fields.clone(),
            });
        }
    }
    for (author, fields) in fresh {
        let _ = db::cache_author_profile(pool, host, author, fields).await;
        if has_any_field(fields) {
            out.push(AvatarUpdate {
                author: author.clone(),
                fields: fields.clone(),
            });
        } else {
            debug!(host, author, "cached as no-data");
        }
    }
    out
}

fn has_any_field(f: &ProfileFields) -> bool {
    f.avatar_url.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
        || f.post_count.is_some()
        || f.karma.is_some()
        || f.join_date.is_some()
        || f.last_active.is_some()
        || f.location.is_some()
        || f.rank.is_some()
}

/// Helper for the dispatcher: pre-load cached profile fields for every author
/// in `posts` from the DB. Returns map (only entries that exist in the cache).
pub async fn load_cached(
    pool: &SqlitePool,
    host: &str,
    posts: &[Post],
) -> std::collections::HashMap<String, ProfileFields> {
    let mut out = std::collections::HashMap::new();
    let mut seen = HashSet::new();
    for p in posts {
        if p.author.trim().is_empty() || !seen.insert(p.author.clone()) {
            continue;
        }
        match db::get_cached_author_profile(pool, host, &p.author).await {
            Ok(Some(fields)) => {
                out.insert(p.author.clone(), fields);
            }
            Ok(None) => {}
            Err(e) => warn!(host, author=%p.author, err=%e, "profile cache read failed"),
        }
    }
    out
}

/// Reasonable wait between dispatch and read so the JS fetches complete.
pub fn dispatch_delay() -> Duration {
    Duration::from_millis(2500)
}

/// Returned when no fetch is necessary (everything cached).
pub fn nothing_to_dispatch() -> Result<()> {
    Err(anyhow!("nothing to dispatch"))
}
