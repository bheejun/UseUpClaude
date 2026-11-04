use crate::models::{DiscoveredProject, DiscoveredSession};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use uuid::Uuid;

const MAX_SESSIONS: usize = 200;
const MAX_PER_PROJECT: usize = 40;
const MAX_LINES: usize = 40;
const MAX_LINE_BYTES: usize = 64 * 1024;

struct Peeked {
    cwd: Option<String>,
    session: DiscoveredSession,
}

/// List Claude Code sessions under `~/.claude/projects`.
/// Returns ids, folder hints, and a short redacted title — not transcripts.
pub fn discover_claude_sessions() -> Vec<DiscoveredProject> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut found = discover_in(&home.join(".claude").join("projects"));
    let sessions_root = home.join(".claude").join("sessions");
    if sessions_root.is_dir() {
        found.extend(discover_in(&sessions_root));
    }
    sort_projects(&mut found);
    found.truncate(MAX_SESSIONS);
    found
}

pub(crate) fn discover_in(root: &Path) -> Vec<DiscoveredProject> {
    let mut groups: Vec<DiscoveredProject> = Vec::new();
    let mut total = 0usize;
    let Ok(projects) = fs::read_dir(root) else {
        return groups;
    };

    for project in projects.flatten() {
        if total >= MAX_SESSIONS {
            break;
        }
        let dir = project.path();
        if !dir.is_dir() {
            continue;
        }
        let folder_name = project.file_name().to_string_lossy().to_string();
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_session_file(p))
            .collect();
        files.sort();
        let mut added = 0usize;
        for path in files {
            if total >= MAX_SESSIONS || added >= MAX_PER_PROJECT {
                break;
            }
            if let Some(peeked) = peek_session(&path) {
                insert_session(&mut groups, peeked.cwd, &folder_name, peeked.session);
                total += 1;
                added += 1;
            }
        }
    }

    sort_projects(&mut groups);
    groups
}

fn is_session_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return false;
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .is_some()
}

fn insert_session(
    groups: &mut Vec<DiscoveredProject>,
    cwd: Option<String>,
    folder_name: &str,
    session: DiscoveredSession,
) {
    if let Some(group) = groups.iter_mut().find(|g| match (&g.cwd, &cwd) {
        (Some(a), Some(b)) => a == b,
        (None, None) => g.label == folder_name,
        _ => false,
    }) {
        group.sessions.push(session);
        return;
    }
    groups.push(DiscoveredProject {
        label: match &cwd {
            Some(path) => basename(path),
            None => folder_name.to_string(),
        },
        cwd,
        sessions: vec![session],
    });
}

fn sort_projects(groups: &mut [DiscoveredProject]) {
    for group in groups.iter_mut() {
        group
            .sessions
            .sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    }
    groups.sort_by(|a, b| {
        let tb = b.sessions.first().map(|s| s.modified_ms).unwrap_or(0);
        let ta = a.sessions.first().map(|s| s.modified_ms).unwrap_or(0);
        tb.cmp(&ta)
    });
}

fn basename(path: &str) -> String {
    path.split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .next_back()
        .unwrap_or(path)
        .to_string()
}

fn peek_session(path: &Path) -> Option<Peeked> {
    let session_id = path.file_stem()?.to_str()?.to_string();
    if Uuid::parse_str(&session_id).is_err() {
        return None;
    }
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut cwd = None;
    let mut title = None;
    for _ in 0..MAX_LINES {
        let line = match read_line_capped(&mut reader, MAX_LINE_BYTES) {
            Ok(Some(line)) => line,
            _ => break,
        };
        if cwd.is_none() {
            cwd = cwd_from_line(&line);
        }
        if title.is_none() {
            title = title_from_line(&line);
        }
        if cwd.is_some() && title.is_some() {
            break;
        }
    }
    let title = title.unwrap_or_else(|| {
        let mut short: String = session_id.chars().take(8).collect();
        short.push('…');
        short
    });
    Some(Peeked {
        cwd,
        session: DiscoveredSession {
            session_id,
            title,
            modified_ms: modified_ms(path),
        },
    })
}

fn cwd_from_line(line: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(line) {
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            let cwd = cwd.trim();
            if !cwd.is_empty() {
                return Some(cwd.to_string());
            }
        }
    }
    sniff_cwd(line)
}

fn title_from_line(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let content = value.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(s) = item.as_str() {
                    parts.push(s.to_string());
                } else if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(s) = item.get("text").and_then(Value::as_str) {
                        parts.push(s.to_string());
                    }
                }
            }
            parts.join(" ")
        }
        _ => return None,
    };
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let redacted = redact_preview(&collapsed);
    let trimmed = redacted.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, 80))
    }
}

fn sniff_cwd(line: &str) -> Option<String> {
    let key = "\"cwd\":\"";
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('"') => out.push('"'),
                Some(other) => out.push(other),
                None => break,
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
        if out.len() > 512 {
            break;
        }
    }
    let out = out.trim();
    if out.is_empty() {
        None
    } else {
        Some(out.to_string())
    }
}

fn read_line_capped<R: Read>(reader: &mut BufReader<R>, cap: usize) -> std::io::Result<Option<String>> {
    let mut collected = Vec::new();
    let mut byte = [0u8; 1];
    let mut seen = false;
    loop {
        let n = reader.read(&mut byte)?;
        if n == 0 {
            if !seen {
                return Ok(None);
            }
            break;
        }
        seen = true;
        if byte[0] == b'\n' {
            break;
        }
        if collected.len() < cap {
            collected.push(byte[0]);
        }
    }
    if collected.last() == Some(&b'\r') {
        collected.pop();
    }
    Ok(Some(String::from_utf8_lossy(&collected).into_owned()))
}

fn modified_ms(path: &Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut t: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        t.push('…');
    }
    t
}

/// Strip key-shaped secrets from a short preview. Never log the original.
pub(crate) fn redact_preview(input: &str) -> String {
    let mut out = input.to_string();
    for marker in ["sk-ant-", "ANTHROPIC_API_KEY="] {
        while let Some(idx) = out.find(marker) {
            let tail = &out[idx..];
            let rel = tail
                .char_indices()
                .find(|(_, c)| {
                    c.is_whitespace() || matches!(*c, '"' | '\'' | ',' | '}' | ')' | ']' | ';')
                })
                .map(|(i, _)| i)
                .unwrap_or(tail.len())
                .max(marker.len());
            let end = idx + rel;
            if !out.is_char_boundary(end) {
                break;
            }
            out.replace_range(idx..end, "[redacted]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_api_key_material() {
        let got = redact_preview("Fix parser sk-ant-api03-SUPERSECRET please");
        assert!(got.contains("Fix parser"));
        assert!(got.contains("[redacted]"));
        assert!(!got.contains("SUPERSECRET"));
        assert!(!got.contains("sk-ant"));
    }

    #[test]
    fn reads_cwd_and_redacts_title() {
        let root = std::env::temp_dir().join(format!("uuc-discover-{}", Uuid::new_v4()));
        let proj = root.join("C--work-demo");
        fs::create_dir_all(&proj).unwrap();
        let id = "11111111-1111-4111-8111-111111111111";
        let body = r#"{"type":"attachment","cwd":"C:\\work\\demo","sessionId":"11111111-1111-4111-8111-111111111111"}
{"type":"user","cwd":"C:\\work\\demo","sessionId":"11111111-1111-4111-8111-111111111111","message":{"role":"user","content":"Fix parser sk-ant-api03-SUPERSECRET please"}}
"#;
        fs::write(proj.join(format!("{id}.jsonl")), body).unwrap();
        fs::write(proj.join("notes.txt"), "sk-ant-api03-SHOULD-NOT-APPEAR").unwrap();
        let found = discover_in(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].cwd.as_deref(), Some(r"C:\work\demo"));
        assert_eq!(found[0].sessions.len(), 1);
        let title = &found[0].sessions[0].title;
        assert!(title.contains("Fix parser"), "{title}");
        assert!(title.contains("[redacted]"), "{title}");
        assert!(!title.contains("SUPERSECRET"), "{title}");
        assert!(!title.contains("sk-ant"), "{title}");
        assert_eq!(found[0].sessions[0].session_id, id);
    }
}
