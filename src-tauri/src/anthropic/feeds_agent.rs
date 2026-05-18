// Conversational agent for editing feeds.yaml. Wraps the Anthropic Messages
// API with:
//   - Anthropic's server-side `web_search` tool (so the agent can look up
//     RSS endpoints without us hosting a search backend)
//   - Surgical local tools: list_categories, list_forums_in_category,
//     get_forum, add_or_update_forum, remove_forum, add_category,
//     rename_category, remove_category, revert_last
//
// Why surgical (no read-the-whole-file): for a 100+ forum config, dumping
// the entire YAML into context on every tool call would waste several
// thousand tokens per turn. The agent only needs to see the slice it's
// editing.
//
// The agent runs a loop until the model emits a non-`tool_use` stop_reason,
// up to MAX_TOOL_TURNS turns. Progress is reported via the Tauri event
// `feeds_agent_progress` so the UI can show step-by-step what's happening.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::config::{AppConfig, CategoryConfig, ForumConfig, ForumKind};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 4096;
const MAX_TOOL_TURNS: usize = 16;

const SYSTEM_PROMPT: &str = r#"You are the feed-configuration assistant for feedBulletin, a forum reader.

The user will ask you to add, remove, or reconfigure forum sources. You have surgical tools that operate on individual entries — you never need to read or write the whole config file.

## Tool inventory

- web_search: look up forum RSS endpoints, sub names, or other discovery
- list_categories(): returns the list of category names + forum counts
- list_forums_in_category(category): returns title + kind for each forum in a category
- get_forum(category, title): returns the full config object for one forum
- add_or_update_forum(category, forum): insert or replace a forum (matched by title within category). Pass the full forum object: { kind, title, url?, sub?, user?, description?, poll_interval_s?, disabled? }
- remove_forum(category, title): delete a forum by title within a category
- add_category(name, position?): create a new category. Position is 0-based; omit to append.
- rename_category(old_name, new_name): rename a category, preserving its forums
- remove_category(name): delete a category and all its forums (use with care)

## Schema cheat sheet

Each forum has:
- kind (required): one of `reddit | xenforo | vbulletin | discourse | phpbb | smf | ipb | ubiquiti | generic_rss`
- title (optional): defaults to URL or capitalized sub
- url (required for non-reddit): the RSS feed URL
- sub (reddit only): subreddit name without `r/`
- user (reddit only): username without `u/`
- description (optional)
- poll_interval_s (optional, default 1800; minimum 60)
- disabled (optional bool)

## Rules

1. Start with list_categories() to know what categories exist.
2. For non-Reddit forums you MUST find the actual RSS endpoint via web_search.
   Common patterns:
   - vBulletin: `<root>/external.php?type=RSS2`
   - XenForo: `<root>/forums/-/index.rss`
   - Discourse: `<root>/latest.rss`
   - phpBB: `<root>/feed.php`
   Confirm by searching for the specific forum's name + "RSS" and read the result before deciding.
3. For Reddit, use `kind: reddit` with `sub:` (subreddit) or `user:` (user). NO url needed.
4. Pick an existing category that fits; if none, propose a new one with add_category.
5. Edits apply immediately on disk and are snapshotted (the user can revert).
6. After you finish editing, tell the user to restart feedBulletin for changes to take effect.
7. Be concise. Don't paste the full YAML back at the user — describe what changed."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    pub messages: Vec<ChatMessage>,
    pub did_modify_feeds: bool,
    pub final_text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    stop_reason: Option<String>,
}

pub async fn run_agent(
    app: &AppHandle,
    feeds_path: &Path,
    history: Vec<ChatMessage>,
    user_message: String,
) -> Result<AgentResult> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        anyhow!("ANTHROPIC_API_KEY not set — configure it via 1Password to use the AI assistant")
    })?;

    let mut messages = history;
    messages.push(ChatMessage {
        role: "user".into(),
        content: Value::String(user_message),
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("building anthropic http client")?;

    let tools = tool_definitions();

    let mut did_modify_feeds = false;
    let mut final_text = String::new();

    for turn in 0..MAX_TOOL_TURNS {
        let req_body = json!({
            "model": MODEL,
            "max_tokens": MAX_TOKENS,
            "system": SYSTEM_PROMPT,
            "tools": tools,
            "messages": messages.iter().map(|m| json!({
                "role": m.role,
                "content": m.content,
            })).collect::<Vec<_>>(),
        });

        emit_progress(app, "thinking", json!({ "turn": turn }));

        let resp = client
            .post(ANTHROPIC_URL)
            .header("x-api-key", &api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&req_body)
            .send()
            .await
            .context("calling anthropic")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("anthropic API returned {}: {}", status, truncate(&body, 400));
        }

        let parsed: MessagesResponse = resp.json().await.context("parsing anthropic response")?;
        let content_blocks: Vec<Value> = parsed.content.clone();

        messages.push(ChatMessage {
            role: "assistant".into(),
            content: Value::Array(content_blocks.clone()),
        });

        let mut tool_uses: Vec<(String, String, Value)> = Vec::new();
        for block in &content_blocks {
            let ty = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match ty {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        if !final_text.is_empty() {
                            final_text.push_str("\n\n");
                        }
                        final_text.push_str(t);
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    tool_uses.push((id, name, input));
                }
                _ => {}
            }
        }

        let stop = parsed.stop_reason.as_deref().unwrap_or("");

        let local_tools: Vec<_> = tool_uses
            .iter()
            .filter(|(_, name, _)| is_local_tool(name))
            .collect();

        if local_tools.is_empty() {
            // No local tool calls. Either pure text response or server-side
            // tools only — done either way.
            break;
        }

        let mut tool_results: Vec<Value> = Vec::new();
        for (id, name, input) in &tool_uses {
            if !is_local_tool(name) {
                continue;
            }
            emit_progress(app, "tool", json!({ "name": name, "input": input }));
            let result = execute_tool(feeds_path, name, input, &mut did_modify_feeds).await;
            if matches!(name.as_str(), "add_or_update_forum" | "remove_forum" | "add_category" | "rename_category" | "remove_category") {
                emit_progress(app, "applied", json!({ "tool": name }));
            }
            tool_results.push(json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": serde_json::to_string(&result).unwrap_or_default(),
            }));
        }

        messages.push(ChatMessage {
            role: "user".into(),
            content: Value::Array(tool_results),
        });

        if stop != "tool_use" {
            // Model is done but asked for tools; loop once more to let it
            // digest the results.
        }
    }

    if final_text.is_empty() {
        final_text = "(no text response from the model)".to_string();
    }

    info!(did_modify_feeds, "feeds agent run complete");
    Ok(AgentResult {
        messages,
        did_modify_feeds,
        final_text,
    })
}

fn is_local_tool(name: &str) -> bool {
    matches!(
        name,
        "list_categories"
            | "list_forums_in_category"
            | "get_forum"
            | "add_or_update_forum"
            | "remove_forum"
            | "add_category"
            | "rename_category"
            | "remove_category"
    )
}

fn tool_definitions() -> Value {
    json!([
        { "type": "web_search_20250305", "name": "web_search", "max_uses": 5 },
        {
            "name": "list_categories",
            "description": "List all category names with the number of forums in each. Use this to see the overall config without dumping the whole file.",
            "input_schema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "list_forums_in_category",
            "description": "List forums within a category — title + kind + URL/sub identifier. Use this to find what's already configured for a topic area.",
            "input_schema": {
                "type": "object",
                "properties": { "category": { "type": "string" } },
                "required": ["category"],
                "additionalProperties": false
            }
        },
        {
            "name": "get_forum",
            "description": "Read the full config for one forum (all fields including description, poll_interval_s, etc).",
            "input_schema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string" },
                    "title": { "type": "string" }
                },
                "required": ["category", "title"],
                "additionalProperties": false
            }
        },
        {
            "name": "add_or_update_forum",
            "description": "Add a new forum or replace an existing one matched by title within a category. The forum must validate against the schema; invalid forums are rejected with an error.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Name of an existing category. Use add_category first if it doesn't exist." },
                    "forum": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["reddit", "xenforo", "vbulletin", "discourse", "phpbb", "smf", "ipb", "ubiquiti", "generic_rss"] },
                            "title": { "type": "string" },
                            "url": { "type": "string", "description": "Required for non-reddit kinds. Must be the RSS endpoint URL." },
                            "sub": { "type": "string", "description": "Reddit subreddit name (no r/ prefix). Required for kind: reddit unless `user` is set." },
                            "user": { "type": "string", "description": "Reddit username (no u/ prefix)." },
                            "description": { "type": "string" },
                            "poll_interval_s": { "type": "integer", "minimum": 60 },
                            "disabled": { "type": "boolean" }
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    }
                },
                "required": ["category", "forum"],
                "additionalProperties": false
            }
        },
        {
            "name": "remove_forum",
            "description": "Delete a forum by title within a category.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string" },
                    "title": { "type": "string" }
                },
                "required": ["category", "title"],
                "additionalProperties": false
            }
        },
        {
            "name": "add_category",
            "description": "Create a new (empty) category. Pass position to insert at a specific index (0-based); omit to append.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "position": { "type": "integer", "minimum": 0 }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        },
        {
            "name": "rename_category",
            "description": "Rename a category, preserving its forums.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "old_name": { "type": "string" },
                    "new_name": { "type": "string" }
                },
                "required": ["old_name", "new_name"],
                "additionalProperties": false
            }
        },
        {
            "name": "remove_category",
            "description": "Delete a category and all its forums. Confirm with the user before doing this.",
            "input_schema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"],
                "additionalProperties": false
            }
        }
    ])
}

async fn execute_tool(
    feeds_path: &Path,
    name: &str,
    input: &Value,
    did_modify: &mut bool,
) -> Value {
    let cfg = match read_config(feeds_path) {
        Ok(c) => c,
        Err(e) => return json!({ "error": format!("could not read feeds.yaml: {}", e) }),
    };

    match name {
        "list_categories" => {
            let cats: Vec<Value> = cfg
                .categories
                .iter()
                .map(|c| json!({ "name": c.name, "forum_count": c.forums.len() }))
                .collect();
            json!({ "categories": cats })
        }
        "list_forums_in_category" => {
            let cat = string_arg(input, "category");
            match cfg.categories.iter().find(|c| c.name == cat) {
                None => json!({ "error": format!("no category named '{}'", cat) }),
                Some(c) => {
                    let forums: Vec<Value> = c
                        .forums
                        .iter()
                        .map(|f| {
                            json!({
                                "title": f.resolved_title(),
                                "kind": f.kind.as_str(),
                                "url": f.url.as_deref(),
                                "sub": f.sub.as_deref(),
                                "user": f.user.as_deref(),
                                "disabled": f.disabled,
                            })
                        })
                        .collect();
                    json!({ "forums": forums })
                }
            }
        }
        "get_forum" => {
            let cat = string_arg(input, "category");
            let title = string_arg(input, "title");
            match cfg.categories.iter().find(|c| c.name == cat) {
                None => json!({ "error": format!("no category named '{}'", cat) }),
                Some(c) => match c.forums.iter().find(|f| f.resolved_title() == title) {
                    None => json!({ "error": format!("no forum titled '{}' in '{}'", title, cat) }),
                    Some(f) => json!(f),
                },
            }
        }
        "add_or_update_forum" => {
            let cat = string_arg(input, "category");
            let forum_json = input.get("forum").cloned().unwrap_or(Value::Null);
            let forum: ForumConfig = match serde_json::from_value(forum_json) {
                Ok(f) => f,
                Err(e) => return json!({ "error": format!("invalid forum object: {}", e) }),
            };
            if let Err(e) = forum_invariants(&forum) {
                return json!({ "error": e });
            }
            let mut new_cfg = cfg.clone();
            let Some(target_cat) = new_cfg.categories.iter_mut().find(|c| c.name == cat) else {
                return json!({ "error": format!("no category named '{}'. Use add_category first.", cat) });
            };
            let new_title = forum.resolved_title();
            let position = target_cat
                .forums
                .iter()
                .position(|f| f.resolved_title() == new_title);
            match position {
                Some(idx) => target_cat.forums[idx] = forum.clone(),
                None => target_cat.forums.push(forum.clone()),
            }
            match commit(feeds_path, &new_cfg) {
                Ok(snap) => {
                    *did_modify = true;
                    json!({
                        "ok": true,
                        "action": position.map(|_| "updated").unwrap_or("added"),
                        "title": new_title,
                        "category": cat,
                        "snapshot": snap,
                    })
                }
                Err(e) => json!({ "error": format!("write failed: {}", e) }),
            }
        }
        "remove_forum" => {
            let cat = string_arg(input, "category");
            let title = string_arg(input, "title");
            let mut new_cfg = cfg.clone();
            let Some(target_cat) = new_cfg.categories.iter_mut().find(|c| c.name == cat) else {
                return json!({ "error": format!("no category named '{}'", cat) });
            };
            let before = target_cat.forums.len();
            target_cat.forums.retain(|f| f.resolved_title() != title);
            if target_cat.forums.len() == before {
                return json!({ "error": format!("no forum titled '{}' in '{}'", title, cat) });
            }
            match commit(feeds_path, &new_cfg) {
                Ok(snap) => {
                    *did_modify = true;
                    json!({ "ok": true, "removed": title, "category": cat, "snapshot": snap })
                }
                Err(e) => json!({ "error": format!("write failed: {}", e) }),
            }
        }
        "add_category" => {
            let name_arg = string_arg(input, "name");
            if name_arg.trim().is_empty() {
                return json!({ "error": "name must be non-empty" });
            }
            if cfg.categories.iter().any(|c| c.name == name_arg) {
                return json!({ "error": format!("category '{}' already exists", name_arg) });
            }
            let position = input
                .get("position")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let mut new_cfg = cfg.clone();
            let new_cat = CategoryConfig {
                name: name_arg.clone(),
                forums: vec![],
            };
            match position {
                Some(p) if p <= new_cfg.categories.len() => new_cfg.categories.insert(p, new_cat),
                _ => new_cfg.categories.push(new_cat),
            }
            match commit(feeds_path, &new_cfg) {
                Ok(snap) => {
                    *did_modify = true;
                    json!({ "ok": true, "added_category": name_arg, "snapshot": snap })
                }
                Err(e) => json!({ "error": format!("write failed: {}", e) }),
            }
        }
        "rename_category" => {
            let old = string_arg(input, "old_name");
            let new = string_arg(input, "new_name");
            if new.trim().is_empty() {
                return json!({ "error": "new_name must be non-empty" });
            }
            if cfg.categories.iter().any(|c| c.name == new) && old != new {
                return json!({ "error": format!("a category named '{}' already exists", new) });
            }
            let mut new_cfg = cfg.clone();
            let Some(target) = new_cfg.categories.iter_mut().find(|c| c.name == old) else {
                return json!({ "error": format!("no category named '{}'", old) });
            };
            target.name = new.clone();
            match commit(feeds_path, &new_cfg) {
                Ok(snap) => {
                    *did_modify = true;
                    json!({ "ok": true, "renamed": [old, new], "snapshot": snap })
                }
                Err(e) => json!({ "error": format!("write failed: {}", e) }),
            }
        }
        "remove_category" => {
            let name_arg = string_arg(input, "name");
            let mut new_cfg = cfg.clone();
            let before = new_cfg.categories.len();
            new_cfg.categories.retain(|c| c.name != name_arg);
            if new_cfg.categories.len() == before {
                return json!({ "error": format!("no category named '{}'", name_arg) });
            }
            match commit(feeds_path, &new_cfg) {
                Ok(snap) => {
                    *did_modify = true;
                    json!({ "ok": true, "removed_category": name_arg, "snapshot": snap })
                }
                Err(e) => json!({ "error": format!("write failed: {}", e) }),
            }
        }
        _ => json!({ "error": format!("unknown tool '{}'", name) }),
    }
}

fn read_config(path: &Path) -> Result<AppConfig> {
    AppConfig::from_path(path)
}

/// Enforce the same per-kind invariants the schema validator runs (the
/// Anthropic input_schema is too coarse to express "reddit needs sub or user
/// xor non-reddit needs url"). Returns Err(message) on invariant violation.
fn forum_invariants(f: &ForumConfig) -> std::result::Result<(), String> {
    match f.kind {
        ForumKind::Reddit => {
            if f.url.is_none() && f.sub.is_none() && f.user.is_none() {
                return Err("reddit forum needs one of 'sub', 'user', or 'url'".into());
            }
        }
        _ => {
            if f.url.is_none() {
                return Err(format!(
                    "kind '{}' requires a 'url' field",
                    f.kind.as_str()
                ));
            }
        }
    }
    if let Some(u) = &f.url {
        if !u.starts_with("http://") && !u.starts_with("https://") {
            return Err(format!("url must start with http(s):// — got '{}'", u));
        }
    }
    if let Some(n) = f.poll_interval_s {
        if n < 60 {
            return Err("poll_interval_s must be at least 60 seconds".into());
        }
    }
    Ok(())
}

/// Serialize the config to YAML, snapshot the existing file, then write.
fn commit(feeds_path: &Path, cfg: &AppConfig) -> Result<String> {
    let yaml = serde_yaml::to_string(cfg).context("serializing AppConfig")?;
    // Validate round-trip: re-parse what we're about to write so we never
    // commit something we couldn't parse back.
    AppConfig::from_str(&yaml).context("round-trip parse of serialized YAML failed")?;

    let parent = feeds_path.parent().unwrap_or(Path::new("."));
    let snapshots_dir = parent.join("feeds.yaml.snapshots");
    std::fs::create_dir_all(&snapshots_dir).ok();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let snapshot_path = snapshots_dir.join(format!("{}.yaml", ts));
    if feeds_path.exists() {
        if let Ok(prev) = std::fs::read_to_string(feeds_path) {
            std::fs::write(&snapshot_path, prev).ok();
        }
    }
    std::fs::write(feeds_path, &yaml).context("writing feeds.yaml")?;
    prune_snapshots(&snapshots_dir, 20).ok();
    Ok(snapshot_path.display().to_string())
}

fn prune_snapshots(dir: &Path, keep: usize) -> Result<()> {
    let mut entries: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("yaml"))
        .filter_map(|e| {
            let stem = e.path().file_stem()?.to_str()?.to_string();
            let ts: u64 = stem.parse().ok()?;
            Some((ts, e.path()))
        })
        .collect();
    entries.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    for (_, path) in entries.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Restore the most-recent snapshot. Returns the YAML content that was
/// restored, or None if there are no snapshots.
pub fn revert_to_previous_snapshot(feeds_path: &Path) -> Result<Option<String>> {
    let parent = feeds_path.parent().unwrap_or(Path::new("."));
    let snapshots_dir = parent.join("feeds.yaml.snapshots");
    if !snapshots_dir.exists() {
        return Ok(None);
    }
    let mut entries: Vec<(u64, PathBuf)> = std::fs::read_dir(&snapshots_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("yaml"))
        .filter_map(|e| {
            let stem = e.path().file_stem()?.to_str()?.to_string();
            let ts: u64 = stem.parse().ok()?;
            Some((ts, e.path()))
        })
        .collect();
    entries.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let Some((_, latest)) = entries.first() else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(latest)
        .with_context(|| format!("reading snapshot {}", latest.display()))?;
    if feeds_path.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let pre_revert = snapshots_dir.join(format!("{}.yaml", ts));
        if let Ok(cur) = std::fs::read_to_string(feeds_path) {
            let _ = std::fs::write(&pre_revert, cur);
        }
    }
    std::fs::write(feeds_path, &content)
        .with_context(|| format!("writing {}", feeds_path.display()))?;
    let _ = std::fs::remove_file(latest);
    Ok(Some(content))
}

fn emit_progress(app: &AppHandle, kind: &str, data: Value) {
    let payload = json!({ "kind": kind, "data": data });
    if let Err(e) = app.emit("feeds_agent_progress", payload) {
        warn!(err = %e, "failed to emit feeds_agent_progress");
    }
}

fn string_arg(input: &Value, key: &str) -> String {
    input.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
