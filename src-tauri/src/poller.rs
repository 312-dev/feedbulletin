use crate::config::{AppConfig, ForumKind};
use crate::cookies::CookieCache;
use crate::db;
use crate::forums;
use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{info, warn};

pub const DEFAULT_POLL_INTERVAL_S: u64 = 1800;

const NON_REDDIT_CONCURRENCY: usize = 6;
const REDDIT_INTER_REQUEST_MS: u64 = 2200;

pub async fn seed_from_config(pool: &SqlitePool, cfg: &AppConfig) -> Result<()> {
    use std::collections::HashSet;
    let mut kept_categories: HashSet<String> = HashSet::new();
    let mut kept_forums: HashSet<(String, String)> = HashSet::new();

    for (c_idx, c) in cfg.categories.iter().enumerate() {
        kept_categories.insert(c.name.clone());
        let cid = db::upsert_category(pool, &c.name, c_idx as i64).await?;
        for f in &c.forums {
            if f.disabled {
                continue;
            }
            let title = f.resolved_title();
            let url = f.resolved_source_url();
            kept_forums.insert((c.name.clone(), title.clone()));
            db::upsert_forum(
                pool,
                cid,
                f.kind.as_str(),
                &title,
                &url,
                f.description.as_deref(),
                f.poll_interval_s.unwrap_or(DEFAULT_POLL_INTERVAL_S) as i64,
                f.theme.as_deref(),
            )
            .await?;
        }
    }

    // WHY: prune anything in the DB that isn't in the current feeds.yaml — categories
    // can be renamed/removed and forums can move between categories. Without this, the
    // index would show stale "Atlanta in Chicago & Suburbs" rows after a config edit.
    db::prune_to_set(pool, &kept_categories, &kept_forums).await?;
    Ok(())
}

pub async fn poll_all_once(
    pool: &SqlitePool,
    cfg: &AppConfig,
    cookies: Option<CookieCache>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let non_reddit_sem = Arc::new(Semaphore::new(NON_REDDIT_CONCURRENCY));
    let mut reddit_jobs: Vec<(String, crate::config::ForumConfig)> = Vec::new();
    let mut other_tasks = Vec::new();

    for c in &cfg.categories {
        for f in &c.forums {
            if f.disabled {
                continue;
            }
            if matches!(f.kind, ForumKind::Reddit) {
                reddit_jobs.push((c.name.clone(), f.clone()));
            } else {
                let pool = pool.clone();
                let client = client.clone();
                let category_name = c.name.clone();
                let forum_cfg = f.clone();
                let sem = non_reddit_sem.clone();
                let cookies = cookies.clone();
                other_tasks.push(tokio::spawn(async move {
                    let _permit = sem.acquire_owned().await.ok();
                    poll_forum(&pool, &client, &category_name, &forum_cfg, cookies.as_ref()).await
                }));
            }
        }
    }

    let pool_clone = pool.clone();
    let client_clone = client.clone();
    let cookies_clone = cookies.clone();
    let reddit_handle = tokio::spawn(async move {
        for (cat, f) in reddit_jobs {
            if let Err(e) =
                poll_forum(&pool_clone, &client_clone, &cat, &f, cookies_clone.as_ref()).await
            {
                warn!(err = %e, "reddit job error");
            }
            tokio::time::sleep(Duration::from_millis(REDDIT_INTER_REQUEST_MS)).await;
        }
    });

    for t in other_tasks {
        let _ = t.await;
    }
    let _ = reddit_handle.await;
    Ok(())
}

async fn poll_forum(
    pool: &SqlitePool,
    client: &reqwest::Client,
    category_name: &str,
    forum_cfg: &crate::config::ForumConfig,
    cookies: Option<&CookieCache>,
) -> Result<()> {
    let title = forum_cfg.resolved_title();
    let category = match sqlx::query_scalar::<_, i64>("SELECT id FROM categories WHERE name = ?")
        .bind(category_name)
        .fetch_optional(pool)
        .await?
    {
        Some(g) => g,
        None => return Ok(()),
    };
    let forum_id = match sqlx::query_scalar::<_, i64>(
        "SELECT id FROM forums WHERE category_id = ? AND title = ?",
    )
    .bind(category)
    .bind(&title)
    .fetch_optional(pool)
    .await?
    {
        Some(id) => id,
        None => return Ok(()),
    };

    match forums::fetch_forum(client, forum_cfg, cookies).await {
        Ok(threads) => {
            info!(forum = %title, count = threads.len(), "fetched");
            for t in &threads {
                if let Err(e) = db::upsert_thread(pool, forum_id, t).await {
                    warn!(forum = %title, err = %e, "upsert thread");
                }
            }
            let _ = db::refresh_forum_stats(pool, forum_id, chrono::Utc::now().timestamp()).await;
        }
        Err(e) => {
            warn!(forum = %title, err = %e, "fetch failed");
            let _ = db::record_forum_error(pool, forum_id, &e.to_string()).await;
        }
    }
    Ok(())
}

pub fn spawn_background(pool: Arc<SqlitePool>, cfg: Arc<AppConfig>, cookies: CookieCache) {
    tokio::spawn(async move {
        if let Err(e) = poll_all_once(&pool, &cfg, Some(cookies.clone())).await {
            warn!(err = %e, "initial poll failed");
        }
        let mut ticker = tokio::time::interval(Duration::from_secs(DEFAULT_POLL_INTERVAL_S));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = poll_all_once(&pool, &cfg, Some(cookies.clone())).await {
                warn!(err = %e, "scheduled poll failed");
            }
        }
    });
}
