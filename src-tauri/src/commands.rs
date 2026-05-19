use crate::cookies::{self, CookieCache};
use crate::db::{self, CategoryWithForums, ForumRow, ThreadRow};
use crate::poller;
use crate::scrape;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    webview::{PageLoadEvent, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl,
};

pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<crate::config::AppConfig>,
    /// Filesystem path of the active feeds.yaml. Used by the settings UI to
    /// persist edits — restart is required for the changes to take effect.
    pub config_path: std::path::PathBuf,
    pub cookies: CookieCache,
    /// Thread IDs we've already auto-expanded media for in this app session.
    /// Prevents an infinite click→rescrape→click loop when the expand-button
    /// selector still matches after click (rare, but seen with double-state
    /// toggles).
    pub expanded_threads: Arc<std::sync::Mutex<std::collections::HashSet<i64>>>,
    /// Current app theme as a "is dark" flag. Used so newly opened thread
    /// webviews boot with the correct `color-scheme` hint without an extra
    /// IPC round-trip from the frontend.
    pub webview_dark: Arc<std::sync::atomic::AtomicBool>,
}


const THREAD_WEBVIEW_LABEL: &str = "thread_view";
// WHY: two-tier sticky header in /thread/[id]:
//   28px Forum Reader brand strip + 40px thread nav strip = 68px in HTML coords.
// On macOS, Tauri's child-webview LogicalPosition is measured from the OUTER
// window top (i.e. the OS title bar is at y=0..~28). So to land the webview at
// content-area-y=68 we need to ADD the title bar height: 68 + 28 = 96.
// On non-macOS the LogicalPosition appears to use content-area coords, so the
// offset is just 68. Adjust empirically per OS.
#[cfg(target_os = "macos")]
pub const TOOLBAR_HEIGHT: f64 = 96.0;
#[cfg(not(target_os = "macos"))]
pub const TOOLBAR_HEIGHT: f64 = 68.0;

// WHY: legacy vBulletin templates (Bimmerforums et al.) hard-code image widths and
// huge tables that overflow our child webview. Inject this before content paints so
// elements are clamped to viewport-100% on every page we load. Also force scroll
// to the page top in case the source URL carries a #postN anchor that would
// otherwise land the user mid-thread.
/// JS that runs once at PageLoadEvent::Finished — returns a JSON string with
/// the host, url, and a capped slice of the rendered DOM. The Rust callback
/// decodes this and dispatches to `scrape::handle_sample` to learn/apply the
/// site profile. We cap the HTML at 800KB before serialization to keep IPC
/// latency reasonable; Rust's skeleton minifier handles the rest.
const SCRAPE_RETURN_JS: &str = r#"
(function() {
  try {
    var html = document.documentElement && document.documentElement.outerHTML
      ? document.documentElement.outerHTML
      : "";
    if (html.length > 800000) html = html.slice(0, 800000);
    return JSON.stringify({
      kind: "scrape_sample",
      host: location.host,
      url: location.href,
      html: html
    });
  } catch (e) {
    return JSON.stringify({kind: "scrape_error", error: String(e)});
  }
})()
"#;

/// Rewrite reddit.com / np.reddit.com / new.reddit.com to old.reddit.com.
/// Old-reddit's DOM is stable enough that the LLM-derived selectors don't break
/// every week; new-reddit's React render is too volatile. URL transform only —
/// nothing platform-specific lives in the extractor itself.
fn maybe_rewrite_to_old_reddit(parsed: &mut tauri::Url) {
    let lower = match parsed.host_str() {
        Some(h) => h.to_ascii_lowercase(),
        None => return,
    };
    let needs_swap = matches!(
        lower.as_str(),
        "www.reddit.com" | "reddit.com" | "np.reddit.com" | "new.reddit.com"
    );
    if needs_swap {
        let _ = parsed.set_host(Some("old.reddit.com"));
    }
}

const VIEWPORT_FIX_SCRIPT: &str = r#"
(function() {
  function inject() {
    if (!document.head) return false;
    if (document.getElementById('__fr_viewport_fix__')) return true;
    var style = document.createElement('style');
    style.id = '__fr_viewport_fix__';
    style.textContent = "\
      html, body { max-width: 100vw !important; overflow-x: hidden !important; }\
      img, video, iframe, embed, object { max-width: 100% !important; height: auto !important; }\
      table { max-width: 100% !important; display: block; overflow-x: auto; }\
      pre, code { max-width: 100% !important; overflow-x: auto; white-space: pre-wrap; }\
      * { box-sizing: border-box; }";
    document.head.appendChild(style);
    return true;
  }
  function topscroll() {
    try { window.scrollTo(0, 0); } catch (e) {}
  }
  if (!inject()) {
    document.addEventListener('DOMContentLoaded', inject, { once: true });
  }
  document.addEventListener('DOMContentLoaded', topscroll, { once: true });
  window.addEventListener('load', topscroll, { once: true });
})();
"#;

#[tauri::command]
pub async fn get_categories(
    state: State<'_, AppState>,
) -> Result<Vec<CategoryWithForums>, String> {
    db::list_categories_with_forums(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_forum(
    forum_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<ForumRow>, String> {
    db::get_forum(&state.pool, forum_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_thread(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<ThreadRow>, String> {
    db::get_thread(&state.pool, thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_forum_threads(
    forum_id: i64,
    limit: Option<i64>,
    offset: Option<i64>,
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ThreadRow>, String> {
    let sort_mode = sort.as_deref().map(db::SortMode::from_str).unwrap_or(db::SortMode::New);
    db::list_forum_threads(
        &state.pool,
        forum_id,
        limit.unwrap_or(50),
        offset.unwrap_or(0),
        sort_mode,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn forum_supports_hot(
    forum_id: i64,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    db::forum_supports_hot(&state.pool, forum_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_category(
    category_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<crate::db::CategoryRow>, String> {
    db::get_category(&state.pool, category_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_category_threads(
    category_id: i64,
    limit: Option<i64>,
    offset: Option<i64>,
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ThreadRow>, String> {
    let sort_mode = sort.as_deref().map(db::SortMode::from_str).unwrap_or(db::SortMode::New);
    db::list_category_threads(
        &state.pool,
        category_id,
        limit.unwrap_or(50),
        offset.unwrap_or(0),
        sort_mode,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn category_thread_count(
    category_id: i64,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    db::category_thread_count(&state.pool, category_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn category_supports_hot(
    category_id: i64,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    db::category_supports_hot(&state.pool, category_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_threads(
    query: String,
    forum_id: Option<i64>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ThreadRow>, String> {
    db::search_threads(&state.pool, &query, forum_id, limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_forum_visited(
    forum_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::mark_forum_visited(&state.pool, forum_id, chrono::Utc::now().timestamp())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_all_forums_visited(state: State<'_, AppState>) -> Result<(), String> {
    db::mark_all_forums_visited(&state.pool, chrono::Utc::now().timestamp())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_thread_read(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("UPDATE threads SET read = 1 WHERE id = ?")
        .bind(thread_id)
        .execute(state.pool.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn set_last_read_post_index(
    thread_id: i64,
    post_index: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Monotonic: only advance the watermark, never rewind. Otherwise quick
    // scroll-jitter could pull the marker backwards.
    sqlx::query(
        "UPDATE threads SET last_read_post_index = MAX(COALESCE(last_read_post_index, 0), ?) WHERE id = ?",
    )
    .bind(post_index)
    .bind(thread_id)
    .execute(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_last_read_post_index(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<i64>, String> {
    let r = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT last_read_post_index FROM threads WHERE id = ?",
    )
    .bind(thread_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())?;
    Ok(r.flatten())
}

/// Round-trip YAML through AppConfig to normalize formatting. Struct-field
/// declaration order in `ForumConfig` (title first, then kind, then connection
/// fields) drives the output ordering — so a re-serialize is also the auto-
/// formatter. Returns the raw input unchanged if it doesn't parse, so the
/// editor can still display broken YAML for the user to fix.
fn format_yaml(raw: &str) -> String {
    let Ok(cfg) = crate::config::AppConfig::from_str(raw) else {
        return raw.to_string();
    };
    serde_yaml::to_string(&cfg).unwrap_or_else(|_| raw.to_string())
}

/// Read the active feeds.yaml, auto-format it, and silently write the
/// formatted version back if it differs from disk. The settings UI then
/// opens with a clean "no unsaved changes" state.
#[tauri::command]
pub async fn get_feeds_yaml(state: State<'_, AppState>) -> Result<String, String> {
    let raw = std::fs::read_to_string(&state.config_path)
        .map_err(|e| format!("reading {}: {}", state.config_path.display(), e))?;
    let formatted = format_yaml(&raw);
    if formatted != raw {
        // Best-effort silent rewrite. If this fails (permission, etc.) we
        // still return the formatted view to the user; their save will retry.
        let _ = std::fs::write(&state.config_path, &formatted);
    }
    Ok(formatted)
}

/// Persist edited feeds.yaml. Validates AND auto-formats by round-tripping
/// through AppConfig — the on-disk file always lands in canonical form
/// (title-first, struct-order). Returns the formatted text so the editor
/// can update to match what actually got written.
#[tauri::command]
pub async fn save_feeds_yaml(
    yaml: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let cfg = crate::config::AppConfig::from_str(&yaml)
        .map_err(|e| format!("invalid YAML: {}", e))?;
    let formatted = serde_yaml::to_string(&cfg)
        .map_err(|e| format!("formatting YAML: {}", e))?;
    std::fs::write(&state.config_path, &formatted)
        .map_err(|e| format!("writing {}: {}", state.config_path.display(), e))?;
    Ok(formatted)
}

/// Return the active feeds.yaml's filesystem path. Useful when the
/// settings UI wants to show the user where edits land.
#[tauri::command]
pub async fn get_feeds_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.config_path.display().to_string())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "snake_case")]
pub struct HistoryRow {
    pub thread_id: i64,
    pub title: String,
    pub source_url: String,
    pub forum_id: Option<i64>,
    pub forum_title: Option<String>,
    pub visited_at: i64,
}

#[tauri::command]
pub async fn record_thread_visit(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Pull title + url + forum context from the canonical tables. If the
    // thread row vanished (cache pruned), skip silently — there's nothing
    // useful to record.
    let row = sqlx::query(
        "SELECT t.title, t.source_url, t.forum_id, f.title as forum_title
         FROM threads t LEFT JOIN forums f ON f.id = t.forum_id
         WHERE t.id = ?",
    )
    .bind(thread_id)
    .fetch_optional(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())?;
    let Some(r) = row else { return Ok(()); };
    let title: String = sqlx::Row::get(&r, "title");
    let source_url: String = sqlx::Row::get(&r, "source_url");
    let forum_id: Option<i64> = sqlx::Row::get(&r, "forum_id");
    let forum_title: Option<String> = sqlx::Row::get(&r, "forum_title");

    // Upsert: same thread visited again just bumps visited_at + refreshes
    // any title that may have changed since the last visit.
    sqlx::query(
        "INSERT INTO thread_history(thread_id, title, source_url, forum_id, forum_title, visited_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(thread_id) DO UPDATE SET
            title = excluded.title,
            source_url = excluded.source_url,
            forum_id = excluded.forum_id,
            forum_title = excluded.forum_title,
            visited_at = excluded.visited_at",
    )
    .bind(thread_id)
    .bind(&title)
    .bind(&source_url)
    .bind(forum_id)
    .bind(forum_title)
    .bind(now)
    .execute(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())?;

    // Keep the table bounded — never more than 50 rows. UI shows top 10
    // by default; the larger tail keeps things sensible if we ever expose
    // "see more" later.
    sqlx::query(
        "DELETE FROM thread_history
         WHERE thread_id NOT IN (
             SELECT thread_id FROM thread_history
             ORDER BY visited_at DESC LIMIT 50
         )",
    )
    .execute(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_thread_history(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryRow>, String> {
    let n = limit.unwrap_or(10).clamp(1, 50);
    sqlx::query_as::<_, HistoryRow>(
        "SELECT thread_id, title, source_url, forum_id, forum_title, visited_at
         FROM thread_history ORDER BY visited_at DESC LIMIT ?",
    )
    .bind(n)
    .fetch_all(state.pool.as_ref())
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_thread_history_entry(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM thread_history WHERE thread_id = ?")
        .bind(thread_id)
        .execute(state.pool.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn clear_thread_history_all(
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM thread_history")
        .execute(state.pool.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Send a user message to the feeds-configuration agent. The agent has
/// web_search + read/validate/apply tools on the active feeds.yaml. Returns
/// the updated message history (which the frontend should pass back next turn)
/// along with a flag indicating whether feeds.yaml was modified.
#[tauri::command]
pub async fn feeds_agent_send_message(
    app: AppHandle,
    history: Vec<crate::anthropic::feeds_agent::ChatMessage>,
    message: String,
    state: State<'_, AppState>,
) -> Result<crate::anthropic::feeds_agent::AgentResult, String> {
    crate::anthropic::feeds_agent::run_agent(&app, &state.config_path, history, message)
        .await
        .map_err(|e| e.to_string())
}

/// Revert feeds.yaml to the most recent snapshot taken by the agent. Returns
/// the restored content, or null if no snapshots exist.
#[tauri::command]
pub async fn feeds_agent_revert(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    crate::anthropic::feeds_agent::revert_to_previous_snapshot(&state.config_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_forum_threads_read(
    forum_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("UPDATE threads SET read = 1 WHERE forum_id = ?")
        .bind(forum_id)
        .execute(state.pool.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- favorites ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostFavoriteSnapshot {
    pub has_post_number: bool,
    pub author: Option<String>,
    pub timestamp_raw: Option<String>,
    pub body_html: String,
    pub body_excerpt: Option<String>,
    pub thread_title: String,
    pub thread_source_url: String,
}

#[tauri::command]
pub async fn favorite_post(
    thread_id: i64,
    post_index: i64,
    snapshot: PostFavoriteSnapshot,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::favorite_post(
        &state.pool,
        thread_id,
        post_index,
        snapshot.has_post_number,
        snapshot.author.as_deref(),
        snapshot.timestamp_raw.as_deref(),
        &snapshot.body_html,
        snapshot.body_excerpt.as_deref(),
        &snapshot.thread_title,
        &snapshot.thread_source_url,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unfavorite_post(
    thread_id: i64,
    post_index: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::unfavorite_post(&state.pool, thread_id, post_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_post_favorited(
    thread_id: i64,
    post_index: i64,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    db::is_post_favorited(&state.pool, thread_id, post_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_thread_favorite_indexes(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<i64>, String> {
    db::list_thread_favorite_indexes(&state.pool, thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_favorite_posts(
    state: State<'_, AppState>,
) -> Result<Vec<db::FavoritePostRow>, String> {
    db::list_favorite_posts(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn thread_has_favorites(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    db::thread_has_favorites(&state.pool, thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn find_thread_id_by_source_url(
    url: String,
    state: State<'_, AppState>,
) -> Result<Option<i64>, String> {
    db::find_thread_id_by_source_url(&state.pool, &url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn refresh_forum(
    forum_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let forum = db::get_forum(&state.pool, forum_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "forum not found".to_string())?;
    // Find the matching ForumConfig in the loaded YAML by title
    let cfg = state
        .config
        .categories
        .iter()
        .flat_map(|c| c.forums.iter())
        .find(|f| f.resolved_title() == forum.title)
        .cloned()
        .ok_or_else(|| "forum config not found".to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let cookies = state.cookies.clone();
    match crate::forums::fetch_forum(&client, &cfg, Some(&cookies)).await {
        Ok(threads) => {
            for t in &threads {
                let _ = db::upsert_thread(&state.pool, forum_id, t).await;
            }
            let _ = db::refresh_forum_stats(
                &state.pool,
                forum_id,
                chrono::Utc::now().timestamp(),
            )
            .await;
            Ok(())
        }
        Err(e) => {
            let _ = db::record_forum_error(&state.pool, forum_id, &e.to_string()).await;
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn refresh_all(state: State<'_, AppState>) -> Result<(), String> {
    poller::poll_all_once(&state.pool, &state.config, Some(state.cookies.clone()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_thread_webview(
    app: AppHandle,
    url: String,
    thread_id: Option<i64>,
    platform_hint: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut parsed: tauri::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;

    // WHY: vBulletin/phpBB feed URLs often include "#postN" anchors that auto-scroll
    // the browser mid-thread on load. Strip the fragment so the embedded webview always
    // opens at the natural top of the source page.
    parsed.set_fragment(None);

    // WHY: new-reddit's React DOM is too volatile to learn against; old.reddit
    // is server-rendered HTML with a stable structure. URL transform only.
    maybe_rewrite_to_old_reddit(&mut parsed);

    // Close any existing thread webview first.
    close_existing_thread_webview(&app);

    let window = app
        .get_window("main")
        .ok_or_else(|| "main window not found".to_string())?;

    let inner = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let logical_w = (inner.width as f64) / scale;
    let logical_h = (inner.height as f64) / scale;

    // Capture context for the on_page_load closure — it must own everything 'static.
    let pool = state.pool.clone();
    let app_handle = app.clone();
    let platform_hint_owned = platform_hint.clone();

    // Capture the current theme into the init script so the source page boots
    // in the right color-scheme. The Rust state was set by the frontend's
    // setWebviewColorScheme call on app startup (and on every subsequent
    // toggle), so on first page load this matches whatever the user is
    // currently looking at in the app shell.
    let is_dark = state
        .webview_dark
        .load(std::sync::atomic::Ordering::Relaxed);
    let scheme_script = if is_dark {
        COLOR_SCHEME_DARK_JS
    } else {
        COLOR_SCHEME_LIGHT_JS
    };

    let builder = WebviewBuilder::new(THREAD_WEBVIEW_LABEL, WebviewUrl::External(parsed.clone()))
        .user_agent(crate::forums::USER_AGENT)
        .initialization_script(VIEWPORT_FIX_SCRIPT)
        .initialization_script(scheme_script)
        .on_page_load(move |webview, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished) {
                return;
            }
            // Mute media preemptively. If the user ends up in webview mode, the
            // frontend will call restore_thread_webview which unmutes. If they
            // stay in native mode, shrink_thread_webview will also re-apply
            // mute (idempotent via window.__fr_force_mute flag).
            let _ = webview.eval(MUTE_PAGE_JS);

            let pool = pool.clone();
            let app_handle = app_handle.clone();
            let hint = platform_hint_owned.clone();
            let thread_id = thread_id;
            let _ = webview.eval_with_callback(SCRAPE_RETURN_JS, move |json| {
                // The callback is sync; spin up an async task to run the pipeline.
                let pool = pool.clone();
                let app_handle = app_handle.clone();
                let hint = hint.clone();
                tauri::async_runtime::spawn(async move {
                    process_scrape_payload(&app_handle, &pool, &json, thread_id, hint.as_deref())
                        .await;
                });
            });
        });

    let pos = LogicalPosition::new(0.0, TOOLBAR_HEIGHT);
    let size = LogicalSize::new(logical_w, (logical_h - TOOLBAR_HEIGHT).max(100.0));

    let webview = window
        .add_child(builder, pos, size)
        .map_err(|e| e.to_string())?;

    // WHY: inject Chrome cookies for the destination host so authenticated pages
    // (FlyerTalk Sponsors, BimmerPost logged-in views, Reddit personalized feed)
    // render the same as in the user's browser. Non-fatal if any one cookie fails.
    let parsed_url: url::Url = parsed.as_str().parse().map_err(|e: url::ParseError| e.to_string())?;
    let cookies_to_set = cookies::webview_cookies_for_url(&parsed_url, &state.cookies);
    for c in cookies_to_set {
        let _ = webview.set_cookie(c);
    }

    Ok(())
}

/// eval_with_callback delivers a JSON-string-of-JSON (the JS expression's
/// return value, serialized by Tauri once more). Unwrap both layers, then
/// hand off to the scrape pipeline.
async fn process_scrape_payload(
    app: &AppHandle,
    pool: &Arc<SqlitePool>,
    raw_outer: &str,
    thread_id: Option<i64>,
    platform_hint: Option<&str>,
) {
    // outer layer: a serialized JS string. parse to get the inner JSON.
    let inner: String = match serde_json::from_str(raw_outer) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err=%e, "scrape callback outer-decode failed");
            return;
        }
    };
    let sample = match scrape::decode_message(&inner) {
        Some(s) => s,
        None => return,
    };
    match scrape::handle_sample(
        pool,
        &sample.host,
        &sample.url,
        &sample.html,
        thread_id,
        platform_hint,
    )
    .await
    {
        Ok(Some(payload)) => {
            let _ = app.emit("posts_ready", &payload);

            // Agent-driven media expansion: if the active profile identified
            // expand-button selectors and we haven't already clicked them for
            // this thread in this session, click + re-scrape so the next
            // posts_ready carries the expanded media. Fire-and-forget — the
            // helper spawns its own task to avoid Send issues with the tauri
            // Webview handle.
            if let Some(tid) = thread_id {
                maybe_expand_media_then_rescrape(app, pool, &sample.host, tid);
            }

            // Agent-driven avatar enrichment: when the profile populated the
            // author_profile_url_template trio, fetch avatars off-thread.
            // Cached per (host, author) so future threads reuse.
            spawn_avatar_enrichment(app, pool, &sample.host, &payload);
        }
        Ok(None) => {
            // No profile + no learner; frontend keeps the webview as the visible UI.
            let _ = app.emit("posts_unavailable", &sample.host);
        }
        Err(e) => {
            tracing::warn!(err=%e, host=sample.host, "scrape pipeline failed");
            let _ = app.emit("posts_unavailable", &sample.host);
        }
    }
}

/// If the active profile for `host` has any `media_expand_selectors` and we
/// haven't already done expansion for `thread_id`, drive the live webview to
/// click all matches, wait, and re-eval SCRAPE_RETURN_JS so the next
/// process_scrape_payload pass captures the expanded DOM. Bounded to one
/// click-pass per thread per session.
///
/// Implementation note: the actual click+sleep+rescrape runs inside a
/// separately-spawned task so the !Send tauri Webview handle never crosses
/// the await boundary in the caller's task. Returns immediately after
/// scheduling.
fn maybe_expand_media_then_rescrape(
    app: &AppHandle,
    pool: &Arc<SqlitePool>,
    host: &str,
    thread_id: i64,
) {
    // Reservation pass: bail if already expanded for this thread in this session.
    let expanded_set = {
        let state: tauri::State<'_, AppState> = app.state();
        state.expanded_threads.clone()
    };
    {
        let mut guard = expanded_set.lock().unwrap();
        if guard.contains(&thread_id) {
            return;
        }
        guard.insert(thread_id);
    }

    let app = app.clone();
    let pool = pool.clone();
    let host = host.to_string();
    tauri::async_runtime::spawn(async move {
        let profiles = match db::list_site_profiles_for_host(&pool, &host).await {
            Ok(v) => v,
            Err(_) => return,
        };
        // Pick the profile whose probe currently matches AND that has
        // expand selectors. This is the profile we'll re-extract with
        // after expansion, skipping handle_sample's probe-matching to
        // avoid an unnecessary (rate-limited) re-learn.
        let Some((sel, content_type_owned)) = profiles.iter().find_map(|p| {
            let s = scrape::parse_selectors(&p.selectors_json).ok()?;
            if s.media_expand_selectors.is_empty() {
                None
            } else {
                Some((s, p.content_type.clone()))
            }
        }) else {
            return;
        };
        let expand_selectors = sel.media_expand_selectors.clone();
        let Some(webview) = app.get_webview(THREAD_WEBVIEW_LABEL) else {
            return;
        };

        let selectors_json = serde_json::to_string(&expand_selectors).unwrap_or("[]".to_string());
        let click_js = format!(
            r#"(function(){{var sels={};sels.forEach(function(s){{try{{document.querySelectorAll(s).forEach(function(el){{try{{el.click();}}catch(e){{}}}});}}catch(e){{}}}});}})()"#,
            selectors_json
        );
        let _ = webview.eval(&click_js);

        // Page time to fetch/render expanded media (image lazyload, embed mount).
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        // Re-mute in case any expanded media starts auto-playing.
        let _ = webview.eval(MUTE_PAGE_JS);

        // Re-capture and extract DIRECTLY with the known profile — NO probe
        // matching, NO re-learn. Avoids the rate-limit loop that happens when
        // expander clicks change the DOM enough that the original probe stops
        // matching, which used to trigger a fresh anthropic::learn call.
        let app_handle = app.clone();
        let pool_extract = pool.clone();
        let host_extract = host.clone();
        let _ = webview.eval_with_callback(SCRAPE_RETURN_JS, move |json| {
            let pool = pool_extract.clone();
            let app_handle = app_handle.clone();
            let host = host_extract.clone();
            let selectors = sel.clone();
            let content_type = content_type_owned.clone();
            tauri::async_runtime::spawn(async move {
                reextract_with_known_profile(&app_handle, &pool, &host, thread_id, &selectors, &content_type, &json).await;
            });
        });
    });
}

/// Kick off avatar enrichment for the freshly-rendered posts. Drives the
/// LIVE WebView (already authenticated as the user, same-origin to the
/// thread page) to fetch each unique author's profile and extract their
/// avatar via the agent-supplied locator. Cached per (host, author) so
/// future threads on this host reuse the result. Emits `avatars_resolved`
/// with the (author, avatar_url) list — frontend patches rendered posts
/// by author name.
fn spawn_avatar_enrichment(
    app: &AppHandle,
    pool: &Arc<SqlitePool>,
    host: &str,
    payload: &scrape::PostsReadyPayload,
) {
    let app = app.clone();
    let pool = pool.clone();
    let host = host.to_string();
    let posts = payload.posts.clone();
    let content_type = payload.content_type.clone();
    tauri::async_runtime::spawn(async move {
        let profile = match db::get_site_profile(&pool, &host, &content_type).await {
            Ok(Some(p)) => p,
            _ => return,
        };
        let Ok(selectors) = scrape::parse_selectors(&profile.selectors_json) else {
            return;
        };

        // Pull anything already cached so we emit it immediately and skip its
        // network round-trip.
        let cached = crate::enrichment::load_cached(&pool, &host, &posts).await;

        // Build the dispatch JS for uncached authors. Returns None when
        // there's nothing new to fetch (everyone is already cached).
        let dispatch = crate::enrichment::build_dispatch_js(&selectors, &posts, &cached);

        let Some(webview) = app.get_webview(THREAD_WEBVIEW_LABEL) else {
            return;
        };

        // If there are existing cached avatars, emit them right away so the
        // user sees avatars instantly on revisits.
        if !cached.is_empty() {
            let immediate = crate::enrichment::persist_and_collect(
                &pool,
                &host,
                &std::collections::HashMap::new(),
                &cached,
            )
            .await;
            if !immediate.is_empty() {
                let _ = app.emit(
                    "avatars_resolved",
                    &crate::enrichment::AvatarsResolvedPayload {
                        host: host.clone(),
                        updates: immediate,
                    },
                );
            }
        }

        let Some(dispatch_js) = dispatch else {
            return; // nothing fresh to fetch
        };
        // Ensure the avatars-results object exists, then fire all fetches.
        let _ = webview.eval("window.__fr_avatars = window.__fr_avatars || {};");
        let _ = webview.eval(&dispatch_js);

        tokio::time::sleep(crate::enrichment::dispatch_delay()).await;

        // Read the results back. eval_with_callback's callback is sync but
        // we want async-style flow; bridge via tokio oneshot.
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let tx = std::sync::Mutex::new(Some(tx));
        let _ = webview.eval_with_callback(crate::enrichment::READ_AVATARS_JS, move |s| {
            if let Ok(mut g) = tx.lock() {
                if let Some(t) = g.take() {
                    let _ = t.send(s);
                }
            }
        });
        let raw = match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(s)) => s,
            _ => return,
        };
        let fresh = crate::enrichment::decode_results(&raw);
        if fresh.is_empty() {
            return;
        }
        let updates = crate::enrichment::persist_and_collect(
            &pool,
            &host,
            &fresh,
            &std::collections::HashMap::new(),
        )
        .await;
        if !updates.is_empty() {
            let _ = app.emit(
                "avatars_resolved",
                &crate::enrichment::AvatarsResolvedPayload {
                    host: host.clone(),
                    updates,
                },
            );
        }
    });
}

/// Used by maybe_expand_media_then_rescrape. Decodes the post-expansion
/// scrape sample, applies the KNOWN profile's selectors, and emits a fresh
/// posts_ready. Skips handle_sample's probe-matching (which would otherwise
/// re-learn — burning a learn API call) and skips the critique pass.
async fn reextract_with_known_profile(
    app: &AppHandle,
    pool: &Arc<SqlitePool>,
    host: &str,
    thread_id: i64,
    selectors: &scrape::SelectorSet,
    content_type: &str,
    raw_outer: &str,
) {
    let inner: String = match serde_json::from_str(raw_outer) {
        Ok(s) => s,
        Err(_) => return,
    };
    let sample = match scrape::decode_message(&inner) {
        Some(s) => s,
        None => return,
    };
    let posts = match scrape::extract::apply(&sample.html, selectors, Some(&sample.url)) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(err=%e, "post-expansion extract failed");
            return;
        }
    };
    if posts.is_empty() {
        tracing::debug!(host, "post-expansion extract yielded zero posts; not emitting");
        return;
    }
    if let Ok(json) = serde_json::to_string(&posts) {
        let _ = db::cache_posts(pool, thread_id, &json).await;
    }
    let payload = scrape::PostsReadyPayload {
        host: host.to_string(),
        url: sample.url,
        posts,
        source: "anthropic".to_string(),
        content_type: content_type.to_string(),
    };
    let _ = app.emit("posts_ready", &payload);

    // After expansion-driven re-scrape we ALSO need to re-fire avatar
    // enrichment, otherwise the freshly-replaced posts array loses the
    // patched avatar/profile fields and the user sees silhouettes flash back.
    // spawn_avatar_enrichment hits the per-author cache first so this is
    // typically a same-cycle re-emit with no extra HTTP.
    spawn_avatar_enrichment(app, pool, host, &payload);
}

// ---------- native-render commands surfaced to the frontend ----------

#[derive(Serialize)]
pub struct SiteProfileSummary {
    pub host: String,
    pub content_type: String,
    pub content_probe: String,
    pub source: String,
    pub success_count: i64,
    pub fail_count: i64,
    pub last_validated_at: i64,
}

#[derive(Serialize)]
pub struct CachedPostsResponse {
    pub posts: serde_json::Value,
    pub scraped_at: i64,
}

#[derive(Deserialize)]
pub struct RenderPreferenceUpdate {
    pub host: String,
    /// "native" | "webview"
    pub mode: String,
}

#[tauri::command]
pub async fn get_site_profile(
    host: String,
    state: State<'_, AppState>,
) -> Result<Vec<SiteProfileSummary>, String> {
    let rows = db::list_site_profiles_for_host(&state.pool, &host)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| SiteProfileSummary {
            host: r.host,
            content_type: r.content_type,
            content_probe: r.content_probe,
            source: r.source,
            success_count: r.success_count,
            fail_count: r.fail_count,
            last_validated_at: r.last_validated_at,
        })
        .collect())
}

#[tauri::command]
pub async fn get_cached_posts(
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<Option<CachedPostsResponse>, String> {
    let result = db::get_cached_posts(&state.pool, thread_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(result.and_then(|(json, ts)| {
        serde_json::from_str::<serde_json::Value>(&json).ok().map(|posts| CachedPostsResponse {
            posts,
            scraped_at: ts,
        })
    }))
}

#[tauri::command]
pub async fn set_render_preference(
    update: RenderPreferenceUpdate,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Stored as a tiny JSON map under one key so reads are atomic.
    let key = "native_render_per_host";
    let raw = db::get_pref(&state.pool, key).await.map_err(|e| e.to_string())?;
    let mut map: serde_json::Map<String, serde_json::Value> = match raw {
        Some(s) => serde_json::from_str(&s).unwrap_or_default(),
        None => serde_json::Map::new(),
    };
    map.insert(update.host, serde_json::Value::String(update.mode));
    let s = serde_json::to_string(&map).map_err(|e| e.to_string())?;
    db::set_pref(&state.pool, key, &s).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_render_preferences(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let raw = db::get_pref(&state.pool, "native_render_per_host")
        .await
        .map_err(|e| e.to_string())?;
    Ok(raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({})))
}

/// Trigger lazy-load on the active thread webview. Looks up the active
/// site_profile's `load_more_strategy`, injects the right JS (click button,
/// scroll, or navigate), waits ~2.5s for the DOM to settle, re-scrapes, and
/// emits `posts_ready` with the new full post list. The frontend's existing
/// posts_ready listener replaces the rendered posts.
#[tauri::command]
pub async fn load_more_posts(
    app: AppHandle,
    thread_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let webview = app
        .get_webview(THREAD_WEBVIEW_LABEL)
        .ok_or_else(|| "no thread webview is alive".to_string())?;

    let thread = db::get_thread(&state.pool, thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())?;

    // Resolve the host (post URL-rewrite, e.g. reddit.com → old.reddit.com).
    let mut parsed: tauri::Url = thread
        .source_url
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;
    maybe_rewrite_to_old_reddit(&mut parsed);
    let host = parsed
        .host_str()
        .ok_or_else(|| "thread url has no host".to_string())?
        .to_string();

    // Pick the first profile for this host that has a non-"none" strategy.
    let profiles = db::list_site_profiles_for_host(&state.pool, &host)
        .await
        .map_err(|e| e.to_string())?;
    let pick = profiles.iter().find_map(|p| {
        let s = scrape::parse_selectors(&p.selectors_json).ok()?;
        if s.load_more_strategy != "none" {
            Some((s, p.content_type.clone()))
        } else {
            None
        }
    });
    let Some((sel, _ct)) = pick else {
        // Nothing to do — silently no-op so the frontend can call this
        // unconditionally without checking strategy upfront.
        return Ok(());
    };

    let trigger_js = match sel.load_more_strategy.as_str() {
        "click_button" => {
            let s = sel.load_more_selector.unwrap_or_default();
            if s.trim().is_empty() {
                return Ok(());
            }
            // .click() each match — Reddit's ".morecomments" can appear many
            // times; clicking the FIRST one near the bottom is typical.
            format!(
                r#"(function(){{var els=document.querySelectorAll({});var visible=Array.from(els).filter(function(e){{var r=e.getBoundingClientRect();return r.top<window.innerHeight*2;}});if(visible.length>0){{visible[0].click();}}else if(els.length>0){{els[0].click();}}}})()"#,
                serde_json::to_string(&s).unwrap_or("\"\"".to_string())
            )
        }
        "infinite_scroll" => {
            "(function(){window.scrollTo(0, document.body.scrollHeight);})()".to_string()
        }
        "next_page" => {
            // We resolve pagination_next URL in JS so relative hrefs work.
            let next_sel = sel.pagination_next.unwrap_or_default();
            if next_sel.trim().is_empty() {
                return Ok(());
            }
            format!(
                r#"(function(){{var a=document.querySelector({});if(a&&a.href){{window.location.href=a.href;}}}})()"#,
                serde_json::to_string(&next_sel).unwrap_or("\"\"".to_string())
            )
        }
        _ => return Ok(()),
    };

    webview.eval(&trigger_js).map_err(|e| e.to_string())?;
    // Wait for DOM to settle. 2.5s is enough for Reddit's morecomments XHR
    // and for most forum page navigations.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    // Re-scrape and process — handle_sample (via process_scrape_payload) will
    // emit posts_ready, which the frontend's listener appends/replaces.
    let app_handle = app.clone();
    let pool = state.pool.clone();
    let hint = thread.op_author.clone(); // unused, just for type
    let _ = hint;
    let thread_id_owned = thread_id;
    webview
        .eval_with_callback(SCRAPE_RETURN_JS, move |json| {
            let pool = pool.clone();
            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                process_scrape_payload(&app_handle, &pool, &json, Some(thread_id_owned), None).await;
            });
        })
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// The "Re-learn" / "Clear layout cache" UI button hits this. Behavior:
///
/// 1. If we ship a bundled seed for this host, restore the seed selectors
///    (delete the existing profile, write the seed). This is the right
///    answer for the seeded set — the seeds are hand-verified and reliably
///    better than what a one-shot Anthropic call produces.
/// 2. Otherwise, delete the profile. The next page load will trigger a
///    fresh Anthropic learn (current behavior pre-seed).
///
/// WHY: the old behavior was unconditional delete, which on seeded hosts
/// destroyed the bundled selectors and forced a fresh anthropic call that
/// produced the same buggy output every time (e.g. autopia.org learner
/// missed the `img` suffix on the avatar selector). The user's actual
/// intent when clicking "Re-learn" is "give me the best known selectors
/// and re-scrape" — not "burn my best profile and pay for a new API call."
#[tauri::command]
pub async fn relearn_site_profile(
    host: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let seeds = scrape::seed::find_seeds_for_host(&host);
    db::delete_site_profile(&state.pool, &host)
        .await
        .map_err(|e| e.to_string())?;
    if !seeds.is_empty() {
        for s in &seeds {
            db::upsert_site_profile(
                &state.pool,
                &s.host,
                &s.content_type,
                &s.content_probe,
                &s.selectors_json,
                "seed",
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        tracing::info!(
            host,
            seed_count = seeds.len(),
            "relearn: restored bundled seed(s) for host instead of running learner"
        );
    } else {
        tracing::info!(host, "relearn: no bundled seed for host — fresh anthropic learn will run on next visit");
    }
    Ok(())
}

// ---------- debugger refinement commands ----------

// WHY: Tauri auto-converts top-level command args (host → host, contentType →
// content_type) but NOT fields of nested structs. The frontend's invoke()
// passes a `req` object with camelCase keys, so we tell serde to accept them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefineRequest {
    pub host: String,
    pub content_type: String,
    pub field_name: String,
    pub element_html: String,
    pub complaint: String,
}

#[derive(Serialize)]
pub struct RefineResponse {
    pub revision_id: i64,
    pub content_type: String,
    pub selectors_json: String,
}

#[tauri::command]
pub async fn refine_site_profile(
    req: RefineRequest,
    state: State<'_, AppState>,
) -> Result<RefineResponse, String> {
    let existing = db::get_site_profile(&state.pool, &req.host, &req.content_type)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no existing profile to refine for ({}, {})", req.host, req.content_type))?;

    let learned = crate::anthropic::refine(
        &req.host,
        &existing.selectors_json,
        &req.element_html,
        &req.field_name,
        &req.complaint,
    )
    .await
    .map_err(|e| e.to_string())?;

    // The model is allowed to update content_type / content_probe along the
    // way — if it does, persist under the new (host, content_type) key.
    let mut sel = learned.selectors;
    if sel.content_type.trim().is_empty() {
        sel.content_type = req.content_type.clone();
    }
    let selectors_json = serde_json::to_string(&sel).map_err(|e| e.to_string())?;
    let revision_id = db::insert_profile_revision(
        &state.pool,
        &req.host,
        &sel.content_type,
        &sel.content_probe,
        &selectors_json,
        "manual",
        existing.current_revision_id,
        Some(&req.complaint),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(RefineResponse {
        revision_id,
        content_type: sel.content_type,
        selectors_json,
    })
}

#[derive(Serialize)]
pub struct ProfileRevisionDto {
    pub id: i64,
    pub host: String,
    pub content_type: String,
    pub parent_revision_id: Option<i64>,
    pub source: String,
    pub user_complaint: Option<String>,
    pub created_at: i64,
    pub is_current: bool,
}

#[tauri::command]
pub async fn list_site_profile_revisions(
    host: String,
    content_type: String,
    state: State<'_, AppState>,
) -> Result<Vec<ProfileRevisionDto>, String> {
    let profile = db::get_site_profile(&state.pool, &host, &content_type)
        .await
        .map_err(|e| e.to_string())?;
    let current = profile.and_then(|p| p.current_revision_id);
    let revs = db::list_profile_revisions(&state.pool, &host, Some(&content_type))
        .await
        .map_err(|e| e.to_string())?;
    Ok(revs
        .into_iter()
        .map(|r| ProfileRevisionDto {
            is_current: Some(r.id) == current,
            id: r.id,
            host: r.host,
            content_type: r.content_type,
            parent_revision_id: r.parent_revision_id,
            source: r.source,
            user_complaint: r.user_complaint,
            created_at: r.created_at,
        })
        .collect())
}

#[tauri::command]
pub async fn undo_site_profile_revision(
    host: String,
    content_type: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let profile = db::get_site_profile(&state.pool, &host, &content_type)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no profile to undo for ({host}, {content_type})"))?;
    let current_id = match profile.current_revision_id {
        Some(id) => id,
        None => return Ok(false),
    };
    let current = db::get_profile_revision(&state.pool, current_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "current revision row not found".to_string())?;
    let Some(parent) = current.parent_revision_id else {
        return Ok(false);
    };
    db::set_current_revision(&state.pool, &host, &content_type, parent)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn reset_site_profile(
    host: String,
    content_type: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let revs = db::list_profile_revisions(&state.pool, &host, Some(&content_type))
        .await
        .map_err(|e| e.to_string())?;
    let Some(first) = revs.into_iter().find(|r| r.parent_revision_id.is_none()) else {
        return Ok(false);
    };
    db::set_current_revision(&state.pool, &host, &content_type, first.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn close_thread_webview(app: AppHandle) -> Result<(), String> {
    close_existing_thread_webview(&app);
    Ok(())
}

/// Shrink the thread webview to a 1x1 sliver in the corner instead of closing
/// it. The page stays loaded so we can later drive it for lazy-load (clicking
/// "load more" buttons, scrolling, navigating to next pages). The Svelte
/// NativeThreadView covers the whole content area so the user never sees this.
///
/// Also mutes every audio/video element on the page so the hidden webview
/// doesn't auto-play sound (Reddit clips, embedded YouTube, etc.) while the
/// user is reading the native render.
#[tauri::command]
pub async fn shrink_thread_webview(app: AppHandle) -> Result<(), String> {
    if let Some(wv) = app.get_webview(THREAD_WEBVIEW_LABEL) {
        let _ = wv.set_position(tauri::LogicalPosition::new(0.0, 0.0));
        let _ = wv.set_size(tauri::LogicalSize::new(1.0, 1.0));
        let _ = wv.eval(MUTE_PAGE_JS);
    }
    Ok(())
}

/// JS injected when the webview is shrunken: pause + mute every <audio> /
/// <video> on the page and re-apply on any future media element via a
/// MutationObserver. Keeps a flag (window.__fr_force_mute) so the install runs
/// only once. Survives SPA-style dynamic media insertion.
const MUTE_PAGE_JS: &str = r#"
(function() {
  try {
    if (window.__fr_force_mute === true) {
      // Already installed — just re-apply to current elements.
    } else {
      window.__fr_force_mute = true;
      var mo = new MutationObserver(function() {
        document.querySelectorAll('audio, video').forEach(function(el) {
          try { el.muted = true; el.pause && el.pause(); } catch (e) {}
        });
      });
      mo.observe(document.documentElement, { childList: true, subtree: true });
      window.__fr_force_mute_observer = mo;
    }
    document.querySelectorAll('audio, video').forEach(function(el) {
      try { el.muted = true; el.pause && el.pause(); } catch (e) {}
    });
  } catch (e) {}
})()
"#;

/// JS injected when the webview is restored to full size: unmute media so the
/// user can hear audio in the original site again. Disconnects the
/// MutationObserver so future content isn't auto-muted.
const UNMUTE_PAGE_JS: &str = r#"
(function() {
  try {
    if (window.__fr_force_mute_observer) {
      window.__fr_force_mute_observer.disconnect();
      window.__fr_force_mute_observer = null;
    }
    window.__fr_force_mute = false;
    document.querySelectorAll('audio, video').forEach(function(el) {
      try { el.muted = false; } catch (e) {}
    });
  } catch (e) {}
})()
"#;

/// Restore the thread webview to full size below the toolbar. Used when the
/// user toggles back to "Webview" mode.
#[tauri::command]
pub async fn restore_thread_webview(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let inner = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let logical_w = (inner.width as f64) / scale;
    let logical_h = (inner.height as f64) / scale;
    if let Some(wv) = app.get_webview(THREAD_WEBVIEW_LABEL) {
        let _ = wv.set_position(tauri::LogicalPosition::new(0.0, TOOLBAR_HEIGHT));
        let _ = wv.set_size(tauri::LogicalSize::new(
            logical_w,
            (logical_h - TOOLBAR_HEIGHT).max(100.0),
        ));
        let _ = wv.eval(UNMUTE_PAGE_JS);
    }
    Ok(())
}

fn close_existing_thread_webview(app: &AppHandle) {
    if let Some(wv) = app.get_webview(THREAD_WEBVIEW_LABEL) {
        let _ = wv.close();
    }
}

/// Apply a zoom factor to both the main app webview AND the child thread
/// webview (when present). Clamped to a sane range so a fat-fingered pinch
/// can't render the UI unreadable. Driven from the frontend in response to
/// ⌘/Ctrl + `=`/`-`/`0` and trackpad pinch (ctrlKey wheel) events.
#[tauri::command]
pub async fn set_zoom_level(app: AppHandle, level: f64) -> Result<f64, String> {
    let clamped = level.clamp(0.25, 5.0);
    if let Some(wv) = app.get_webview("main") {
        let _ = wv.set_zoom(clamped);
    }
    if let Some(wv) = app.get_webview(THREAD_WEBVIEW_LABEL) {
        let _ = wv.set_zoom(clamped);
    }
    Ok(clamped)
}

/// JS that flips the embedded source-site page into dark mode by setting
/// `<html style="color-scheme: dark">` and inserting a matching
/// `<meta name="color-scheme" content="dark">`. Sites that honor
/// `prefers-color-scheme` (modern Reddit, Discourse, recent XenForo) will
/// follow; sites that don't (old.reddit, classic vBulletin 3.x) stay
/// unchanged — which is the same outcome as not injecting at all.
const COLOR_SCHEME_DARK_JS: &str = r#"
(function() {
  try {
    document.documentElement.style.colorScheme = 'dark';
    var existing = document.querySelector('meta[name="color-scheme"]#__fr_color_scheme__');
    if (!existing) {
      var m = document.createElement('meta');
      m.id = '__fr_color_scheme__';
      m.setAttribute('name', 'color-scheme');
      m.setAttribute('content', 'dark light');
      (document.head || document.documentElement).appendChild(m);
    } else {
      existing.setAttribute('content', 'dark light');
    }
  } catch (e) {}
})()
"#;

const COLOR_SCHEME_LIGHT_JS: &str = r#"
(function() {
  try {
    document.documentElement.style.colorScheme = 'light';
    var existing = document.querySelector('meta[name="color-scheme"]#__fr_color_scheme__');
    if (existing) existing.setAttribute('content', 'light dark');
  } catch (e) {}
})()
"#;

/// Record the current app theme and reflect it onto the live thread webview
/// (if any). Called from the frontend whenever the user flips the theme
/// toggle. Newly opened thread webviews pick up the stored state via the
/// `initialization_script` path in `open_thread_webview`.
#[tauri::command]
pub async fn set_webview_color_scheme(
    app: AppHandle,
    state: State<'_, AppState>,
    dark: bool,
) -> Result<(), String> {
    state
        .webview_dark
        .store(dark, std::sync::atomic::Ordering::Relaxed);
    if let Some(wv) = app.get_webview(THREAD_WEBVIEW_LABEL) {
        let js = if dark { COLOR_SCHEME_DARK_JS } else { COLOR_SCHEME_LIGHT_JS };
        let _ = wv.eval(js);
    }
    Ok(())
}
