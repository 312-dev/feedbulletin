// WHY: the Anthropic SelectorSet tool definition uses a nested serde_json::json!{}
// that exceeds the default 128-deep macro recursion limit as the schema grows.
#![recursion_limit = "512"]

use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tracing::{info, warn};

// `anthropic`, `db`, and `scrape` are surfaced as `pub` so the in-app WebView
// smoke test in `examples/learn_old_reddit_smoketest.rs` can drive the full
// pipeline end-to-end without going through the Tauri command bridge.
pub mod anthropic;
mod commands;
mod config;
mod cookies;
pub mod db;
pub mod enrichment;
mod forums;
mod poller;
pub mod scrape;

pub use config::{AppConfig, CategoryConfig, ForumConfig, ForumKind};
pub use db::{CategoryRow, CategoryWithForums, ForumRow, ThreadInput, ThreadRow};

pub mod test_exports {
    use crate::db::ThreadInput;
    use anyhow::Result;
    use sqlx::{Row, SqlitePool};

    pub fn reddit_parse(body: &str) -> Result<Vec<ThreadInput>> {
        crate::forums::reddit::parse(body)
    }
    pub fn xenforo_parse(body: &str) -> Result<Vec<ThreadInput>> {
        crate::forums::xenforo::parse(body)
    }
    pub fn generic_rss_parse(body: &str) -> Result<Vec<ThreadInput>> {
        crate::forums::generic_rss::parse(body)
    }
    pub async fn open_test_pool() -> Result<SqlitePool> {
        crate::db::open_pool("sqlite::memory:").await
    }
    pub async fn schema_table_names(pool: &SqlitePool) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| r.get::<String, _>("name"))
            .collect())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .try_init();

    bootstrap_anthropic_key_from_1password();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            // WHY: on resize/DPR change the child webview's bounds become stale.
            // Recompute using the SAME TOOLBAR_HEIGHT the open path used so the
            // viewport stays flush below our HTML header (28px brand + 40px nav,
            // plus macOS title-bar offset on macOS).
            //
            // CRITICAL: only resize when the webview is currently full-size.
            // When the user is in native-render mode we shrink it to 1×1 so
            // the Svelte view is visible above it; clobbering that back to
            // full size on every resize event would hide the native render
            // behind the live site again. The 64px threshold separates
            // "shrunken-for-native" (1×1) from "full-webview-mode".
            if let tauri::WindowEvent::Resized(_) = event {
                if window.label() != "main" {
                    return;
                }
                if let Some(wv) = window.app_handle().get_webview("thread_view") {
                    let currently_shrunken = wv
                        .size()
                        .ok()
                        .map(|s| s.width < 64)
                        .unwrap_or(false);
                    if currently_shrunken {
                        return;
                    }
                    if let (Ok(inner), Ok(scale)) =
                        (window.inner_size(), window.scale_factor())
                    {
                        let logical_w = (inner.width as f64) / scale;
                        let logical_h = (inner.height as f64) / scale;
                        let top = commands::TOOLBAR_HEIGHT;
                        let _ = wv.set_position(tauri::LogicalPosition::new(0.0, top));
                        let _ = wv.set_size(tauri::LogicalSize::new(
                            logical_w,
                            (logical_h - top).max(100.0),
                        ));
                    }
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // WHY: macOS convention — config + db live under
            // ~/Library/Application Support/<bundle-id>/. We still honor an
            // in-repo feeds.yaml when present so `cargo tauri dev` keeps
            // editing the repo file; otherwise we seed app-data with a
            // bundled default on first run.
            let project_root = locate_project_root();
            let project_config = config::locate_config(&project_root);

            let app_data = handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| project_root.clone());
            std::fs::create_dir_all(&app_data).ok();
            let app_data_config = app_data.join("feeds.yaml");

            let config_path = if project_config.exists() {
                project_config
            } else if app_data_config.exists() {
                app_data_config
            } else {
                // First run of an installed app: seed app-data with the
                // bundled default so the user has a starting point they can
                // edit at ~/Library/Application Support/<bundle-id>/feeds.yaml.
                const BUNDLED_FEEDS_YAML: &str = include_str!("../../feeds.yaml");
                if let Err(e) = std::fs::write(&app_data_config, BUNDLED_FEEDS_YAML) {
                    warn!(err = %e, path = %app_data_config.display(),
                          "could not seed feeds.yaml in app data dir");
                }
                app_data_config
            };
            info!(path = %config_path.display(), "loading feeds.yaml");

            let cfg = match config::AppConfig::from_path(&config_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(err = %e, "could not load feeds.yaml; starting empty");
                    config::AppConfig { categories: vec![] }
                }
            };
            let cfg = Arc::new(cfg);

            let db_path = app_data.join("forum-reader.sqlite");
            let db_url = format!("sqlite://{}", db_path.display());
            info!(url = %db_url, "opening database");

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .build()
                .expect("tokio runtime");

            let cookie_cache = cookies::empty_cache();

            let pool = rt.block_on(async {
                let pool = db::open_pool(&db_url).await.expect("open sqlite");
                poller::seed_from_config(&pool, &cfg)
                    .await
                    .expect("seed config");

                // Insert bundled site-profile seeds for well-known hosts that
                // the user hasn't already learned/refined themselves. Best-
                // effort: log and continue if anything fails.
                match scrape::seed::seed_missing_profiles(&pool).await {
                    Ok(0) => {}
                    Ok(n) => info!("seeded {n} bundled site profile(s)"),
                    Err(e) => warn!("seeding bundled site profiles failed: {e:#}"),
                }

                // WHY: best-effort warmup of the cookie cache so the first poll has any
                // saved cookies. Cookie persistence failures are non-fatal; we log + continue.
                if let Ok(persisted) = cookies::load_from_db(&pool).await {
                    if !persisted.is_empty() {
                        let grouped = cookies::group_by_root_domain(persisted);
                        if let Ok(mut w) = cookie_cache.write() {
                            *w = grouped;
                        }
                        info!("loaded persisted cookies from db");
                    }
                }

                Arc::new(pool)
            });

            app.manage(commands::AppState {
                pool: pool.clone(),
                config: cfg.clone(),
                config_path: config_path.clone(),
                cookies: cookie_cache.clone(),
                expanded_threads: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
                webview_dark: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });

            std::thread::spawn(move || {
                rt.block_on(async move {
                    poller::spawn_background(pool, cfg, cookie_cache);
                    futures::future::pending::<()>().await;
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_categories,
            commands::get_forum,
            commands::get_thread,
            commands::get_forum_threads,
            commands::forum_supports_hot,
            commands::get_category,
            commands::get_category_threads,
            commands::category_thread_count,
            commands::category_supports_hot,
            commands::search_threads,
            commands::mark_forum_visited,
            commands::mark_all_forums_visited,
            commands::mark_thread_read,
            commands::mark_forum_threads_read,
            commands::refresh_forum,
            commands::refresh_all,
            commands::open_thread_webview,
            commands::close_thread_webview,
            commands::get_site_profile,
            commands::get_cached_posts,
            commands::set_render_preference,
            commands::get_render_preferences,
            commands::relearn_site_profile,
            commands::refine_site_profile,
            commands::list_site_profile_revisions,
            commands::undo_site_profile_revision,
            commands::reset_site_profile,
            commands::shrink_thread_webview,
            commands::restore_thread_webview,
            commands::load_more_posts,
            commands::set_zoom_level,
            commands::set_webview_color_scheme,
            commands::favorite_post,
            commands::unfavorite_post,
            commands::is_post_favorited,
            commands::list_thread_favorite_indexes,
            commands::list_favorite_posts,
            commands::thread_has_favorites,
            commands::find_thread_id_by_source_url,
            commands::set_last_read_post_index,
            commands::get_last_read_post_index,
            commands::get_feeds_yaml,
            commands::save_feeds_yaml,
            commands::get_feeds_path,
            commands::record_thread_visit,
            commands::list_thread_history,
            commands::clear_thread_history_entry,
            commands::clear_thread_history_all,
            commands::feeds_agent_send_message,
            commands::feeds_agent_revert,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn locate_project_root() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("feeds.yaml");
        if candidate.exists() {
            return cwd;
        }
        let parent = cwd.join("..").join("feeds.yaml");
        if parent.exists() {
            return cwd.parent().unwrap_or(&cwd).to_path_buf();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut p = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for _ in 0..6 {
            if p.join("feeds.yaml").exists() {
                return p;
            }
            if !p.pop() {
                break;
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Populate `ANTHROPIC_API_KEY` in process env from 1Password if not already
/// set. GUI-launched macOS apps don't inherit the user's direnv environment,
/// so this is the bridge between the user's keychain-backed `op` setup and
/// the in-process `std::env::var("ANTHROPIC_API_KEY")` reads scattered
/// through `src/anthropic/mod.rs`.
///
/// Flow:
/// 1. If `ANTHROPIC_API_KEY` is already set (cargo tauri dev), no-op.
/// 2. Read the OP service-account token from macOS Keychain (account name
///    `op-service-account`) via `/usr/bin/security`.
/// 3. Shell out to `op read 'op://MCP/<uuid>/password'` with the token in env.
/// 4. Set `ANTHROPIC_API_KEY` in the current process env.
///
/// All failures are warnings — the app continues running with reduced
/// functionality (RSS polling still works; per-thread native extraction will
/// fall back to webview mode).
fn bootstrap_anthropic_key_from_1password() {
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        return;
    }

    let token_out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-a", "op-service-account", "-w"])
        .output();
    let token = match token_out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        Ok(o) => {
            warn!(
                stderr = %String::from_utf8_lossy(&o.stderr).trim(),
                "1Password service-account token not found in Keychain; \
                 native extraction will fail until ANTHROPIC_API_KEY is provided"
            );
            return;
        }
        Err(e) => {
            warn!(err = %e, "could not invoke /usr/bin/security");
            return;
        }
    };
    if token.is_empty() {
        warn!("Keychain returned empty op-service-account token");
        return;
    }

    // GUI-launched apps have a minimal PATH; probe common Homebrew locations.
    let op_bin = ["/opt/homebrew/bin/op", "/usr/local/bin/op", "/usr/bin/op"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .copied();
    let op_bin = match op_bin {
        Some(p) => p,
        None => {
            warn!("`op` binary not found in /opt/homebrew/bin, /usr/local/bin, or /usr/bin");
            return;
        }
    };

    let result = std::process::Command::new(op_bin)
        .args(["read", "op://MCP/hgdnixfuvu5aqjo3u3sn3uoycm/password"])
        .env("OP_SERVICE_ACCOUNT_TOKEN", &token)
        .output();
    match result {
        Ok(o) if o.status.success() => {
            let key = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if key.is_empty() {
                warn!("`op read` returned an empty value for ANTHROPIC_API_KEY");
                return;
            }
            // Safety: process is single-threaded here — only the main thread is
            // alive before Tauri's runtime spins up workers, so no race with
            // concurrent env reads.
            unsafe { std::env::set_var("ANTHROPIC_API_KEY", &key); }
            info!("ANTHROPIC_API_KEY loaded from 1Password");
        }
        Ok(o) => warn!(
            stderr = %String::from_utf8_lossy(&o.stderr).trim(),
            "`op read` failed"
        ),
        Err(e) => warn!(err = %e, "could not invoke op"),
    }
}
