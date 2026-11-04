use crate::models::{ClaudeDetectResult, ClaudeJsonOutput, CliEnvelope};
use serde_json::Value;
use crate::secrets;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Message(String),
    #[error("claude binary not found — set path in Settings or install Claude Code CLI")]
    NotFound,
    #[error("timed out after {0}s")]
    TimedOut(u64),
    #[error("cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct ActiveRun {
    pub child: Mutex<Option<Child>>,
    pub cancel_flag: AtomicBool,
}

impl ActiveRun {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            cancel_flag: AtomicBool::new(false),
        }
    }
}

pub fn resolve_binary(override_path: Option<&str>) -> Result<PathBuf, CliError> {
    if let Some(p) = override_path {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::Message(format!(
            "configured claude path not found: {p}"
        )));
    }
    which::which("claude").map_err(|_| CliError::NotFound)
}

pub async fn detect_claude(override_path: Option<&str>) -> ClaudeDetectResult {
    match resolve_binary(override_path) {
        Ok(path) => {
            let version = Command::new(&path)
                .arg("--version")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .ok()
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        if e.is_empty() {
                            None
                        } else {
                            Some(e)
                        }
                    } else {
                        Some(s)
                    }
                });
            ClaudeDetectResult {
                found: true,
                path: Some(path.display().to_string()),
                version,
                error: None,
            }
        }
        Err(e) => ClaudeDetectResult {
            found: false,
            path: None,
            version: None,
            error: Some(e.to_string()),
        },
    }
}

/// Map MVP preset → Claude CLI `--allowedTools` args.
pub fn allowed_tools_args(preset: &str) -> Vec<String> {
    match preset {
        "read" | "readonly" => vec!["--allowedTools".into(), "Read".into()],
        "read_edit" => vec!["--allowedTools".into(), "Read,Edit".into()],
        "full" => vec![],
        other => vec!["--allowedTools".into(), other.to_string()],
    }
}

pub struct RunSpec<'a> {
    pub binary: &'a Path,
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub resume: Option<&'a str>,
    pub model: Option<&'a str>,
    pub allowed_tools_preset: &'a str,
    pub timeout_secs: u64,
    pub api_key: Option<&'a str>,
}

pub struct RunOutput {
    pub exit_code: i32,
    pub parsed: Option<ClaudeJsonOutput>,
    pub result_text: String,
    pub session_id: Option<String>,
    pub total_cost_usd: Option<f64>,
    pub parse_ok: bool,
}

pub async fn run_claude_managed(
    spec: RunSpec<'_>,
    active: Arc<ActiveRun>,
) -> Result<RunOutput, CliError> {
    active.cancel_flag.store(false, Ordering::SeqCst);

    let mut cmd = Command::new(spec.binary);
    cmd.arg("-p")
        .arg(spec.prompt)
        .arg("--output-format")
        .arg("json")
        .current_dir(spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    for arg in allowed_tools_args(spec.allowed_tools_preset) {
        cmd.arg(arg);
    }
    if let Some(id) = spec.resume {
        cmd.arg("--resume").arg(id);
    }
    if let Some(model) = spec.model {
        if !model.is_empty() {
            cmd.arg("--model").arg(model);
        }
    }

    if let Some(key) = spec.api_key {
        cmd.env("ANTHROPIC_API_KEY", key);
    } else if let Ok(Some(key)) = secrets::load_api_key() {
        cmd.env("ANTHROPIC_API_KEY", key);
    }

    let mut child = cmd.spawn()?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Message("failed to capture stdout".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliError::Message("failed to capture stderr".into()))?;

    *active.child.lock().await = Some(child);

    let out_task = async {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map_err(CliError::from)?;
        Ok::<_, CliError>(String::from_utf8_lossy(&buf).to_string())
    };
    let err_task = async {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map_err(CliError::from)?;
        Ok::<_, CliError>(String::from_utf8_lossy(&buf).to_string())
    };

    let wait_task = async {
        loop {
            if active.cancel_flag.load(Ordering::SeqCst) {
                if let Some(mut c) = active.child.lock().await.take() {
                    let _ = c.kill().await;
                }
                return Err(CliError::Cancelled);
            }
            {
                let mut guard = active.child.lock().await;
                if let Some(c) = guard.as_mut() {
                    match c.try_wait().map_err(CliError::from)? {
                        Some(status) => {
                            let code = status.code().unwrap_or(-1);
                            *guard = None;
                            return Ok(code);
                        }
                        None => {}
                    }
                } else {
                    return Err(CliError::Cancelled);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };

    let joined = timeout(
        Duration::from_secs(spec.timeout_secs.max(30)),
        async { tokio::try_join!(out_task, err_task, wait_task) },
    )
    .await;

    match joined {
        Err(_) => {
            if let Some(mut c) = active.child.lock().await.take() {
                let _ = c.kill().await;
            }
            Err(CliError::TimedOut(spec.timeout_secs))
        }
        Ok(Err(e)) => Err(e),
        Ok(Ok((stdout, stderr, exit_code))) => {
            *active.child.lock().await = None;
            Ok(parse_output(exit_code, stdout, stderr))
        }
    }
}

fn parse_output(exit_code: i32, stdout: String, stderr: String) -> RunOutput {
    // Parse the root object. The last `{` is often inside `usage`, and taking
    // that slice drops `result` / `session_id` and leaks the envelope into chat.
    if let Some(env) = interpret_cli_text(&stdout) {
        let parsed = ClaudeJsonOutput {
            session_id: env.session_id.clone(),
            result: Some(Value::String(env.result_text.clone())),
            is_error: Some(env.is_error),
            error: None,
            subtype: None,
            total_cost_usd: env.total_cost_usd,
        };
        return RunOutput {
            exit_code,
            session_id: env.session_id,
            total_cost_usd: env.total_cost_usd,
            result_text: env.result_text,
            parsed: Some(parsed),
            parse_ok: true,
        };
    }

    let result_text = fallback_message(exit_code, &stdout, &stderr);
    RunOutput {
        exit_code,
        parsed: None,
        result_text,
        session_id: None,
        total_cost_usd: None,
        parse_ok: false,
    }
}

/// Find a CLI result envelope in process stdout.
/// Accepts a single JSON document or log lines followed by that document.
pub(crate) fn interpret_cli_text(text: &str) -> Option<CliEnvelope> {
    let trimmed = text.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(env) = envelope_from_value(&value) {
            return Some(env);
        }
        if let Some(env) = value
            .as_array()
            .and_then(|items| items.iter().rev().find_map(envelope_from_value))
        {
            return Some(env);
        }
    }
    scan_envelopes(trimmed)
}

/// Unwrap a stored chat message only when the entire message is the CLI envelope.
pub(crate) fn envelope_if_whole_document(content: &str) -> Option<CliEnvelope> {
    let trimmed = content.trim().trim_start_matches('\u{feff}');
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(env) = envelope_from_value(&value) {
        return Some(env);
    }
    value
        .as_array()
        .and_then(|items| items.iter().rev().find_map(envelope_from_value))
}

fn envelope_from_value(value: &Value) -> Option<CliEnvelope> {
    let obj = value.as_object()?;
    let type_result = obj.get("type").and_then(Value::as_str) == Some("result");
    let marker = obj.contains_key("session_id")
        || obj.contains_key("total_cost_usd")
        || obj.contains_key("usage")
        || type_result;
    if !marker {
        return None;
    }
    let has_payload = obj.contains_key("result")
        || obj.contains_key("error")
        || obj.contains_key("is_error")
        || type_result;
    if !has_payload {
        return None;
    }

    let result = obj.get("result").and_then(value_as_message);
    let error = obj.get("error").and_then(value_as_message);
    let is_error = obj
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result_text = result
        .filter(|s| !s.is_empty())
        .or(error.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| {
            if is_error {
                "Claude returned an error.".to_string()
            } else {
                "(empty response)".to_string()
            }
        });

    let session_id = obj
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Some(CliEnvelope {
        result_text,
        session_id,
        total_cost_usd: obj.get("total_cost_usd").and_then(value_as_f64),
        is_error,
    })
}

fn value_as_message(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn scan_envelopes(input: &str) -> Option<CliEnvelope> {
    let mut rest = input;
    let mut found = None;
    while !rest.is_empty() {
        let Some(idx) = rest.find(['{', '[']) else {
            break;
        };
        rest = &rest[idx..];
        let before = rest.len();
        let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        match stream.next() {
            Some(Ok(value)) => {
                if let Some(env) = envelope_from_value(&value) {
                    found = Some(env);
                } else if let Some(items) = value.as_array() {
                    if let Some(env) = items.iter().rev().find_map(envelope_from_value) {
                        found = Some(env);
                    }
                }
                rest = advance_str(rest, stream.byte_offset());
            }
            _ => {
                rest = advance_str(rest, 1);
            }
        }
        if rest.len() >= before {
            rest = advance_str(rest, 1);
        }
    }
    found
}

fn advance_str(s: &str, bytes: usize) -> &str {
    if s.is_empty() {
        return s;
    }
    let mut n = bytes.min(s.len());
    if n == 0 {
        n = s.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    while n < s.len() && !s.is_char_boundary(n) {
        n += 1;
    }
    if n > s.len() {
        n = s.len();
    }
    &s[n..]
}

fn first_plain_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    if line.contains('{')
        || line.contains('}')
        || line.contains("sk-ant")
        || line.contains("session_id")
        || line.contains("api_key")
    {
        return None;
    }
    let mut s: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        s.push('…');
    }
    Some(s)
}

fn fallback_message(exit_code: i32, stdout: &str, stderr: &str) -> String {
    if stdout.trim().is_empty() {
        if let Some(line) = first_plain_line(stderr) {
            return line;
        }
        return format!("Claude CLI failed (exit {exit_code}).");
    }
    if let Some(line) = first_plain_line(stderr) {
        format!("Couldn't read the Claude response (invalid JSON, exit {exit_code}). {line}")
    } else {
        format!("Couldn't read the Claude response (invalid JSON, exit {exit_code}).")
    }
}

pub async fn cancel_run(active: &ActiveRun) -> Result<(), CliError> {
    active.cancel_flag.store(true, Ordering::SeqCst);
    if let Some(mut c) = active.child.lock().await.take() {
        let _ = c.kill().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_envelope() -> String {
        r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":12,"result":"Hello\nworld","session_id":"abc-123","total_cost_usd":0.0123,"usage":{"input_tokens":10,"output_tokens":5,"cache_creation":{"ephemeral_5m_input_tokens":0}}}"#.to_string()
    }

    #[test]
    fn extracts_result_not_envelope() {
        let out = parse_output(0, sample_envelope(), String::new());
        assert!(out.parse_ok);
        assert_eq!(out.result_text, "Hello\nworld");
        assert_eq!(out.session_id.as_deref(), Some("abc-123"));
        assert!((out.total_cost_usd.unwrap() - 0.0123).abs() < 1e-9);
        assert!(!out.result_text.contains("session_id"));
        assert!(!out.result_text.contains("total_cost_usd"));
    }

    #[test]
    fn nested_braces_inside_result_do_not_hide_the_message() {
        let stdout = r#"{"type":"result","result":"fn main() { println!(\"hi\"); }","session_id":"s1","total_cost_usd":0.5,"usage":{"cache_creation":{"ephemeral_5m_input_tokens":0}}}"#;
        let out = parse_output(0, stdout.to_string(), String::new());
        assert_eq!(out.result_text, "fn main() { println!(\"hi\"); }");
        assert_eq!(out.session_id.as_deref(), Some("s1"));
        assert_eq!(out.total_cost_usd, Some(0.5));
    }

    #[test]
    fn leading_log_then_json() {
        let stdout = "Warning: something happened\n{\"type\":\"result\",\"result\":\"pong\",\"session_id\":\"sid\",\"total_cost_usd\":0.01}\n";
        let out = parse_output(0, stdout.to_string(), String::new());
        assert!(out.parse_ok);
        assert_eq!(out.result_text, "pong");
        assert_eq!(out.session_id.as_deref(), Some("sid"));
    }

    #[test]
    fn pretty_printed_json() {
        let stdout = r#"{
  "type": "result",
  "result": "line one\nline two",
  "session_id": "pretty",
  "total_cost_usd": 1.25,
  "usage": { "output_tokens": 2 }
}"#;
        let out = parse_output(0, stdout.to_string(), String::new());
        assert_eq!(out.result_text, "line one\nline two");
        assert_eq!(out.session_id.as_deref(), Some("pretty"));
        assert_eq!(out.total_cost_usd, Some(1.25));
    }

    #[test]
    fn parse_failure_is_short_and_hides_the_blob() {
        let stdout = "{\"session_id\":\"secret-session\",\"usage\":{\"output_tokens\":1}";
        let out = parse_output(1, stdout.to_string(), "{\"error\":\"nope\"}".into());
        assert!(!out.parse_ok);
        assert!(out.session_id.is_none());
        assert!(out.result_text.len() < 180);
        assert!(!out.result_text.contains("secret-session"));
        assert!(!out.result_text.contains("usage"));
        assert!(out.result_text.contains("invalid JSON"));
    }

    #[test]
    fn plain_stderr_is_used_when_stdout_is_empty() {
        let out = parse_output(1, String::new(), "Error: claude binary failed\n".into());
        assert!(!out.parse_ok);
        assert_eq!(out.result_text, "Error: claude binary failed");
    }

    #[test]
    fn stored_prose_is_not_unwrapped() {
        let prose = "Here is an example:\n{\"type\":\"result\",\"result\":\"x\",\"session_id\":\"s\"}";
        assert!(envelope_if_whole_document(prose).is_none());
        let env = interpret_cli_text(prose).expect("stdout scan");
        assert_eq!(env.result_text, "x");
    }

    #[test]
    fn whole_document_envelope_unwraps_for_old_transcripts() {
        let env = envelope_if_whole_document(&sample_envelope()).expect("envelope");
        assert_eq!(env.result_text, "Hello\nworld");
        assert_eq!(env.session_id.as_deref(), Some("abc-123"));
    }
}
