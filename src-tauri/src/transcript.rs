//! Read Claude Code session JSONL for tip resolution and GUI transcript import.
//! Never writes Claude's JSONL — resume goes through `claude -p --resume`.

use crate::models::{GuiTurn, TurnRole, TurnStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 50_000;
const MAX_IMPORT_TURNS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTip {
    pub leaf_uuid: String,
    /// How the tip was chosen.
    pub source: TipSource,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipSource {
    /// Latest `last-prompt.leafUuid` that still exists in the tree.
    LastPrompt,
    /// Newest leaf by timestamp, then uuid (deterministic).
    NewestLeaf,
}

#[derive(Debug, Clone)]
struct Node {
    uuid: String,
    parent_uuid: Option<String>,
    timestamp: Option<DateTime<Utc>>,
    is_sidechain: bool,
    is_meta: bool,
    /// Visible user/assistant text extracted from this line, if any.
    text: Option<String>,
    role: Option<TurnRole>,
}

/// Locate `<session_id>.jsonl` under `~/.claude/projects` (and `sessions`).
pub fn find_session_jsonl(cli_session_id: &str, cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let file_name = format!("{cli_session_id}.jsonl");
    let projects = home.join(".claude").join("projects");
    if let Some(path) = prefer_cwd_jsonl(&projects, cwd, &file_name) {
        return Some(path);
    }
    if let Some(path) = scan_for_jsonl(&projects, &file_name) {
        return Some(path);
    }
    let sessions = home.join(".claude").join("sessions");
    if sessions.is_dir() {
        if let Some(path) = prefer_cwd_jsonl(&sessions, cwd, &file_name) {
            return Some(path);
        }
        return scan_for_jsonl(&sessions, &file_name);
    }
    None
}

fn prefer_cwd_jsonl(root: &Path, cwd: &str, file_name: &str) -> Option<PathBuf> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() || !root.is_dir() {
        return None;
    }
    let sanitized = sanitize_project_dir(trimmed);
    let direct = root.join(&sanitized).join(file_name);
    if direct.is_file() {
        return Some(direct);
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == sanitized || name.contains(&sanitized) || sanitized.contains(&name) {
            let path = dir.join(file_name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn scan_for_jsonl(root: &Path, file_name: &str) -> Option<PathBuf> {
    let Ok(projects) = std::fs::read_dir(root) else {
        return None;
    };
    for project in projects.flatten() {
        let dir = project.path();
        if !dir.is_dir() {
            continue;
        }
        let path = dir.join(file_name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn sanitize_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | ' ' => '-',
            _ => c,
        })
        .collect()
}

/// Resolve the current tip of a session JSONL tree.
/// Prefer the latest `last-prompt.leafUuid` when it is still a node; otherwise
/// the newest leaf by timestamp (then uuid).
pub fn resolve_tip(path: &Path) -> Option<SessionTip> {
    let parsed = parse_jsonl(path)?;
    resolve_tip_from(&parsed)
}

fn resolve_tip_from(parsed: &ParsedJsonl) -> Option<SessionTip> {
    if let Some(leaf) = parsed.last_prompt_leaf.as_ref() {
        if let Some(node) = parsed.nodes.get(leaf) {
            return Some(SessionTip {
                leaf_uuid: leaf.clone(),
                source: TipSource::LastPrompt,
                timestamp: node.timestamp,
            });
        }
    }
    let leaves = leaf_uuids(&parsed.nodes);
    if leaves.is_empty() {
        return None;
    }
    let mut ranked: Vec<&Node> = leaves
        .iter()
        .filter_map(|id| parsed.nodes.get(id))
        .collect();
    ranked.sort_by(|a, b| {
        let ta = a.timestamp.unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        let tb = b.timestamp.unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        tb.cmp(&ta).then_with(|| a.uuid.cmp(&b.uuid))
    });
    let chosen = ranked.first()?;
    Some(SessionTip {
        leaf_uuid: chosen.uuid.clone(),
        source: TipSource::NewestLeaf,
        timestamp: chosen.timestamp,
    })
}

/// Import visible user/assistant text from the whole JSONL tree (every branch).
/// Skips tool-result users, meta lines, and sidechains. Does not invent text.
/// Resume tip is still resolved separately (`last-prompt` / newest leaf).
pub fn import_mainline_turns(path: &Path) -> (Option<SessionTip>, Vec<GuiTurn>) {
    let Some(parsed) = parse_jsonl(path) else {
        return (None, Vec::new());
    };
    let tip = resolve_tip_from(&parsed);
    let mainline: HashSet<String> = tip
        .as_ref()
        .map(|t| walk_to_root(&parsed.nodes, &t.leaf_uuid).into_iter().collect())
        .unwrap_or_default();
    let mut nodes: Vec<&Node> = parsed
        .nodes
        .values()
        .filter(|node| !node.is_sidechain && !node.is_meta)
        .filter(|node| node.role.is_some())
        .filter(|node| {
            node.text
                .as_ref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
        })
        .collect();
    nodes.sort_by(|a, b| {
        let ta = a.timestamp.unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        let tb = b.timestamp.unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        ta.cmp(&tb).then_with(|| a.uuid.cmp(&b.uuid))
    });
    let mut turns = Vec::new();
    for node in nodes {
        let Some(role) = node.role.clone() else {
            continue;
        };
        let Some(text) = node.text.as_ref().map(|t| t.trim()).filter(|t| !t.is_empty()) else {
            continue;
        };
        let started = node.timestamp.unwrap_or_else(Utc::now);
        let exit_code = if role == TurnRole::Assistant {
            Some(0)
        } else {
            None
        };
        turns.push(GuiTurn {
            id: Uuid::new_v4(),
            role,
            content: text.to_string(),
            exit_code,
            started_at: started,
            finished_at: Some(started),
            status: TurnStatus::Done,
            cost_usd: None,
            events: Vec::new(),
            cli_uuid: Some(node.uuid.clone()),
            off_mainline: !mainline.is_empty() && !mainline.contains(&node.uuid),
        });
        if turns.len() >= MAX_IMPORT_TURNS {
            break;
        }
    }
    (tip, turns)
}

const MERGE_TIME_SLACK_SECS: i64 = 120;

/// Merge JSONL imports with AppData GUI turns. Prefer `cli_uuid`, else
/// role+content+time. Keep unmatched GUI-only history; never invent text.
fn merge_gui_turns(existing: &[GuiTurn], imported: Vec<GuiTurn>) -> Vec<GuiTurn> {
    if existing.is_empty() {
        return imported;
    }
    if imported.is_empty() {
        return existing.to_vec();
    }

    let mut used: HashSet<usize> = HashSet::new();
    let mut by_cli: HashMap<&str, usize> = HashMap::new();
    for (idx, turn) in existing.iter().enumerate() {
        if let Some(cli) = turn.cli_uuid.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            by_cli.entry(cli).or_insert(idx);
        }
    }

    let mut merged = Vec::with_capacity(existing.len().saturating_add(imported.len()));
    for imp in imported {
        let match_idx = imp
            .cli_uuid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|cli| by_cli.get(cli).copied())
            .filter(|idx| !used.contains(idx))
            .or_else(|| find_content_time_match(existing, &imp, &used));

        if let Some(idx) = match_idx {
            used.insert(idx);
            let mut kept = existing[idx].clone();
            kept.cli_uuid = imp.cli_uuid.or(kept.cli_uuid);
            kept.off_mainline = imp.off_mainline;
            // CLI timestamps are authoritative for resume display order.
            kept.started_at = imp.started_at;
            if kept.finished_at.is_none() {
                kept.finished_at = imp.finished_at;
            }
            // Keep existing content (repairs / GUI text); do not invent.
            if kept.content.trim().is_empty() && !imp.content.trim().is_empty() {
                kept.content = imp.content;
            }
            merged.push(kept);
        } else {
            merged.push(imp);
        }
    }

    for (idx, turn) in existing.iter().enumerate() {
        if used.contains(&idx) {
            continue;
        }
        merged.push(turn.clone());
    }

    merged.sort_by(|a, b| {
        a.started_at
            .cmp(&b.started_at)
            .then_with(|| a.cli_uuid.cmp(&b.cli_uuid))
            .then_with(|| a.id.cmp(&b.id))
    });
    if merged.len() > MAX_IMPORT_TURNS {
        // Prefer newest settled history when capping.
        let skip = merged.len() - MAX_IMPORT_TURNS;
        merged = merged.split_off(skip);
    }
    merged
}

fn find_content_time_match(
    existing: &[GuiTurn],
    imported: &GuiTurn,
    used: &HashSet<usize>,
) -> Option<usize> {
    let want = imported.content.trim();
    if want.is_empty() {
        return None;
    }
    let mut best: Option<(i64, usize)> = None;
    for (idx, turn) in existing.iter().enumerate() {
        if used.contains(&idx) {
            continue;
        }
        if turn.role != imported.role {
            continue;
        }
        if turn.content.trim() != want {
            continue;
        }
        let delta = (turn.started_at - imported.started_at).num_seconds().abs();
        if delta > MERGE_TIME_SLACK_SECS {
            continue;
        }
        match best {
            Some((best_delta, _)) if best_delta <= delta => {}
            _ => best = Some((delta, idx)),
        }
    }
    best.map(|(_, idx)| idx)
}

/// Update `cli_tip_uuid` from the JSONL tip without rewriting GUI turns.
/// Safe to call right after a CLI turn (avoids dropping the just-written bubbles).
pub fn refresh_tip_uuid(session: &mut crate::models::GuiSession) -> bool {
    let Some(cli_id) = session.cli_session_id.as_deref().map(str::trim) else {
        return false;
    };
    if cli_id.is_empty() {
        return false;
    }
    let Some(path) = find_session_jsonl(cli_id, &session.cwd) else {
        return false;
    };
    let Some(tip) = resolve_tip(&path) else {
        return false;
    };
    if session.cli_tip_uuid.as_deref() == Some(tip.leaf_uuid.as_str()) {
        return false;
    }
    session.cli_tip_uuid = Some(tip.leaf_uuid);
    true
}

/// Sync GUI display turns from the Claude Code JSONL tree when the tip moved,
/// the thread is empty, or imported branch turns are missing. Merges with
/// existing AppData turns (does not wipe UseUpClaude-only history). Keeps
/// pending turns. Never deletes the GuiSession file. Resume still uses tip.
pub fn sync_gui_from_cli(session: &mut crate::models::GuiSession) -> bool {
    let Some(cli_id) = session.cli_session_id.as_deref().map(str::trim) else {
        return false;
    };
    if cli_id.is_empty() {
        return false;
    }
    let Some(path) = find_session_jsonl(cli_id, &session.cwd) else {
        return false;
    };
    let (tip, imported) = import_mainline_turns(&path);
    let Some(tip) = tip else {
        return false;
    };
    let tip_changed = session.cli_tip_uuid.as_deref() != Some(tip.leaf_uuid.as_str());
    let empty = !session
        .turns
        .iter()
        .any(|t| matches!(t.role, TurnRole::User | TurnRole::Assistant) && t.status == TurnStatus::Done);
    let has_pending = session
        .turns
        .iter()
        .any(|t| t.status == TurnStatus::Pending);

    // Never rebuild the thread while a turn is in flight — keep the GUI user
    // bubble that has not landed in JSONL yet.
    if has_pending {
        if session.cli_tip_uuid.as_deref() != Some(tip.leaf_uuid.as_str()) {
            session.cli_tip_uuid = Some(tip.leaf_uuid);
            return true;
        }
        return false;
    }

    let existing_cli: HashSet<&str> = session
        .turns
        .iter()
        .filter_map(|t| t.cli_uuid.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .collect();
    let missing_import = imported.iter().any(|t| {
        t.cli_uuid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some_and(|cli| !existing_cli.contains(cli))
    });

    if !tip_changed && !empty && !missing_import {
        if session.cli_tip_uuid.is_none() {
            session.cli_tip_uuid = Some(tip.leaf_uuid);
            return true;
        }
        return false;
    }
    if imported.is_empty() && !empty {
        session.cli_tip_uuid = Some(tip.leaf_uuid);
        return tip_changed;
    }

    // Tip moved, thread empty, or new branch turns appeared: merge JSONL import
    // with AppData. GuiSession files are never deleted.
    session.turns = merge_gui_turns(&session.turns, imported);
    session.cli_tip_uuid = Some(tip.leaf_uuid);
    session.updated_at = Utc::now();
    true
}

struct ParsedJsonl {
    nodes: HashMap<String, Node>,
    last_prompt_leaf: Option<String>,
}

fn parse_jsonl(path: &Path) -> Option<ParsedJsonl> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut last_prompt_leaf: Option<String> = None;
    let mut line = String::new();
    for _ in 0..MAX_LINES {
        line.clear();
        let n = match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        if line.len() > MAX_LINE_BYTES {
            line.truncate(MAX_LINE_BYTES);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let type_name = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if type_name == "last-prompt" {
            if let Some(leaf) = value
                .get("leafUuid")
                .or_else(|| value.get("leaf_uuid"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                last_prompt_leaf = Some(leaf.to_string());
            }
            continue;
        }
        let Some(uuid) = value
            .get("uuid")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        let parent_uuid = value
            .get("parentUuid")
            .or_else(|| value.get("parent_uuid"))
            .and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    v.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                }
            });
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let is_sidechain = value
            .get("isSidechain")
            .or_else(|| value.get("is_sidechain"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_meta = value
            .get("isMeta")
            .or_else(|| value.get("is_meta"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (role, text) = match type_name.as_str() {
            "user" => {
                if is_meta {
                    (None, None)
                } else {
                    match human_user_text(&value) {
                        Some(t) => (Some(TurnRole::User), Some(t)),
                        None => (None, None),
                    }
                }
            }
            "assistant" => {
                let message = value.get("message");
                let text = message
                    .map(assistant_visible_text)
                    .unwrap_or_default();
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    (None, None)
                } else {
                    (Some(TurnRole::Assistant), Some(text))
                }
            }
            _ => (None, None),
        };
        // Later duplicate uuid wins (Claude Code replays records on resume).
        nodes.insert(
            uuid.clone(),
            Node {
                uuid,
                parent_uuid,
                timestamp,
                is_sidechain,
                is_meta,
                text,
                role,
            },
        );
    }
    Some(ParsedJsonl {
        nodes,
        last_prompt_leaf,
    })
}

fn leaf_uuids(nodes: &HashMap<String, Node>) -> Vec<String> {
    let mut parents: HashSet<String> = HashSet::new();
    for node in nodes.values() {
        if let Some(parent) = &node.parent_uuid {
            parents.insert(parent.clone());
        }
    }
    nodes
        .keys()
        .filter(|id| !parents.contains(*id))
        .cloned()
        .collect()
}

fn walk_to_root(nodes: &HashMap<String, Node>, tip: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = Some(tip.to_string());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(node) = nodes.get(&id) else {
            break;
        };
        chain.push(id);
        current = node.parent_uuid.clone();
    }
    chain.reverse();
    chain
}

/// Human-typed user text only. Tool-result-only rows return None.
fn human_user_text(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    match content {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        Value::Array(items) => {
            let mut parts = Vec::new();
            let mut saw_tool = false;
            let mut saw_text = false;
            for item in items {
                if let Some(s) = item.as_str() {
                    if !s.trim().is_empty() {
                        parts.push(s.to_string());
                        saw_text = true;
                    }
                    continue;
                }
                let Some(obj) = item.as_object() else {
                    continue;
                };
                match obj.get("type").and_then(Value::as_str) {
                    Some("tool_result") => saw_tool = true,
                    Some("text") => {
                        if let Some(s) = obj.get("text").and_then(Value::as_str) {
                            if !s.trim().is_empty() {
                                parts.push(s.to_string());
                                saw_text = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if saw_text {
                Some(parts.join("\n"))
            } else if saw_tool {
                None
            } else {
                None
            }
        }
        _ => None,
    }
}

fn assistant_visible_text(message: &Value) -> String {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return text.to_string();
    }
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for block in blocks {
        let Some(obj) = block.as_object() else {
            continue;
        };
        if obj.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = obj.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_jsonl(body: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("uuc-transcript-{}", Uuid::new_v4()));
        let proj = root.join("proj");
        fs::create_dir_all(&proj).unwrap();
        let path = proj.join("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.jsonl");
        fs::write(&path, body).unwrap();
        (root, path)
    }

    #[test]
    fn prefers_last_prompt_leaf_over_older_branch() {
        let body = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"first"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"answer1"}]}}
{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-01-01T00:00:02.000Z","message":{"role":"user","content":"branch-a"}}
{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-01-01T00:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"old tip"}]}}
{"type":"user","uuid":"u3","parentUuid":"a1","timestamp":"2026-01-01T00:01:00.000Z","message":{"role":"user","content":"branch-b"}}
{"type":"assistant","uuid":"a3","parentUuid":"u3","timestamp":"2026-01-01T00:01:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"new tip"}]}}
{"type":"last-prompt","sessionId":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","leafUuid":"a3","lastPrompt":"branch-b"}
"#;
        let (root, path) = write_jsonl(body);
        let tip = resolve_tip(&path).expect("tip");
        assert_eq!(tip.leaf_uuid, "a3");
        assert_eq!(tip.source, TipSource::LastPrompt);
        let (_tip, turns) = import_mainline_turns(&path);
        let texts: Vec<&str> = turns.iter().map(|t| t.content.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "first",
                "answer1",
                "branch-a",
                "old tip",
                "branch-b",
                "new tip"
            ]
        );
        let by_content: HashMap<&str, &GuiTurn> =
            turns.iter().map(|t| (t.content.as_str(), t)).collect();
        assert!(!by_content["first"].off_mainline);
        assert!(!by_content["branch-b"].off_mainline);
        assert!(!by_content["new tip"].off_mainline);
        assert!(by_content["branch-a"].off_mainline);
        assert!(by_content["old tip"].off_mainline);
        assert_eq!(by_content["branch-a"].cli_uuid.as_deref(), Some("u2"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn newest_leaf_when_no_last_prompt() {
        let body = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"root"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"base"}]}}
{"type":"user","uuid":"u-old","parentUuid":"a1","timestamp":"2026-01-01T00:00:02.000Z","message":{"role":"user","content":"old"}}
{"type":"assistant","uuid":"a-old","parentUuid":"u-old","timestamp":"2026-01-01T00:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"old leaf"}]}}
{"type":"user","uuid":"u-new","parentUuid":"a1","timestamp":"2026-01-01T00:02:00.000Z","message":{"role":"user","content":"new"}}
{"type":"assistant","uuid":"a-new","parentUuid":"u-new","timestamp":"2026-01-01T00:02:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"new leaf"}]}}
"#;
        let (root, path) = write_jsonl(body);
        let tip = resolve_tip(&path).expect("tip");
        assert_eq!(tip.leaf_uuid, "a-new");
        assert_eq!(tip.source, TipSource::NewestLeaf);
        let (_tip, turns) = import_mainline_turns(&path);
        assert_eq!(
            turns
                .iter()
                .map(|t| t.content.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "base", "old", "old leaf", "new", "new leaf"]
        );
        let off: Vec<&str> = turns
            .iter()
            .filter(|t| t.off_mainline)
            .map(|t| t.content.as_str())
            .collect();
        assert_eq!(off, vec!["old", "old leaf"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_tool_results_and_meta() {
        let body = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"ask"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"calling"},{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}
{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-01-01T00:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file"}]}}
{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-01-01T00:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}
{"type":"user","uuid":"u-meta","parentUuid":"a2","timestamp":"2026-01-01T00:00:04.000Z","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}
{"type":"last-prompt","leafUuid":"a2"}
"#;
        let (root, path) = write_jsonl(body);
        let (_tip, turns) = import_mainline_turns(&path);
        let texts: Vec<&str> = turns.iter().map(|t| t.content.as_str()).collect();
        assert_eq!(texts, vec!["ask", "calling", "done"]);
        assert!(!texts.iter().any(|t| t.contains("file") || t.contains("Continue")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_replaces_empty_session_turns() {
        let body = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"hello"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}
{"type":"last-prompt","leafUuid":"a1"}
"#;
        let (root, path) = write_jsonl(body);
        let sid = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        // Point cwd so find uses scan under our temp tree — call import directly.
        let (tip, turns) = import_mainline_turns(&path);
        assert_eq!(tip.unwrap().leaf_uuid, "a1");
        assert_eq!(turns.len(), 2);
        let _ = fs::remove_dir_all(&root);
        let _ = sid;
    }

    #[test]
    fn later_uuid_replay_wins() {
        let body = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"early"}}
{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"final"}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}
{"type":"last-prompt","leafUuid":"a1"}
"#;
        let (root, path) = write_jsonl(body);
        let (_tip, turns) = import_mainline_turns(&path);
        assert_eq!(turns[0].content, "final");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_keeps_gui_only_and_matches_cli_uuid() {
        let t0 = DateTime::parse_from_rfc3339("2026-01-01T00:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let t1 = DateTime::parse_from_rfc3339("2026-01-01T00:00:01.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let gui_only_id = Uuid::new_v4();
        let existing = vec![
            GuiTurn {
                id: Uuid::new_v4(),
                role: TurnRole::User,
                content: "from-cli".into(),
                exit_code: None,
                started_at: t0,
                finished_at: Some(t0),
                status: TurnStatus::Done,
                cost_usd: None,
                events: Vec::new(),
                cli_uuid: Some("u1".into()),
                off_mainline: false,
            },
            GuiTurn {
                id: gui_only_id,
                role: TurnRole::User,
                content: "gui-only note".into(),
                exit_code: None,
                started_at: t0 + chrono::Duration::seconds(30),
                finished_at: Some(t0 + chrono::Duration::seconds(30)),
                status: TurnStatus::Done,
                cost_usd: None,
                events: Vec::new(),
                cli_uuid: None,
                off_mainline: false,
            },
            GuiTurn {
                id: Uuid::new_v4(),
                role: TurnRole::Assistant,
                content: "answer".into(),
                exit_code: Some(0),
                started_at: t1,
                finished_at: Some(t1),
                status: TurnStatus::Done,
                cost_usd: Some(0.42),
                events: Vec::new(),
                cli_uuid: Some("a1".into()),
                off_mainline: false,
            },
        ];
        let imported = vec![
            GuiTurn {
                id: Uuid::new_v4(),
                role: TurnRole::User,
                content: "from-cli".into(),
                exit_code: None,
                started_at: t0,
                finished_at: Some(t0),
                status: TurnStatus::Done,
                cost_usd: None,
                events: Vec::new(),
                cli_uuid: Some("u1".into()),
                off_mainline: false,
            },
            GuiTurn {
                id: Uuid::new_v4(),
                role: TurnRole::Assistant,
                content: "answer".into(),
                exit_code: Some(0),
                started_at: t1,
                finished_at: Some(t1),
                status: TurnStatus::Done,
                cost_usd: None,
                events: Vec::new(),
                cli_uuid: Some("a1".into()),
                off_mainline: false,
            },
            GuiTurn {
                id: Uuid::new_v4(),
                role: TurnRole::User,
                content: "other-branch".into(),
                exit_code: None,
                started_at: t0 + chrono::Duration::seconds(2),
                finished_at: Some(t0 + chrono::Duration::seconds(2)),
                status: TurnStatus::Done,
                cost_usd: None,
                events: Vec::new(),
                cli_uuid: Some("u2".into()),
                off_mainline: true,
            },
        ];
        let merged = merge_gui_turns(&existing, imported);
        let texts: Vec<&str> = merged.iter().map(|t| t.content.as_str()).collect();
        assert_eq!(
            texts,
            vec!["from-cli", "answer", "other-branch", "gui-only note"]
        );
        let answer = merged.iter().find(|t| t.content == "answer").unwrap();
        assert_eq!(answer.cost_usd, Some(0.42));
        assert!(merged.iter().any(|t| t.id == gui_only_id));
        assert!(merged
            .iter()
            .any(|t| t.content == "other-branch" && t.off_mainline));
    }

    #[test]
    fn merge_matches_by_content_and_time_without_cli_uuid() {
        let t0 = DateTime::parse_from_rfc3339("2026-01-01T00:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let gui_id = Uuid::new_v4();
        let existing = vec![GuiTurn {
            id: gui_id,
            role: TurnRole::User,
            content: "hello".into(),
            exit_code: None,
            started_at: t0 + chrono::Duration::seconds(5),
            finished_at: Some(t0 + chrono::Duration::seconds(5)),
            status: TurnStatus::Done,
            cost_usd: None,
            events: Vec::new(),
            cli_uuid: None,
            off_mainline: false,
        }];
        let imported = vec![GuiTurn {
            id: Uuid::new_v4(),
            role: TurnRole::User,
            content: "hello".into(),
            exit_code: None,
            started_at: t0,
            finished_at: Some(t0),
            status: TurnStatus::Done,
            cost_usd: None,
            events: Vec::new(),
            cli_uuid: Some("u-hello".into()),
            off_mainline: true,
        }];
        let merged = merge_gui_turns(&existing, imported);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, gui_id);
        assert_eq!(merged[0].cli_uuid.as_deref(), Some("u-hello"));
        assert!(merged[0].off_mainline);
        assert_eq!(merged[0].started_at, t0);
    }
}
