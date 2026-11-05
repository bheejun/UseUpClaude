//! Recover GUI turns stuck on the literal `(empty response)` from Claude Code JSONL.

use crate::models::{GuiSession, TurnRole};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const EMPTY_PLACEHOLDER: &str = "(empty response)";
const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 20_000;

/// Fill `(empty response)` turns from the matching Claude Code transcript when possible.
/// Leaves the placeholder when the JSONL has no recoverable answer text.
pub fn recover_empty_response_turns(session: &mut GuiSession) -> bool {
    let Some(cli_session_id) = session.cli_session_id.as_deref().map(str::trim) else {
        return false;
    };
    if cli_session_id.is_empty() {
        return false;
    }
    if !session.turns.iter().any(|turn| {
        matches!(turn.role, TurnRole::Assistant | TurnRole::Error)
            && turn.content.trim() == EMPTY_PLACEHOLDER
    }) {
        return false;
    }
    let Some(path) = find_session_jsonl(cli_session_id, &session.cwd) else {
        return false;
    };
    recover_from_jsonl(session, &path)
}

fn recover_from_jsonl(session: &mut GuiSession, path: &Path) -> bool {
    let empty_indices: Vec<usize> = session
        .turns
        .iter()
        .enumerate()
        .filter(|(_, turn)| {
            matches!(turn.role, TurnRole::Assistant | TurnRole::Error)
                && turn.content.trim() == EMPTY_PLACEHOLDER
        })
        .map(|(index, _)| index)
        .collect();
    if empty_indices.is_empty() {
        return false;
    }
    let answers = extract_answers_from_jsonl(path);
    if answers.is_empty() {
        return false;
    }

    let used: HashSet<String> = session
        .turns
        .iter()
        .filter(|turn| turn.content.trim() != EMPTY_PLACEHOLDER)
        .map(|turn| turn.content.clone())
        .collect();
    let mut available: Vec<String> = answers
        .into_iter()
        .filter(|text| !text.trim().is_empty() && !used.contains(text))
        .collect();
    if available.is_empty() {
        return false;
    }

    let mut changed = false;
    for &index in empty_indices.iter().rev() {
        let Some(text) = available.pop() else {
            break;
        };
        session.turns[index].content = text;
        changed = true;
    }
    changed
}

fn find_session_jsonl(cli_session_id: &str, cwd: &str) -> Option<PathBuf> {
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
    // Folder names sometimes omit a drive letter nuance; try a contains match.
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

fn extract_answers_from_jsonl(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let mut answers = Vec::new();
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
        if let Some(text) = answer_from_jsonl_line(line.trim()) {
            answers.push(text);
        }
    }
    answers
}

fn answer_from_jsonl_line(line: &str) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    match value.get("type").and_then(Value::as_str) {
        Some("result") => {
            let text = value
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if text.is_empty() || text == EMPTY_PLACEHOLDER {
                None
            } else {
                Some(text.to_string())
            }
        }
        Some("assistant") => {
            let message = value.get("message")?;
            let text = assistant_visible_text(message);
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed == EMPTY_PLACEHOLDER {
                None
            } else {
                Some(text)
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
    use crate::models::{GuiTurn, TurnStatus};
    use chrono::Utc;
    use uuid::Uuid;

    fn turn(content: &str) -> GuiTurn {
        GuiTurn {
            id: Uuid::new_v4(),
            role: TurnRole::Assistant,
            content: content.into(),
            exit_code: Some(0),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            status: TurnStatus::Done,
            cost_usd: Some(0.1),
            events: vec![],
        }
    }

    #[test]
    fn recovers_last_answer_into_empty_placeholder() {
        let root = std::env::temp_dir().join(format!("uuc-repair-{}", Uuid::new_v4()));
        let proj = root.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let sid = "22222222-2222-4222-8222-222222222222";
        let body = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"early"}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"Final report"}]}}
{"type":"result","result":"From result line"}
"#;
        let path = proj.join(format!("{sid}.jsonl"));
        std::fs::write(&path, body).unwrap();

        let mut session = GuiSession {
            id: Uuid::new_v4(),
            cli_session_id: Some(sid.into()),
            cwd: "/tmp/demo".into(),
            title: "t".into(),
            model: None,
            effort: None,
            fast_mode: false,
            allowed_tools_preset: "read".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            turns: vec![turn("user ask"), turn(EMPTY_PLACEHOLDER)],
            turn_costs_are_incremental: true,
        };
        session.turns[0].role = TurnRole::User;

        assert!(recover_from_jsonl(&mut session, &path));
        assert_eq!(session.turns[1].content, "From result line");
        assert_eq!(
            scan_for_jsonl(&root, &format!("{sid}.jsonl")),
            Some(path.clone())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaves_placeholder_when_jsonl_has_no_text() {
        let root = std::env::temp_dir().join(format!("uuc-repair-empty-{}", Uuid::new_v4()));
        let proj = root.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let sid = "33333333-3333-4333-8333-333333333333";
        std::fs::write(
            proj.join(format!("{sid}.jsonl")),
            r#"{"type":"user","message":{"content":"hi"}}
{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"only"}]}}
"#,
        )
        .unwrap();
        let answers = extract_answers_from_jsonl(&proj.join(format!("{sid}.jsonl")));
        assert!(answers.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
