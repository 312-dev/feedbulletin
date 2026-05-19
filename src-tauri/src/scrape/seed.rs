//! Bundled site-profile seed data.
//!
//! When the app starts and finds the `site_profiles` table empty for one of
//! the well-known hosts below, it inserts the bundled profile so the user's
//! first thread render works without burning an Anthropic call to re-derive
//! selectors that haven't moved in years.
//!
//! Adding a new seed:
//!   1. Drop a `<host>__<content_type>.md` file under `seed_profiles/` with
//!      the YAML frontmatter + ```json code-fence shape used by the existing
//!      files.
//!   2. Add the new filename to `SEEDS` below.
//!
//! Removing a seed: just delete the array entry and the file. Already-seeded
//! profiles in user DBs are left intact (we never delete on startup).

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{debug, info, warn};

/// One bundled profile MD file per slot. Each `(filename, contents)` pair
/// gets parsed and considered for insertion at startup.
///
/// Filenames are documentation only — the actual identity (host,
/// content_type) is parsed out of the frontmatter inside each file. This
/// keeps a typo in a filename from accidentally renaming a host.
const SEEDS: &[(&str, &str)] = &[
    (
        "community.ui.com__discourse_topic.md",
        include_str!("seed_profiles/community.ui.com__discourse_topic.md"),
    ),
    (
        "www.e90post.com__vbulletin_thread.md",
        include_str!("seed_profiles/www.e90post.com__vbulletin_thread.md"),
    ),
    (
        "www.garagejournal.com__xenforo_text_thread.md",
        include_str!("seed_profiles/www.garagejournal.com__xenforo_text_thread.md"),
    ),
    (
        "xdaforums.com__xenforo_text_thread.md",
        include_str!("seed_profiles/xdaforums.com__xenforo_text_thread.md"),
    ),
    (
        "g80.bimmerpost.com__vbulletin_thread.md",
        include_str!("seed_profiles/g80.bimmerpost.com__vbulletin_thread.md"),
    ),
    (
        "www.bimmerpost.com__vbulletin_thread.md",
        include_str!("seed_profiles/www.bimmerpost.com__vbulletin_thread.md"),
    ),
    (
        "www.autopia.org__xenforo_text_thread.md",
        include_str!("seed_profiles/www.autopia.org__xenforo_text_thread.md"),
    ),
];

#[derive(Debug)]
struct SeedProfile {
    host: String,
    content_type: String,
    content_probe: String,
    selectors_json: String,
}

/// Parse a single seed MD file. The format is fixed — a YAML frontmatter
/// block (`---` … `---`) carrying host/content_type/content_probe, followed
/// somewhere later in the file by a single ```json fenced block containing
/// the canonical SelectorSet JSON.
///
/// The free-form narrative below the JSON block is for human readers; the
/// Rust loader ignores it.
fn parse_seed(src: &str) -> Option<SeedProfile> {
    let after_open = src.strip_prefix("---\n")?;
    let (frontmatter, rest) = after_open.split_once("\n---\n")?;

    let mut host: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut content_probe: Option<String> = None;
    for line in frontmatter.lines() {
        let line = line.trim_end();
        if let Some(v) = line.strip_prefix("host: ") {
            host = Some(unquote(v.trim()).to_string());
        } else if let Some(v) = line.strip_prefix("content_type: ") {
            content_type = Some(unquote(v.trim()).to_string());
        } else if let Some(v) = line.strip_prefix("content_probe: ") {
            content_probe = Some(unquote(v.trim()).to_string());
        }
    }

    let json_open = rest.find("```json\n")?;
    let after_open = &rest[json_open + "```json\n".len()..];
    let json_close = after_open.find("\n```")?;
    let selectors_json = after_open[..json_close].to_string();

    Some(SeedProfile {
        host: host?,
        content_type: content_type?,
        content_probe: content_probe?,
        selectors_json,
    })
}

/// Trim a single layer of matching ASCII quotes (single or double), if any.
fn unquote(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Insert any bundled profile not already present in the user's DB.
///
/// Override policy:
///   - source = "manual" / "anthropic-critique" / "seed" → never override
///     (manual = user direct edit; anthropic-critique = post-render validation
///      refinement; seed = already a bundled profile).
///   - source = "anthropic" → OVERRIDE if a bundled seed exists. Reason: a
///     raw one-shot learner output is inherently best-effort, and when we
///     ship a hand-verified seed for the same host, the seed is almost
///     always better. Bumps the seed in for users who hit the host before
///     this release added the bundled profile.
///   - any other / unknown source → treat as user-touched, don't override.
///
/// Idempotent and safe to call on every startup. Returns the count actually
/// inserted or overridden.
pub async fn seed_missing_profiles(pool: &SqlitePool) -> Result<usize> {
    let mut written = 0usize;
    for (name, body) in SEEDS {
        let Some(p) = parse_seed(body) else {
            warn!("seed profile {name} failed to parse — skipping");
            continue;
        };
        let existing = crate::db::get_site_profile(pool, &p.host, &p.content_type).await?;
        match existing.as_ref().map(|e| e.source.as_str()) {
            None => {
                // No profile at all → fresh insert.
            }
            Some("anthropic") => {
                info!(
                    "overriding stale anthropic-learned profile with bundled seed: {} → {}",
                    p.host, p.content_type
                );
            }
            Some(other) => {
                debug!(
                    "seed {} → {} already present (source={}), skipping",
                    p.host, p.content_type, other
                );
                continue;
            }
        }
        crate::db::upsert_site_profile(
            pool,
            &p.host,
            &p.content_type,
            &p.content_probe,
            &p.selectors_json,
            "seed",
        )
        .await?;

        // DIAGNOSTIC: read back what we just wrote and log it. Catches the
        // case where the upsert returns Ok but the row isn't queryable
        // afterward (transaction visibility, host normalization, etc.).
        match crate::db::get_site_profile(pool, &p.host, &p.content_type).await {
            Ok(Some(r)) => {
                info!(
                    host = %p.host,
                    content_type = %p.content_type,
                    source = %r.source,
                    "seed wrote+verified"
                );
            }
            Ok(None) => {
                warn!(
                    host = %p.host,
                    content_type = %p.content_type,
                    "seed upsert returned Ok but readback found nothing!"
                );
            }
            Err(e) => {
                warn!(host = %p.host, err = %e, "seed readback errored");
            }
        }

        written += 1;
        if existing.is_none() {
            info!("seeded site profile: {} → {}", p.host, p.content_type);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_seed_file_parses() {
        // Catches typos in the YAML frontmatter or a missing/malformed JSON
        // fence at compile-into-the-binary time.
        for (name, body) in SEEDS {
            let p = parse_seed(body).unwrap_or_else(|| panic!("failed to parse {name}"));
            assert!(!p.host.is_empty(), "{name}: empty host");
            assert!(!p.content_type.is_empty(), "{name}: empty content_type");
            assert!(!p.content_probe.is_empty(), "{name}: empty content_probe");
            // Selectors JSON must round-trip through serde_json so it's
            // guaranteed to be insertable as TEXT and re-parseable later.
            let _: serde_json::Value = serde_json::from_str(&p.selectors_json)
                .unwrap_or_else(|e| panic!("{name}: invalid JSON: {e}"));
        }
    }

    #[test]
    fn parses_unquoted_values() {
        let src = "---\nhost: x.y.z\ncontent_type: foo\ncontent_probe: #root\n---\n\n```json\n{\"a\": 1}\n```\n";
        let p = parse_seed(src).unwrap();
        assert_eq!(p.host, "x.y.z");
        assert_eq!(p.content_type, "foo");
        assert_eq!(p.content_probe, "#root");
        assert_eq!(p.selectors_json, "{\"a\": 1}");
    }

    #[test]
    fn parses_double_quoted_probe() {
        let src = "---\nhost: x\ncontent_type: y\ncontent_probe: \"div#posts, table[id]\"\n---\n\n```json\n{}\n```\n";
        let p = parse_seed(src).unwrap();
        assert_eq!(p.content_probe, "div#posts, table[id]");
    }
}
