use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS categories (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS forums (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id           INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    kind                  TEXT    NOT NULL,
    title                 TEXT    NOT NULL,
    source_url            TEXT    NOT NULL,
    description           TEXT,
    poll_interval_s       INTEGER NOT NULL DEFAULT 1800,
    thread_count          INTEGER NOT NULL DEFAULT 0,
    post_count            INTEGER NOT NULL DEFAULT 0,
    last_polled_at        INTEGER,
    last_error            TEXT,
    last_visited_at       INTEGER,
    latest_thread_title   TEXT,
    latest_thread_author  TEXT,
    latest_thread_at      INTEGER,
    latest_thread_url     TEXT,
    theme                 TEXT,
    UNIQUE(category_id, title)
);

CREATE TABLE IF NOT EXISTS threads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    forum_id        INTEGER NOT NULL REFERENCES forums(id) ON DELETE CASCADE,
    source_url      TEXT    NOT NULL,
    title           TEXT    NOT NULL,
    op_author       TEXT,
    op_author_url   TEXT,
    pubdate         INTEGER,
    reply_count     INTEGER,
    last_poster     TEXT,
    last_poster_url TEXT,
    last_post_at    INTEGER,
    excerpt         TEXT,
    read            INTEGER NOT NULL DEFAULT 0,
    hot_score       REAL,
    UNIQUE(forum_id, source_url)
);

CREATE INDEX IF NOT EXISTS idx_threads_forum_pubdate
    ON threads(forum_id, pubdate DESC);

-- WHY: external-content FTS5 table mirrors threads for vB-style search; content rowid = threads.id
CREATE VIRTUAL TABLE IF NOT EXISTS threads_fts USING fts5(
    title, op_author, last_poster, excerpt,
    content='threads', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS threads_ai AFTER INSERT ON threads BEGIN
    INSERT INTO threads_fts(rowid, title, op_author, last_poster, excerpt)
    VALUES (new.id, new.title, COALESCE(new.op_author,''), COALESCE(new.last_poster,''), COALESCE(new.excerpt,''));
END;

CREATE TRIGGER IF NOT EXISTS threads_ad AFTER DELETE ON threads BEGIN
    INSERT INTO threads_fts(threads_fts, rowid, title, op_author, last_poster, excerpt)
    VALUES('delete', old.id, old.title, COALESCE(old.op_author,''), COALESCE(old.last_poster,''), COALESCE(old.excerpt,''));
END;

CREATE TRIGGER IF NOT EXISTS threads_au AFTER UPDATE ON threads BEGIN
    INSERT INTO threads_fts(threads_fts, rowid, title, op_author, last_poster, excerpt)
    VALUES('delete', old.id, old.title, COALESCE(old.op_author,''), COALESCE(old.last_poster,''), COALESCE(old.excerpt,''));
    INSERT INTO threads_fts(rowid, title, op_author, last_poster, excerpt)
    VALUES (new.id, new.title, COALESCE(new.op_author,''), COALESCE(new.last_poster,''), COALESCE(new.excerpt,''));
END;

CREATE TABLE IF NOT EXISTS cookies (
    host        TEXT NOT NULL,
    name        TEXT NOT NULL,
    value       TEXT NOT NULL,
    path        TEXT NOT NULL DEFAULT '/',
    expires     INTEGER,
    secure      INTEGER NOT NULL DEFAULT 0,
    http_only   INTEGER NOT NULL DEFAULT 0,
    same_site   TEXT,
    imported_at INTEGER NOT NULL,
    PRIMARY KEY (host, name, path)
);

-- WHY: per-(host, content_type) CSS-selector profile. A single host can have
-- multiple profiles when its pages render differently (e.g. Reddit shows text
-- threads, image-OP threads, gallery threads, video-embed threads in distinct
-- DOM shapes). `content_probe` is a CSS selector the LLM picks that uniquely
-- matches THIS content type on the live page; the dispatcher tries each cached
-- profile's probe before falling through to a fresh learner call.
-- source = "anthropic" | "manual". `current_revision_id` points into
-- `site_profile_revisions` for undo/reset.
CREATE TABLE IF NOT EXISTS site_profiles (
    host                  TEXT NOT NULL,
    content_type          TEXT NOT NULL,
    content_probe         TEXT NOT NULL DEFAULT '',
    selectors_json        TEXT NOT NULL,
    schema_version        INTEGER NOT NULL DEFAULT 2,
    source                TEXT NOT NULL,
    created_at            INTEGER NOT NULL,
    last_validated_at     INTEGER NOT NULL,
    success_count         INTEGER NOT NULL DEFAULT 0,
    fail_count            INTEGER NOT NULL DEFAULT 0,
    last_error            TEXT,
    current_revision_id   INTEGER,
    PRIMARY KEY (host, content_type)
);

CREATE INDEX IF NOT EXISTS idx_site_profiles_host
    ON site_profiles(host);

-- WHY: every refinement (initial derive + each debug-mode user fix) gets its
-- own row. Chain via parent_revision_id. `set_current_revision` updates
-- site_profiles.current_revision_id; Undo walks to parent; Reset jumps to
-- the row where parent_revision_id IS NULL.
CREATE TABLE IF NOT EXISTS site_profile_revisions (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    host                TEXT NOT NULL,
    content_type        TEXT NOT NULL DEFAULT 'default',
    parent_revision_id  INTEGER,
    selectors_json      TEXT NOT NULL,
    source              TEXT NOT NULL,
    user_complaint      TEXT,
    created_at          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_site_profile_revisions_host
    ON site_profile_revisions(host, content_type, id);

-- WHY: opportunistic cache so re-opening a thread renders instantly without
-- re-scraping. Small TTL enforced in code; key is the local thread id (NOT a
-- foreign-key reference, since threads can be pruned and we'd rather lose the
-- cache than block deletes).
-- NOTE: any future evictor MUST exempt threads where thread_has_favorites() is
-- true — favoriting a post pins the thread cache indefinitely.
CREATE TABLE IF NOT EXISTS thread_post_cache (
    thread_id   INTEGER PRIMARY KEY,
    posts_json  TEXT NOT NULL,
    scraped_at  INTEGER NOT NULL
);

-- WHY: per-post favorites (stars). post_index = post_number when the source
-- exposes it, else the 1-based array position used by NativeThreadView's
-- `id="post-${n}"` anchor. Body snapshot fields keep /favorites readable even
-- if the parent thread row is later pruned. A row's existence here is the
-- signal that the thread's post cache must be retained indefinitely.
CREATE TABLE IF NOT EXISTS favorite_posts (
    thread_id         INTEGER NOT NULL,
    post_index        INTEGER NOT NULL,
    has_post_number   INTEGER NOT NULL DEFAULT 0,
    author            TEXT,
    timestamp_raw     TEXT,
    body_html         TEXT NOT NULL,
    body_excerpt      TEXT,
    thread_title      TEXT NOT NULL,
    thread_source_url TEXT NOT NULL,
    favorited_at      INTEGER NOT NULL,
    PRIMARY KEY (thread_id, post_index)
);

CREATE INDEX IF NOT EXISTS idx_favorite_posts_favorited_at
    ON favorite_posts(favorited_at);

-- WHY: history of recently-viewed threads. We snapshot title + source_url at
-- open time so /history stays readable even if the parent thread row is
-- pruned. Kept small (last 50; UI shows top 10) so this table never grows
-- unbounded.
CREATE TABLE IF NOT EXISTS thread_history (
    thread_id    INTEGER PRIMARY KEY,
    title        TEXT NOT NULL,
    source_url   TEXT NOT NULL,
    forum_id     INTEGER,
    forum_title  TEXT,
    visited_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_thread_history_visited_at
    ON thread_history(visited_at);

-- WHY: small key/value bag for UI state that should persist (render-mode
-- default, per-host native/webview overrides, etc.). Values are JSON-encoded
-- strings so the consumer decides shape.
CREATE TABLE IF NOT EXISTS user_prefs (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- WHY: per-author profile cache. Populated by the enrichment task that
-- fetches each unique author's profile URL after a thread renders. Each
-- field is independently optional — agent populates whichever locators
-- the platform exposes. Cached forever per (host, username); columns
-- being NULL means "we tried and that field wasn't on the profile."
CREATE TABLE IF NOT EXISTS author_profiles (
    host         TEXT NOT NULL,
    author       TEXT NOT NULL,
    avatar_url   TEXT,
    post_count   TEXT,
    karma        TEXT,
    join_date    TEXT,
    last_active  TEXT,
    location     TEXT,
    rank         TEXT,
    fetched_at   INTEGER NOT NULL,
    PRIMARY KEY (host, author)
);
"#;

pub async fn open_pool(url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)
        .context("invalid sqlite url")?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .context("opening sqlite pool")?;

    // Drop legacy tables from v0 layout if they exist, so we cleanly recreate
    // under the renamed (categories/forums) schema. Plan says no-migration-ceremony.
    let _ = sqlx::query("DROP TABLE IF EXISTS feeds").execute(&pool).await;
    let _ = sqlx::query("DROP TABLE IF EXISTS groups").execute(&pool).await;

    // WHY: previous schema lacked op_author_url/last_poster_url; add cheaply on existing dbs.
    let _ = sqlx::query("ALTER TABLE threads ADD COLUMN op_author_url TEXT")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE threads ADD COLUMN last_poster_url TEXT")
        .execute(&pool)
        .await;
    // WHY: per-post read state — last post_index the user has scrolled past in
    // this thread. The native renderer uses this to draw a "New posts ↓"
    // divider above the first unseen reply on reopen. NULL = thread has never
    // been opened in native mode (legacy rows + freshly-polled threads).
    let _ = sqlx::query("ALTER TABLE threads ADD COLUMN last_read_post_index INTEGER")
        .execute(&pool)
        .await;
    // WHY: site_profiles gained current_revision_id when debugger mode landed.
    let _ = sqlx::query("ALTER TABLE site_profiles ADD COLUMN current_revision_id INTEGER")
        .execute(&pool)
        .await;
    // WHY: forums.theme is the per-forum skin override (slug from the frontend
    // theme registry). Idempotent ALTER on existing dbs; no-op on fresh ones
    // since SCHEMA already declares the column.
    let _ = sqlx::query("ALTER TABLE forums ADD COLUMN theme TEXT")
        .execute(&pool)
        .await;
    // WHY: schema v2 moves site_profiles PK from `host` to (host, content_type)
    // and adds content_probe. Cheapest path on existing dbs: drop both tables;
    // they hold pure cache that's a single Anthropic call away from being
    // rebuilt. The DROPs are no-ops on fresh dbs.
    let needs_v2_reset = match sqlx::query("SELECT content_probe FROM site_profiles LIMIT 1")
        .execute(&pool)
        .await
    {
        Ok(_) => false,
        Err(e) => format!("{e}").contains("no such column"),
    };
    if needs_v2_reset {
        let _ = sqlx::query("DROP TABLE IF EXISTS site_profiles").execute(&pool).await;
        let _ = sqlx::query("DROP TABLE IF EXISTS site_profile_revisions").execute(&pool).await;
    }
    // WHY: author_avatars (v1, single avatar_url column) → author_profiles
    // (v2, columns per metadata field). Migration: rename + add columns.
    // No-ops on fresh installs; harmlessly fail when columns already exist.
    let _ = sqlx::query("ALTER TABLE author_avatars RENAME TO author_profiles")
        .execute(&pool)
        .await;
    for col in &["post_count", "karma", "join_date", "last_active", "location", "rank"] {
        let _ = sqlx::query(&format!("ALTER TABLE author_profiles ADD COLUMN {} TEXT", col))
            .execute(&pool)
            .await;
    }

    // WHY: sqlx 0.8's SqliteConnection runs the WHOLE statement-string atomically per
    // query; we use that here because triggers contain semicolons which a naive split
    // would corrupt. fts5 triggers are also order-sensitive after table creation.
    sqlx::raw_sql(SCHEMA)
        .execute(&pool)
        .await
        .context("schema bootstrap")?;
    Ok(pool)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRow {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumRow {
    pub id: i64,
    pub category_id: i64,
    pub kind: String,
    pub title: String,
    pub source_url: String,
    pub description: Option<String>,
    pub poll_interval_s: i64,
    pub thread_count: i64,
    pub post_count: i64,
    pub last_polled_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_visited_at: Option<i64>,
    pub latest_thread_title: Option<String>,
    pub latest_thread_author: Option<String>,
    pub latest_thread_at: Option<i64>,
    pub latest_thread_url: Option<String>,
    pub theme: Option<String>,
    pub unread: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRow {
    pub id: i64,
    pub forum_id: i64,
    pub source_url: String,
    pub title: String,
    pub op_author: Option<String>,
    pub op_author_url: Option<String>,
    pub pubdate: Option<i64>,
    pub reply_count: Option<i64>,
    pub last_poster: Option<String>,
    pub last_poster_url: Option<String>,
    pub last_post_at: Option<i64>,
    pub excerpt: Option<String>,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryWithForums {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub forums: Vec<ForumRow>,
}

pub async fn upsert_category(pool: &SqlitePool, name: &str, sort_order: i64) -> Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO categories(name, sort_order) VALUES (?, ?)")
        .bind(name)
        .bind(sort_order)
        .execute(pool)
        .await?;
    let row = sqlx::query("SELECT id FROM categories WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>("id"))
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_forum(
    pool: &SqlitePool,
    category_id: i64,
    kind: &str,
    title: &str,
    source_url: &str,
    description: Option<&str>,
    poll_interval_s: i64,
    theme: Option<&str>,
) -> Result<i64> {
    sqlx::query(
        r#"INSERT INTO forums(category_id, kind, title, source_url, description, poll_interval_s, theme)
           VALUES (?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(category_id, title) DO UPDATE SET
             kind            = excluded.kind,
             source_url      = excluded.source_url,
             description     = excluded.description,
             poll_interval_s = excluded.poll_interval_s,
             theme           = excluded.theme"#,
    )
    .bind(category_id)
    .bind(kind)
    .bind(title)
    .bind(source_url)
    .bind(description)
    .bind(poll_interval_s)
    .bind(theme)
    .execute(pool)
    .await?;
    let row = sqlx::query("SELECT id FROM forums WHERE category_id = ? AND title = ?")
        .bind(category_id)
        .bind(title)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>("id"))
}

fn forum_row_from_row(r: sqlx::sqlite::SqliteRow) -> ForumRow {
    let last_visited_at: Option<i64> = r.get("last_visited_at");
    let latest_thread_at: Option<i64> = r.get("latest_thread_at");
    let unread = match (latest_thread_at, last_visited_at) {
        (Some(latest), Some(visited)) => latest > visited,
        (Some(_), None) => true,
        _ => false,
    };
    ForumRow {
        id: r.get("id"),
        category_id: r.get("category_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        source_url: r.get("source_url"),
        description: r.get("description"),
        poll_interval_s: r.get("poll_interval_s"),
        thread_count: r.get("thread_count"),
        post_count: r.get("post_count"),
        last_polled_at: r.get("last_polled_at"),
        last_error: r.get("last_error"),
        last_visited_at,
        latest_thread_title: r.get("latest_thread_title"),
        latest_thread_author: r.get("latest_thread_author"),
        latest_thread_at,
        latest_thread_url: r.get("latest_thread_url"),
        theme: r.try_get("theme").ok().flatten(),
        unread,
    }
}

pub async fn list_categories_with_forums(pool: &SqlitePool) -> Result<Vec<CategoryWithForums>> {
    let categories =
        sqlx::query("SELECT id, name, sort_order FROM categories ORDER BY sort_order, id")
            .fetch_all(pool)
            .await?;

    let mut out = Vec::with_capacity(categories.len());
    for c in categories {
        let cid: i64 = c.get("id");
        let forums = sqlx::query(
            r#"SELECT id, category_id, kind, title, source_url, description, poll_interval_s,
                      thread_count, post_count, last_polled_at, last_error,
                      last_visited_at, latest_thread_title, latest_thread_author,
                      latest_thread_at, latest_thread_url, theme
               FROM forums WHERE category_id = ? ORDER BY id"#,
        )
        .bind(cid)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(forum_row_from_row)
        .collect();

        out.push(CategoryWithForums {
            id: cid,
            name: c.get("name"),
            sort_order: c.get("sort_order"),
            forums,
        });
    }
    Ok(out)
}

fn thread_row_from(r: sqlx::sqlite::SqliteRow) -> ThreadRow {
    ThreadRow {
        id: r.get("id"),
        forum_id: r.get("forum_id"),
        source_url: r.get("source_url"),
        title: r.get("title"),
        op_author: r.get("op_author"),
        op_author_url: r.try_get("op_author_url").ok().flatten(),
        pubdate: r.get("pubdate"),
        reply_count: r.get("reply_count"),
        last_poster: r.get("last_poster"),
        last_poster_url: r.try_get("last_poster_url").ok().flatten(),
        last_post_at: r.get("last_post_at"),
        excerpt: r.get("excerpt"),
        read: r.get::<i64, _>("read") != 0,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortMode {
    New,
    Hot,
}

impl SortMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "hot" => SortMode::Hot,
            _ => SortMode::New,
        }
    }
}

pub async fn list_forum_threads(
    pool: &SqlitePool,
    forum_id: i64,
    limit: i64,
    offset: i64,
    sort: SortMode,
) -> Result<Vec<ThreadRow>> {
    // WHY: Hot ranks by hot_score (Reddit score, XenForo reply count, Discourse posts)
    // with NULLS LAST so forums where the parser doesn't populate hot_score fall back
    // to a chronological tail.
    let order_clause = match sort {
        SortMode::New => "ORDER BY COALESCE(pubdate, 0) DESC, id DESC",
        SortMode::Hot => {
            "ORDER BY (hot_score IS NULL) ASC, COALESCE(hot_score, 0) DESC, COALESCE(pubdate, 0) DESC, id DESC"
        }
    };
    let sql = format!(
        r#"SELECT id, forum_id, source_url, title, op_author, op_author_url, pubdate, reply_count,
                  last_poster, last_poster_url, last_post_at, excerpt, read
           FROM threads
           WHERE forum_id = ?
           {order_clause}
           LIMIT ? OFFSET ?"#
    );
    let rows = sqlx::query(&sql)
        .bind(forum_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(thread_row_from).collect())
}

/// Whether any thread in this forum has a non-null hot_score. Used by the UI to
/// decide if the Hot sort toggle should be enabled.
pub async fn forum_supports_hot(pool: &SqlitePool, forum_id: i64) -> Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM threads WHERE forum_id = ? AND hot_score IS NOT NULL LIMIT 1",
    )
    .bind(forum_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

pub async fn list_category_threads(
    pool: &SqlitePool,
    category_id: i64,
    limit: i64,
    offset: i64,
    sort: SortMode,
) -> Result<Vec<ThreadRow>> {
    let order_clause = match sort {
        SortMode::New => "ORDER BY COALESCE(t.pubdate, 0) DESC, t.id DESC",
        SortMode::Hot => {
            "ORDER BY (t.hot_score IS NULL) ASC, COALESCE(t.hot_score, 0) DESC, COALESCE(t.pubdate, 0) DESC, t.id DESC"
        }
    };
    let sql = format!(
        r#"SELECT t.id, t.forum_id, t.source_url, t.title, t.op_author, t.op_author_url,
                  t.pubdate, t.reply_count, t.last_poster, t.last_poster_url,
                  t.last_post_at, t.excerpt, t.read
           FROM threads t
           JOIN forums f ON f.id = t.forum_id
           WHERE f.category_id = ?
           {order_clause}
           LIMIT ? OFFSET ?"#
    );
    let rows = sqlx::query(&sql)
        .bind(category_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(thread_row_from).collect())
}

pub async fn category_thread_count(pool: &SqlitePool, category_id: i64) -> Result<i64> {
    let r: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM threads t JOIN forums f ON f.id = t.forum_id WHERE f.category_id = ?",
    )
    .bind(category_id)
    .fetch_one(pool)
    .await?;
    Ok(r.0)
}

pub async fn category_supports_hot(pool: &SqlitePool, category_id: i64) -> Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM threads t JOIN forums f ON f.id = t.forum_id \
         WHERE f.category_id = ? AND t.hot_score IS NOT NULL LIMIT 1",
    )
    .bind(category_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Remove categories not in `kept_categories` (cascades to their forums + threads)
/// and forums not in `kept_forums` (cascades to their threads). Used after seeding
/// the DB from feeds.yaml so renames/removals propagate without manual DB editing.
pub async fn prune_to_set(
    pool: &SqlitePool,
    kept_categories: &std::collections::HashSet<String>,
    kept_forums: &std::collections::HashSet<(String, String)>,
) -> Result<()> {
    let all_cats: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM categories").fetch_all(pool).await?;
    for (cid, cname) in &all_cats {
        if !kept_categories.contains(cname) {
            sqlx::query("DELETE FROM categories WHERE id = ?")
                .bind(cid)
                .execute(pool)
                .await?;
        }
    }
    let all_forums: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT f.id, f.category_id, f.title FROM forums f",
    )
    .fetch_all(pool)
    .await?;
    let cat_by_id: std::collections::HashMap<i64, String> =
        all_cats.into_iter().collect();
    for (fid, cid, ftitle) in &all_forums {
        let cname = cat_by_id.get(cid);
        match cname {
            Some(name) => {
                if !kept_forums.contains(&(name.clone(), ftitle.clone())) {
                    sqlx::query("DELETE FROM forums WHERE id = ?")
                        .bind(fid)
                        .execute(pool)
                        .await?;
                }
            }
            None => {
                // category already deleted above; cascade should handle this row
                sqlx::query("DELETE FROM forums WHERE id = ?")
                    .bind(fid)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn get_category(pool: &SqlitePool, id: i64) -> Result<Option<CategoryRow>> {
    let row = sqlx::query("SELECT id, name, sort_order FROM categories WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| CategoryRow {
        id: r.get("id"),
        name: r.get("name"),
        sort_order: r.get("sort_order"),
    }))
}

pub async fn search_threads(
    pool: &SqlitePool,
    query: &str,
    forum_id: Option<i64>,
    limit: i64,
) -> Result<Vec<ThreadRow>> {
    let q = sanitize_fts_query(query);
    if q.is_empty() {
        return Ok(vec![]);
    }
    let sql = if forum_id.is_some() {
        // WHY: join threads_fts back to threads via rowid; order by FTS rank then recency.
        r#"SELECT t.id, t.forum_id, t.source_url, t.title, t.op_author, t.op_author_url,
                  t.pubdate, t.reply_count, t.last_poster, t.last_poster_url, t.last_post_at,
                  t.excerpt, t.read
             FROM threads_fts f JOIN threads t ON t.id = f.rowid
             WHERE f.threads_fts MATCH ? AND t.forum_id = ?
             ORDER BY bm25(threads_fts, 3.0, 2.0, 1.5, 1.0), COALESCE(t.pubdate, 0) DESC
             LIMIT ?"#
    } else {
        r#"SELECT t.id, t.forum_id, t.source_url, t.title, t.op_author, t.op_author_url,
                  t.pubdate, t.reply_count, t.last_poster, t.last_poster_url, t.last_post_at,
                  t.excerpt, t.read
             FROM threads_fts f JOIN threads t ON t.id = f.rowid
             WHERE f.threads_fts MATCH ?
             ORDER BY bm25(threads_fts, 3.0, 2.0, 1.5, 1.0), COALESCE(t.pubdate, 0) DESC
             LIMIT ?"#
    };

    let mut qb = sqlx::query(sql).bind(&q);
    if let Some(fid) = forum_id {
        qb = qb.bind(fid);
    }
    qb = qb.bind(limit);

    let rows = qb.fetch_all(pool).await?;
    Ok(rows.into_iter().map(thread_row_from).collect())
}

// WHY: FTS5 MATCH is fussy — bare punctuation/empty terms blow up. Wrap each
// whitespace token in double quotes and prefix-match it; an empty result skips the query.
fn sanitize_fts_query(q: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for raw in q.split_whitespace() {
        let cleaned: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        parts.push(format!("\"{}\"*", cleaned));
    }
    parts.join(" ")
}

pub async fn get_thread(pool: &SqlitePool, thread_id: i64) -> Result<Option<ThreadRow>> {
    let r = sqlx::query(
        r#"SELECT id, forum_id, source_url, title, op_author, op_author_url, pubdate, reply_count,
                  last_poster, last_poster_url, last_post_at, excerpt, read
           FROM threads WHERE id = ?"#,
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(thread_row_from))
}

pub async fn find_thread_id_by_source_url(
    pool: &SqlitePool,
    url: &str,
) -> Result<Option<i64>> {
    let r = sqlx::query_scalar::<_, i64>("SELECT id FROM threads WHERE source_url = ? LIMIT 1")
        .bind(url)
        .fetch_optional(pool)
        .await?;
    Ok(r)
}

pub async fn get_forum(pool: &SqlitePool, forum_id: i64) -> Result<Option<ForumRow>> {
    let r = sqlx::query(
        r#"SELECT id, category_id, kind, title, source_url, description, poll_interval_s,
                  thread_count, post_count, last_polled_at, last_error,
                  last_visited_at, latest_thread_title, latest_thread_author,
                  latest_thread_at, latest_thread_url, theme
           FROM forums WHERE id = ?"#,
    )
    .bind(forum_id)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(forum_row_from_row))
}

pub async fn mark_forum_visited(pool: &SqlitePool, forum_id: i64, at: i64) -> Result<()> {
    sqlx::query("UPDATE forums SET last_visited_at = ? WHERE id = ?")
        .bind(at)
        .bind(forum_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_all_forums_visited(pool: &SqlitePool, at: i64) -> Result<()> {
    sqlx::query("UPDATE forums SET last_visited_at = ?")
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct ThreadInput {
    pub source_url: String,
    pub title: String,
    pub op_author: Option<String>,
    pub op_author_url: Option<String>,
    pub pubdate: Option<i64>,
    pub reply_count: Option<i64>,
    pub last_poster: Option<String>,
    pub last_poster_url: Option<String>,
    pub last_post_at: Option<i64>,
    pub excerpt: Option<String>,
    pub hot_score: Option<f64>,
}

pub async fn upsert_thread(pool: &SqlitePool, forum_id: i64, t: &ThreadInput) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO threads
           (forum_id, source_url, title, op_author, op_author_url, pubdate, reply_count,
            last_poster, last_poster_url, last_post_at, excerpt, hot_score)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(forum_id, source_url) DO UPDATE SET
             title           = excluded.title,
             op_author       = COALESCE(excluded.op_author, threads.op_author),
             op_author_url   = COALESCE(excluded.op_author_url, threads.op_author_url),
             pubdate         = COALESCE(excluded.pubdate, threads.pubdate),
             reply_count     = COALESCE(excluded.reply_count, threads.reply_count),
             last_poster     = COALESCE(excluded.last_poster, threads.last_poster),
             last_poster_url = COALESCE(excluded.last_poster_url, threads.last_poster_url),
             last_post_at    = COALESCE(excluded.last_post_at, threads.last_post_at),
             excerpt         = COALESCE(excluded.excerpt, threads.excerpt),
             hot_score       = COALESCE(excluded.hot_score, threads.hot_score)"#,
    )
    .bind(forum_id)
    .bind(&t.source_url)
    .bind(&t.title)
    .bind(&t.op_author)
    .bind(&t.op_author_url)
    .bind(t.pubdate)
    .bind(t.reply_count)
    .bind(&t.last_poster)
    .bind(&t.last_poster_url)
    .bind(t.last_post_at)
    .bind(&t.excerpt)
    .bind(t.hot_score)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn refresh_forum_stats(pool: &SqlitePool, forum_id: i64, polled_at: i64) -> Result<()> {
    let row = sqlx::query(
        r#"SELECT COUNT(*) AS c,
                  COALESCE(SUM(COALESCE(reply_count, 0)), 0) AS posts
             FROM threads WHERE forum_id = ?"#,
    )
    .bind(forum_id)
    .fetch_one(pool)
    .await?;
    let c: i64 = row.get("c");
    let p: i64 = row.get("posts");

    // Pull the latest thread (by pubdate desc) for last-post preview denorm.
    let latest = sqlx::query(
        r#"SELECT title, op_author, pubdate, source_url
             FROM threads
             WHERE forum_id = ?
             ORDER BY COALESCE(pubdate, 0) DESC, id DESC
             LIMIT 1"#,
    )
    .bind(forum_id)
    .fetch_optional(pool)
    .await?;
    let (lt_title, lt_author, lt_at, lt_url): (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = match latest {
        Some(r) => (
            Some(r.get::<String, _>("title")),
            r.get::<Option<String>, _>("op_author"),
            r.get::<Option<i64>, _>("pubdate"),
            Some(r.get::<String, _>("source_url")),
        ),
        None => (None, None, None, None),
    };

    sqlx::query(
        r#"UPDATE forums SET
             thread_count = ?,
             post_count = ?,
             last_polled_at = ?,
             last_error = NULL,
             latest_thread_title = ?,
             latest_thread_author = ?,
             latest_thread_at = ?,
             latest_thread_url = ?
           WHERE id = ?"#,
    )
    .bind(c)
    .bind(p)
    .bind(polled_at)
    .bind(lt_title)
    .bind(lt_author)
    .bind(lt_at)
    .bind(lt_url)
    .bind(forum_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_forum_error(pool: &SqlitePool, forum_id: i64, msg: &str) -> Result<()> {
    sqlx::query("UPDATE forums SET last_error = ?, last_polled_at = ? WHERE id = ?")
        .bind(msg)
        .bind(chrono::Utc::now().timestamp())
        .bind(forum_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- site_profiles / thread_post_cache / user_prefs ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteProfileRow {
    pub host: String,
    pub content_type: String,
    pub content_probe: String,
    pub selectors_json: String,
    pub schema_version: i64,
    pub source: String,
    pub created_at: i64,
    pub last_validated_at: i64,
    pub success_count: i64,
    pub fail_count: i64,
    pub last_error: Option<String>,
    pub current_revision_id: Option<i64>,
}

fn site_profile_row(r: sqlx::sqlite::SqliteRow) -> SiteProfileRow {
    SiteProfileRow {
        host: r.get("host"),
        content_type: r.get("content_type"),
        content_probe: r.try_get("content_probe").unwrap_or_default(),
        selectors_json: r.get("selectors_json"),
        schema_version: r.get("schema_version"),
        source: r.get("source"),
        created_at: r.get("created_at"),
        last_validated_at: r.get("last_validated_at"),
        success_count: r.get("success_count"),
        fail_count: r.get("fail_count"),
        last_error: r.get("last_error"),
        current_revision_id: r.try_get("current_revision_id").ok().flatten(),
    }
}

/// All cached profiles for one host, in creation order. The dispatcher walks
/// this list, runs each profile's `content_probe`, and uses the first match.
pub async fn list_site_profiles_for_host(
    pool: &SqlitePool,
    host: &str,
) -> Result<Vec<SiteProfileRow>> {
    let rows = sqlx::query(
        r#"SELECT host, content_type, content_probe, selectors_json, schema_version,
                  source, created_at, last_validated_at, success_count, fail_count,
                  last_error, current_revision_id
             FROM site_profiles
             WHERE host = ?
             ORDER BY created_at ASC"#,
    )
    .bind(host)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(site_profile_row).collect())
}

pub async fn get_site_profile(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
) -> Result<Option<SiteProfileRow>> {
    let r = sqlx::query(
        r#"SELECT host, content_type, content_probe, selectors_json, schema_version,
                  source, created_at, last_validated_at, success_count, fail_count,
                  last_error, current_revision_id
             FROM site_profiles WHERE host = ? AND content_type = ?"#,
    )
    .bind(host)
    .bind(content_type)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(site_profile_row))
}

pub async fn upsert_site_profile(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
    content_probe: &str,
    selectors_json: &str,
    source: &str,
) -> Result<()> {
    insert_profile_revision(
        pool,
        host,
        content_type,
        content_probe,
        selectors_json,
        source,
        None,
        None,
    )
    .await?;
    Ok(())
}

/// Insert a new revision and set it as the active one. Returns the new
/// revision id. This is the canonical write path for any selector change —
/// initial derive AND debugger-mode refinements both go through here.
#[allow(clippy::too_many_arguments)]
pub async fn insert_profile_revision(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
    content_probe: &str,
    selectors_json: &str,
    source: &str,
    parent_revision_id: Option<i64>,
    user_complaint: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();
    let rev_id_row =
        sqlx::query("INSERT INTO site_profile_revisions(host, content_type, parent_revision_id, selectors_json, source, user_complaint, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id")
            .bind(host)
            .bind(content_type)
            .bind(parent_revision_id)
            .bind(selectors_json)
            .bind(source)
            .bind(user_complaint)
            .bind(now)
            .fetch_one(pool)
            .await?;
    let rev_id: i64 = rev_id_row.get("id");

    sqlx::query(
        r#"INSERT INTO site_profiles(host, content_type, content_probe, selectors_json, source, created_at, last_validated_at, current_revision_id)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(host, content_type) DO UPDATE SET
             content_probe       = excluded.content_probe,
             selectors_json      = excluded.selectors_json,
             source              = excluded.source,
             last_validated_at   = excluded.last_validated_at,
             last_error          = NULL,
             current_revision_id = excluded.current_revision_id"#,
    )
    .bind(host)
    .bind(content_type)
    .bind(content_probe)
    .bind(selectors_json)
    .bind(source)
    .bind(now)
    .bind(now)
    .bind(rev_id)
    .execute(pool)
    .await?;
    Ok(rev_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRevision {
    pub id: i64,
    pub host: String,
    pub content_type: String,
    pub parent_revision_id: Option<i64>,
    pub selectors_json: String,
    pub source: String,
    pub user_complaint: Option<String>,
    pub created_at: i64,
}

fn revision_row(r: sqlx::sqlite::SqliteRow) -> ProfileRevision {
    ProfileRevision {
        id: r.get("id"),
        host: r.get("host"),
        content_type: r.try_get("content_type").unwrap_or_else(|_| "default".to_string()),
        parent_revision_id: r.get("parent_revision_id"),
        selectors_json: r.get("selectors_json"),
        source: r.get("source"),
        user_complaint: r.get("user_complaint"),
        created_at: r.get("created_at"),
    }
}

pub async fn get_profile_revision(pool: &SqlitePool, id: i64) -> Result<Option<ProfileRevision>> {
    let r = sqlx::query(
        "SELECT id, host, content_type, parent_revision_id, selectors_json, source, user_complaint, created_at \
         FROM site_profile_revisions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(revision_row))
}

pub async fn list_profile_revisions(
    pool: &SqlitePool,
    host: &str,
    content_type: Option<&str>,
) -> Result<Vec<ProfileRevision>> {
    let rows = if let Some(ct) = content_type {
        sqlx::query(
            "SELECT id, host, content_type, parent_revision_id, selectors_json, source, user_complaint, created_at \
             FROM site_profile_revisions WHERE host = ? AND content_type = ? ORDER BY id ASC",
        )
        .bind(host)
        .bind(ct)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, host, content_type, parent_revision_id, selectors_json, source, user_complaint, created_at \
             FROM site_profile_revisions WHERE host = ? ORDER BY id ASC",
        )
        .bind(host)
        .fetch_all(pool)
        .await?
    };
    Ok(rows.into_iter().map(revision_row).collect())
}

/// Move the active revision to `revision_id` and update the materialized
/// selectors_json on site_profiles. Returns the new active revision row.
pub async fn set_current_revision(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
    revision_id: i64,
) -> Result<ProfileRevision> {
    let rev = get_profile_revision(pool, revision_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("revision {revision_id} not found"))?;
    if rev.host != host || rev.content_type != content_type {
        anyhow::bail!(
            "revision {revision_id} belongs to ({}, {})",
            rev.host,
            rev.content_type
        );
    }
    sqlx::query(
        "UPDATE site_profiles SET selectors_json = ?, current_revision_id = ?, \
         last_validated_at = ?, last_error = NULL \
         WHERE host = ? AND content_type = ?",
    )
    .bind(&rev.selectors_json)
    .bind(revision_id)
    .bind(chrono::Utc::now().timestamp())
    .bind(host)
    .bind(content_type)
    .execute(pool)
    .await?;
    Ok(rev)
}

pub async fn bump_profile_success(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE site_profiles SET success_count = success_count + 1, \
         last_validated_at = ?, last_error = NULL \
         WHERE host = ? AND content_type = ?",
    )
    .bind(chrono::Utc::now().timestamp())
    .bind(host)
    .bind(content_type)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn bump_profile_fail(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
    err: &str,
) -> Result<()> {
    tracing::warn!(host, content_type, err, "bump_profile_fail called");
    sqlx::query(
        "UPDATE site_profiles SET fail_count = fail_count + 1, \
         last_validated_at = ?, last_error = ? \
         WHERE host = ? AND content_type = ?",
    )
    .bind(chrono::Utc::now().timestamp())
    .bind(err)
    .bind(host)
    .bind(content_type)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete every profile for a host (all content types) plus its revision chain.
pub async fn delete_site_profile(pool: &SqlitePool, host: &str) -> Result<()> {
    // DIAGNOSTIC: loud log + backtrace so we never lose track of who deleted
    // a profile (the autopia.org ghost-write bug was traced to a delete
    // call that we couldn't otherwise see in the log).
    tracing::warn!(
        host,
        backtrace = ?std::backtrace::Backtrace::force_capture(),
        "delete_site_profile(host) called — deleting ALL content-type profiles for this host"
    );
    sqlx::query("DELETE FROM site_profiles WHERE host = ?")
        .bind(host)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM site_profile_revisions WHERE host = ?")
        .bind(host)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete one specific content-type profile for a host.
pub async fn delete_site_profile_one(
    pool: &SqlitePool,
    host: &str,
    content_type: &str,
) -> Result<()> {
    tracing::warn!(
        host,
        content_type,
        backtrace = ?std::backtrace::Backtrace::force_capture(),
        "delete_site_profile_one(host, content_type) called"
    );
    sqlx::query("DELETE FROM site_profiles WHERE host = ? AND content_type = ?")
        .bind(host)
        .bind(content_type)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM site_profile_revisions WHERE host = ? AND content_type = ?")
        .bind(host)
        .bind(content_type)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn cache_posts(pool: &SqlitePool, thread_id: i64, posts_json: &str) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO thread_post_cache(thread_id, posts_json, scraped_at)
           VALUES (?, ?, ?)
           ON CONFLICT(thread_id) DO UPDATE SET
             posts_json = excluded.posts_json,
             scraped_at = excluded.scraped_at"#,
    )
    .bind(thread_id)
    .bind(posts_json)
    .bind(chrono::Utc::now().timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_cached_posts(pool: &SqlitePool, thread_id: i64) -> Result<Option<(String, i64)>> {
    let r = sqlx::query("SELECT posts_json, scraped_at FROM thread_post_cache WHERE thread_id = ?")
        .bind(thread_id)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(|r| (r.get::<String, _>("posts_json"), r.get::<i64, _>("scraped_at"))))
}

pub async fn get_pref(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let r = sqlx::query("SELECT value FROM user_prefs WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(r.map(|r| r.get::<String, _>("value")))
}

pub async fn get_cached_author_profile(
    pool: &SqlitePool,
    host: &str,
    author: &str,
) -> Result<Option<crate::enrichment::ProfileFields>> {
    let r = sqlx::query(
        "SELECT avatar_url, post_count, karma, join_date, last_active, location, rank \
         FROM author_profiles WHERE host = ? AND author = ?",
    )
    .bind(host)
    .bind(author)
    .fetch_optional(pool)
    .await?;
    Ok(r.map(|r| crate::enrichment::ProfileFields {
        avatar_url: r.try_get("avatar_url").ok().flatten(),
        post_count: r.try_get("post_count").ok().flatten(),
        karma: r.try_get("karma").ok().flatten(),
        join_date: r.try_get("join_date").ok().flatten(),
        last_active: r.try_get("last_active").ok().flatten(),
        location: r.try_get("location").ok().flatten(),
        rank: r.try_get("rank").ok().flatten(),
    }))
}

pub async fn cache_author_profile(
    pool: &SqlitePool,
    host: &str,
    author: &str,
    fields: &crate::enrichment::ProfileFields,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO author_profiles(host, author, avatar_url, post_count, karma, \
                                     join_date, last_active, location, rank, fetched_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(host, author) DO UPDATE SET \
           avatar_url  = COALESCE(excluded.avatar_url,  author_profiles.avatar_url), \
           post_count  = COALESCE(excluded.post_count,  author_profiles.post_count), \
           karma       = COALESCE(excluded.karma,       author_profiles.karma), \
           join_date   = COALESCE(excluded.join_date,   author_profiles.join_date), \
           last_active = COALESCE(excluded.last_active, author_profiles.last_active), \
           location    = COALESCE(excluded.location,    author_profiles.location), \
           rank        = COALESCE(excluded.rank,        author_profiles.rank), \
           fetched_at  = excluded.fetched_at",
    )
    .bind(host)
    .bind(author)
    .bind(&fields.avatar_url)
    .bind(&fields.post_count)
    .bind(&fields.karma)
    .bind(&fields.join_date)
    .bind(&fields.last_active)
    .bind(&fields.location)
    .bind(&fields.rank)
    .bind(chrono::Utc::now().timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_pref(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_prefs(key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------- favorite_posts ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritePostRow {
    pub thread_id: i64,
    pub post_index: i64,
    pub has_post_number: bool,
    pub author: Option<String>,
    pub timestamp_raw: Option<String>,
    pub body_html: String,
    pub body_excerpt: Option<String>,
    pub thread_title: String,
    pub thread_source_url: String,
    pub favorited_at: i64,
}

fn favorite_post_row(r: sqlx::sqlite::SqliteRow) -> FavoritePostRow {
    FavoritePostRow {
        thread_id: r.get("thread_id"),
        post_index: r.get("post_index"),
        has_post_number: r.get::<i64, _>("has_post_number") != 0,
        author: r.get("author"),
        timestamp_raw: r.get("timestamp_raw"),
        body_html: r.get("body_html"),
        body_excerpt: r.get("body_excerpt"),
        thread_title: r.get("thread_title"),
        thread_source_url: r.get("thread_source_url"),
        favorited_at: r.get("favorited_at"),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn favorite_post(
    pool: &SqlitePool,
    thread_id: i64,
    post_index: i64,
    has_post_number: bool,
    author: Option<&str>,
    timestamp_raw: Option<&str>,
    body_html: &str,
    body_excerpt: Option<&str>,
    thread_title: &str,
    thread_source_url: &str,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO favorite_posts(
             thread_id, post_index, has_post_number, author, timestamp_raw,
             body_html, body_excerpt, thread_title, thread_source_url, favorited_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(thread_id, post_index) DO UPDATE SET
             has_post_number   = excluded.has_post_number,
             author            = excluded.author,
             timestamp_raw     = excluded.timestamp_raw,
             body_html         = excluded.body_html,
             body_excerpt      = excluded.body_excerpt,
             thread_title      = excluded.thread_title,
             thread_source_url = excluded.thread_source_url"#,
    )
    .bind(thread_id)
    .bind(post_index)
    .bind(if has_post_number { 1_i64 } else { 0 })
    .bind(author)
    .bind(timestamp_raw)
    .bind(body_html)
    .bind(body_excerpt)
    .bind(thread_title)
    .bind(thread_source_url)
    .bind(chrono::Utc::now().timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn unfavorite_post(pool: &SqlitePool, thread_id: i64, post_index: i64) -> Result<()> {
    sqlx::query("DELETE FROM favorite_posts WHERE thread_id = ? AND post_index = ?")
        .bind(thread_id)
        .bind(post_index)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn is_post_favorited(
    pool: &SqlitePool,
    thread_id: i64,
    post_index: i64,
) -> Result<bool> {
    let r: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM favorite_posts WHERE thread_id = ? AND post_index = ? LIMIT 1",
    )
    .bind(thread_id)
    .bind(post_index)
    .fetch_optional(pool)
    .await?;
    Ok(r.is_some())
}

pub async fn list_thread_favorite_indexes(
    pool: &SqlitePool,
    thread_id: i64,
) -> Result<Vec<i64>> {
    let rows = sqlx::query(
        "SELECT post_index FROM favorite_posts WHERE thread_id = ? ORDER BY post_index ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.get::<i64, _>("post_index")).collect())
}

pub async fn list_favorite_posts(pool: &SqlitePool) -> Result<Vec<FavoritePostRow>> {
    let rows = sqlx::query(
        r#"SELECT thread_id, post_index, has_post_number, author, timestamp_raw,
                  body_html, body_excerpt, thread_title, thread_source_url, favorited_at
             FROM favorite_posts
             ORDER BY favorited_at DESC, thread_id DESC, post_index ASC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(favorite_post_row).collect())
}

/// Whether a thread has any favorited posts. Used as the pin-cache signal for
/// any future eviction logic on `thread_post_cache`: pinned threads stay.
pub async fn thread_has_favorites(pool: &SqlitePool, thread_id: i64) -> Result<bool> {
    let r: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM favorite_posts WHERE thread_id = ? LIMIT 1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await?;
    Ok(r.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_then_list_returns_what_was_written() {
        // Regression for the autopia.org "ghost write" bug: the seed-on-
        // startup path called upsert_site_profile with no error, but the
        // dispatcher's list_site_profiles_for_host returned [] for the same
        // host 40s later. This test exercises the exact sequence in-process
        // so we can isolate whether the bug is in the DB layer.
        let pool = open_pool("sqlite::memory:").await.unwrap();

        upsert_site_profile(
            &pool,
            "www.autopia.org",
            "xenforo_text_thread",
            "article.message.message--post.js-post",
            r#"{"content_type":"xenforo_text_thread","content_probe":"article.message.message--post.js-post","post_selector":"article.message.message--post.js-post"}"#,
            "seed",
        )
        .await
        .unwrap();

        let profiles = list_site_profiles_for_host(&pool, "www.autopia.org")
            .await
            .unwrap();
        assert_eq!(profiles.len(), 1, "expected the seed write to be visible");
        assert_eq!(profiles[0].source, "seed");
        assert_eq!(profiles[0].content_type, "xenforo_text_thread");

        let one = get_site_profile(&pool, "www.autopia.org", "xenforo_text_thread")
            .await
            .unwrap();
        assert!(one.is_some(), "get_site_profile should also find the row");
        assert_eq!(one.unwrap().content_probe, "article.message.message--post.js-post");
    }

    #[tokio::test]
    async fn schema_bootstraps_in_memory() {
        let pool = open_pool("sqlite::memory:").await.unwrap();
        let row =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();
        let names: Vec<String> = row
            .into_iter()
            .map(|r| r.get::<String, _>("name"))
            .collect();
        assert!(names.contains(&"categories".to_string()));
        assert!(names.contains(&"forums".to_string()));
        assert!(names.contains(&"threads".to_string()));
    }

    #[tokio::test]
    async fn round_trip_a_thread() {
        let pool = open_pool("sqlite::memory:").await.unwrap();
        let c = upsert_category(&pool, "Cars", 0).await.unwrap();
        let f = upsert_forum(&pool, c, "reddit", "BMW Discussion", "https://x", None, 1800, None)
            .await
            .unwrap();
        upsert_thread(
            &pool,
            f,
            &ThreadInput {
                source_url: "https://x/1".into(),
                title: "Hello".into(),
                op_author: Some("alice".into()),
                pubdate: Some(1_700_000_000),
                reply_count: Some(7),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        refresh_forum_stats(&pool, f, 1_700_000_001).await.unwrap();
        let ts = list_forum_threads(&pool, f, 10, 0, SortMode::New).await.unwrap();
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].title, "Hello");

        // latest-thread denorm now populated.
        let forum = get_forum(&pool, f).await.unwrap().unwrap();
        assert_eq!(forum.latest_thread_title.as_deref(), Some("Hello"));
        assert_eq!(forum.latest_thread_author.as_deref(), Some("alice"));
        assert_eq!(forum.latest_thread_at, Some(1_700_000_000));
        // never visited → unread
        assert!(forum.unread);

        // mark visited at a later time → no longer unread
        mark_forum_visited(&pool, f, 1_700_000_010).await.unwrap();
        let forum = get_forum(&pool, f).await.unwrap().unwrap();
        assert!(!forum.unread);
    }
}
