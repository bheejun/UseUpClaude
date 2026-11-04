use crate::models::{ClaudeDetectResult, ClaudeJsonOutput, CliEnvelope, TurnEvent, TurnEventKind};
use crate::secrets;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
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
    let found = if let Some(p) = override_path {
        let path = PathBuf::from(p);
        if !path.is_file() {
            return Err(CliError::Message(format!(
                "configured claude path not found: {p}"
            )));
        }
        path
    } else {
        which::which("claude").map_err(|_| CliError::NotFound)?
    };
    // `claude` on npm is a `.cmd` shim. Rust refuses to spawn a batch file once
    // the command line passes cmd's ~8191 character limit, and quotes in the
    // prompt are parsed by cmd.exe. The shim's claude.exe takes the same args.
    Ok(prefer_direct_executable(&found))
}

fn is_windows_exe(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path.is_file()
}

/// Use `claude.exe` when `path` is the npm `.cmd` / extensionless shim.
pub(crate) fn prefer_direct_executable(path: &Path) -> PathBuf {
    if is_windows_exe(path) {
        return path.to_path_buf();
    }
    exe_next_to_npm_shim(path)
        .or_else(|| exe_referenced_by_shim(path))
        .unwrap_or_else(|| path.to_path_buf())
}

fn exe_next_to_npm_shim(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let nested = dir
        .join("node_modules")
        .join("@anthropic-ai")
        .join("claude-code")
        .join("bin")
        .join("claude.exe");
    if nested.is_file() {
        return Some(nested);
    }
    let sibling = dir.join("claude.exe");
    sibling.is_file().then_some(sibling)
}

fn exe_referenced_by_shim(path: &Path) -> Option<PathBuf> {
    let text = read_shim_text(path)?;
    let dir = path.parent()?;
    let mut prefix = dir.to_string_lossy().into_owned();
    if !prefix.ends_with('\\') && !prefix.ends_with('/') {
        prefix.push('\\');
    }
    let lower = text.to_ascii_lowercase();
    let marker = "claude.exe";
    let mut search = 0;
    while let Some(rel) = lower[search..].find(marker) {
        let end = search + rel + marker.len();
        let start = text[..end]
            .rfind('"')
            .map(|index| index + 1)
            .or_else(|| {
                text[..end]
                    .rfind(|ch: char| ch.is_whitespace())
                    .map(|index| index + 1)
            })
            .unwrap_or(0);
        let raw = text[start..end].trim().trim_matches('"').trim_matches('\'');
        let expanded = raw.replace("%~dp0", &prefix).replace("%dp0%", &prefix);
        let candidate = PathBuf::from(expanded);
        if candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("claude.exe"))
            && candidate.is_file()
        {
            return Some(candidate);
        }
        search = end;
    }
    None
}

fn read_shim_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > 64 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Inline `--settings` value. This is one argv element for this process only.
/// It is not a file path, and nothing here writes the user's Claude settings file.
pub(crate) const FAST_MODE_SETTINGS: &str = r#"{"fastMode": true}"#;

/// Keep a known effort level, or drop the flag. Case and surrounding space do not matter.
pub(crate) fn normalize_effort(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    EFFORT_LEVELS
        .iter()
        .copied()
        .find(|level| *level == lower)
        .map(str::to_string)
}

/// Flags for one non-interactive turn. The prompt is not included; it is written
/// to stdin so a long or quoted prompt never becomes a batch-file argument.
fn print_mode_args(
    resume: Option<&str>,
    model: Option<&str>,
    tools: &str,
    effort: Option<&str>,
    fast_mode: bool,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];
    args.extend(permission_args(tools));
    if let Some(id) = resume {
        args.push("--resume".to_string());
        args.push(id.to_string());
    }
    if let Some(model) = model {
        if !model.is_empty() {
            args.push("--model".to_string());
            args.push(model.to_string());
        }
    }
    if let Some(level) = normalize_effort(effort) {
        args.push("--effort".to_string());
        args.push(level);
    }
    if fast_mode {
        args.push("--settings".to_string());
        args.push(FAST_MODE_SETTINGS.to_string());
    }
    args
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

/// Map a session tool preset onto `claude -p` permission flags.
///
/// Project skills load from the session cwd. `Skill` is pre-approved so one can
/// start, and its later tool calls still have to pass this same mode. Read-only
/// never includes Edit, Write, Bash, or a permissions bypass. Nothing here passes
/// `--safe-mode` or `--disable-slash-commands`.
pub fn permission_args(preset: &str) -> Vec<String> {
    match preset {
        "read" | "readonly" => flags(&[
            "--permission-mode",
            "dontAsk",
            "--permission-prompts",
            "none",
            "--allowedTools",
            "Skill,Read,Grep,Glob",
        ]),
        "read_edit" => flags(&[
            "--permission-mode",
            "acceptEdits",
            "--permission-prompts",
            "none",
            "--allowedTools",
            "Skill,Read,Grep,Glob,Edit,Write",
        ]),
        "full" => flags(&[
            "--allow-dangerously-skip-permissions",
            "--dangerously-skip-permissions",
            "--permission-mode",
            "bypassPermissions",
            "--permission-prompts",
            "none",
        ]),
        other => {
            let mut args = flags(&[
                "--permission-mode",
                "dontAsk",
                "--permission-prompts",
                "none",
                "--allowedTools",
            ]);
            args.push(other.to_string());
            args
        }
    }
}

fn flags(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

pub struct RunSpec<'a> {
    pub binary: &'a Path,
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub resume: Option<&'a str>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub fast_mode: bool,
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
    pub events: Vec<TurnEvent>,
}

#[derive(Default)]
struct StreamParse {
    events: Vec<TurnEvent>,
    envelope: Option<CliEnvelope>,
}

pub async fn run_claude_managed(
    spec: RunSpec<'_>,
    active: Arc<ActiveRun>,
    on_event: &(dyn Fn(&TurnEvent) + Send + Sync),
) -> Result<RunOutput, CliError> {
    active.cancel_flag.store(false, Ordering::SeqCst);

    let binary = prefer_direct_executable(spec.binary);
    let mut cmd = Command::new(&binary);
    cmd.args(print_mode_args(
        spec.resume,
        spec.model,
        spec.allowed_tools_preset,
        spec.effort,
        spec.fast_mode,
    ))
    .current_dir(spec.cwd)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    // Chat key only. An Admin key is refused before it can reach the child env.
    let chat_key = match spec.api_key {
        Some(key) => cli_key_from_raw(Some(key))?,
        None => match secrets::load_api_key().map_err(|e| CliError::Message(e.to_string()))? {
            Some(key) => cli_key_from_raw(Some(&key))?,
            None => None,
        },
    };
    if let Some(key) = chat_key {
        cmd.env("ANTHROPIC_API_KEY", key);
    }

    let mut child = cmd.spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Message("failed to capture stdout".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliError::Message("failed to capture stderr".into()))?;
    let stdin = child.stdin.take();

    *active.child.lock().await = Some(child);

    let out_task = async {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut line_bytes = Vec::new();
        let mut raw_bytes = Vec::new();
        let mut streamed = StreamParse::default();
        loop {
            line_bytes.clear();
            let n = reader
                .read_until(b'\n', &mut line_bytes)
                .await
                .map_err(CliError::from)?;
            if n == 0 {
                break;
            }
            raw_bytes.extend_from_slice(&line_bytes);
            let line = String::from_utf8_lossy(&line_bytes);
            for event in ingest_stdout_line(&mut streamed, &line) {
                on_event(&event);
            }
        }
        let raw = String::from_utf8_lossy(&raw_bytes).into_owned();
        Ok::<_, CliError>((raw, streamed))
    };
    let err_task = async {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map_err(CliError::from)?;
        Ok::<_, CliError>(String::from_utf8_lossy(&buf).to_string())
    };

    let in_task = async {
        if let Some(mut stdin) = stdin {
            match stdin.write_all(spec.prompt.as_bytes()).await {
                Ok(()) => {
                    let _ = stdin.shutdown().await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(e) => return Err(CliError::Io(e)),
            }
        }
        Ok(())
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
        async { tokio::try_join!(out_task, err_task, wait_task, in_task) },
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
        Ok(Ok(((stdout, streamed), stderr, exit_code, ()))) => {
            *active.child.lock().await = None;
            Ok(finish_run_output(exit_code, &stdout, &stderr, streamed))
        }
    }
}

#[cfg(test)]
fn parse_output(exit_code: i32, stdout: String, stderr: String) -> RunOutput {
    finish_run_output(exit_code, &stdout, &stderr, parse_stream(&stdout))
}

fn finish_run_output(
    exit_code: i32,
    stdout: &str,
    stderr: &str,
    streamed: StreamParse,
) -> RunOutput {
    let StreamParse { events, envelope } = streamed;
    // A result line wins. Pretty-printed `--output-format json` still has no
    // one-line objects, so the whole-document scan remains the fallback.
    if let Some(env) = envelope.or_else(|| interpret_cli_text(stdout)) {
        return output_from_envelope(exit_code, env, events);
    }
    RunOutput {
        exit_code,
        parsed: None,
        result_text: fallback_message(exit_code, stdout, stderr),
        session_id: None,
        total_cost_usd: None,
        parse_ok: false,
        events,
    }
}

fn output_from_envelope(exit_code: i32, env: CliEnvelope, events: Vec<TurnEvent>) -> RunOutput {
    let parsed = ClaudeJsonOutput {
        session_id: env.session_id.clone(),
        result: Some(Value::String(env.result_text.clone())),
        is_error: Some(env.is_error),
        error: None,
        subtype: None,
        total_cost_usd: env.total_cost_usd,
    };
    RunOutput {
        exit_code,
        session_id: env.session_id,
        total_cost_usd: env.total_cost_usd,
        result_text: env.result_text,
        parsed: Some(parsed),
        parse_ok: true,
        events,
    }
}

#[cfg(test)]
fn parse_stream(stdout: &str) -> StreamParse {
    let mut streamed = StreamParse::default();
    for line in stdout.lines() {
        ingest_stdout_line(&mut streamed, line);
    }
    streamed
}

const PREVIEW_MAX_BYTES: usize = 8192;

fn ingest_stdout_line(state: &mut StreamParse, line: &str) -> Vec<TurnEvent> {
    let start = state.events.len();
    let trimmed = line.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return Vec::new();
    };
    match value.get("type").and_then(Value::as_str) {
        Some("assistant") => push_tool_blocks(state, &value, TurnEventKind::ToolUse),
        Some("user") => push_tool_blocks(state, &value, TurnEventKind::ToolResult),
        Some("result") => {
            if let Some(env) = envelope_from_value(&value) {
                state.envelope = Some(env);
            }
        }
        _ => {}
    }
    state.events[start..].to_vec()
}

fn push_tool_blocks(state: &mut StreamParse, value: &Value, kind: TurnEventKind) {
    let Some(blocks) = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let block_type = match kind {
        TurnEventKind::ToolUse => "tool_use",
        TurnEventKind::ToolResult => "tool_result",
    };
    for block in blocks {
        let Some(block) = block.as_object() else {
            continue;
        };
        if block.get("type").and_then(Value::as_str) != Some(block_type) {
            continue;
        }
        match kind {
            TurnEventKind::ToolUse => {
                state.events.push(TurnEvent {
                    kind,
                    tool_use_id: json_string(block, "id"),
                    name: json_string(block, "name"),
                    is_error: false,
                    preview: cut_preview(&json_preview(block.get("input"))),
                });
            }
            TurnEventKind::ToolResult => {
                let tool_use_id = json_string(block, "tool_use_id");
                let name = tool_name_for_id(state, &tool_use_id);
                let is_error = block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                state.events.push(TurnEvent {
                    kind,
                    tool_use_id,
                    name,
                    is_error,
                    preview: cut_preview(&content_preview(block.get("content"))),
                });
            }
        }
    }
}

fn tool_name_for_id(state: &StreamParse, tool_use_id: &str) -> String {
    if tool_use_id.is_empty() {
        return String::new();
    }
    state
        .events
        .iter()
        .rev()
        .find(|event| event.kind == TurnEventKind::ToolUse && event.tool_use_id == tool_use_id)
        .map(|event| event.name.clone())
        .unwrap_or_default()
}

fn json_string(obj: &serde_json::Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn json_preview(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn content_preview(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| content_preview(Some(item)))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if map.contains_key("content") {
                return content_preview(map.get("content"));
            }
            value.to_string()
        }
        other => other.to_string(),
    }
}

fn cut_preview(text: &str) -> String {
    if text.len() <= PREVIEW_MAX_BYTES {
        return text.to_string();
    }
    let mut end = PREVIEW_MAX_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
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
    let errors = obj.get("errors").and_then(joined_errors);
    let subtype = obj.get("subtype").and_then(Value::as_str);
    let is_error = obj
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| subtype_is_error(subtype));
    let result_text = result
        .filter(|s| !s.is_empty())
        .or(error.filter(|s| !s.is_empty()))
        .or(errors)
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

fn subtype_is_error(subtype: Option<&str>) -> bool {
    matches!(
        subtype,
        Some(
            "error_during_execution"
                | "error_max_turns"
                | "error_max_budget_usd"
                | "error_max_structured_output_retries"
        )
    )
}

fn joined_errors(value: &Value) -> Option<String> {
    let parts: Vec<&str> = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
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

/// Accept a chat API key for the child environment. Admin keys are refused.
pub(crate) fn cli_key_from_raw(raw: Option<&str>) -> Result<Option<String>, CliError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if secrets::is_admin_api_key(trimmed) {
        return Err(CliError::Message(
            "Admin API keys are not sent to claude -p. Save an sk-ant-api key for chat.".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
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
    use crate::models::TurnEventKind;
    use serde_json::{json, Value};

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

    fn pair(args: &[String], name: &str, value: &str) -> bool {
        args.windows(2).any(|item| item[0] == name && item[1] == value)
    }

    #[test]
    fn read_only_allows_project_skills_without_writes() {
        for preset in ["read", "readonly"] {
            let args = permission_args(preset);
            assert!(pair(&args, "--permission-mode", "dontAsk"));
            assert!(pair(&args, "--permission-prompts", "none"));
            let tools = &args[args.iter().position(|arg| arg == "--allowedTools").unwrap() + 1];
            for name in ["Skill", "Read", "Grep", "Glob"] {
                assert!(tools.split(',').any(|tool| tool == name), "{name} missing");
            }
            for name in ["Edit", "Write", "Bash", "PowerShell"] {
                assert!(!tools.split(',').any(|tool| tool == name), "{name} leaked");
            }
            assert!(!args.iter().any(|arg| arg.contains("bypass") || arg.contains("dangerously")));
            assert!(!args.iter().any(|arg| arg == "--safe-mode" || arg == "--disable-slash-commands"));
        }
    }

    #[test]
    fn read_edit_allows_file_edits_and_skills_but_not_shell() {
        let args = permission_args("read_edit");
        assert!(pair(&args, "--permission-mode", "acceptEdits"));
        let tools = &args[args.iter().position(|arg| arg == "--allowedTools").unwrap() + 1];
        for name in ["Skill", "Read", "Edit", "Write"] {
            assert!(tools.split(',').any(|tool| tool == name));
        }
        assert!(!tools.split(',').any(|tool| tool == "Bash" || tool == "PowerShell"));
        assert!(!args.iter().any(|arg| arg.contains("bypass") || arg.contains("dangerously")));
    }

    #[test]
    fn unrestricted_passes_permission_bypass_without_a_tool_allowlist() {
        let args = permission_args("full");
        assert!(pair(&args, "--permission-mode", "bypassPermissions"));
        assert!(args.iter().any(|arg| arg == "--dangerously-skip-permissions"));
        assert!(!args.iter().any(|arg| arg == "--allowedTools"));
        assert!(!args.iter().any(|arg| arg == "--safe-mode" || arg == "--disable-slash-commands"));
    }

    #[test]
    fn admin_key_is_never_accepted_for_the_cli() {
        let err = cli_key_from_raw(Some("sk-ant-admin01-example")).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("not sent"));
        assert!(!message.contains("example"));
        assert_eq!(
            cli_key_from_raw(Some("  sk-ant-api03-example  "))
                .unwrap()
                .as_deref(),
            Some("sk-ant-api03-example")
        );
        assert!(cli_key_from_raw(Some("   ")).unwrap().is_none());
        assert!(cli_key_from_raw(None).unwrap().is_none());
    }

    #[test]
    fn whole_document_envelope_unwraps_for_old_transcripts() {
        let env = envelope_if_whole_document(&sample_envelope()).expect("envelope");
        assert_eq!(env.result_text, "Hello\nworld");
        assert_eq!(env.session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn npm_cmd_shim_resolves_to_claude_exe() {
        let root = std::env::temp_dir().join(format!("useupclaude-shim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let exe_dir = root
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin");
        fs::create_dir_all(&exe_dir).unwrap();
        let exe = exe_dir.join("claude.exe");
        fs::write(&exe, b"").unwrap();
        let cmd = root.join("claude.cmd");
        fs::write(
            &cmd,
            "\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\" %*\n",
        )
        .unwrap();
        assert_eq!(prefer_direct_executable(&cmd), exe);
        assert_eq!(prefer_direct_executable(&exe), exe);
        let elsewhere = root.join("other");
        fs::create_dir_all(&elsewhere).unwrap();
        let quoted = elsewhere.join("claude.cmd");
        fs::write(&quoted, format!("\"{}\" %*\n", exe.display())).unwrap();
        assert_eq!(prefer_direct_executable(&quoted), exe);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_stays_off_argv_and_flags_stay() {
        let prompt = "say \"hello\" & echo off ".repeat(400);
        let args = print_mode_args(
            Some("sess-1"),
            Some("claude-opus"),
            "read",
            Some("HIGH"),
            true,
        );
        assert!(pair(&args, "--permission-mode", "dontAsk"));
        assert!(pair(&args, "--resume", "sess-1"));
        assert!(pair(&args, "--model", "claude-opus"));
        assert!(pair(&args, "--effort", "high"));
        assert!(pair(&args, "--settings", FAST_MODE_SETTINGS));
        let settings = &args[args.iter().position(|arg| arg == "--settings").unwrap() + 1];
        assert_eq!(settings, r#"{"fastMode": true}"#);
        assert!(!settings.contains('/') && !settings.contains('\\'));
        assert!(args.iter().any(|arg| arg == "-p"));
        assert!(pair(&args, "--output-format", "stream-json"));
        assert!(args.iter().any(|arg| arg == "--verbose"));
        assert!(!args.iter().any(|arg| arg == "--include-partial-messages"));
        assert!(!args
            .iter()
            .any(|arg| arg == &prompt || arg.contains("hello") || arg.contains('&')));
        assert!(args.iter().map(|arg| arg.len()).sum::<usize>() < 8_191);
        assert!(prompt.len() > 8_191);
        let no_model = print_mode_args(None, Some(""), "full", Some("nope"), false);
        assert!(!no_model.iter().any(|arg| arg == "--model"));
        assert!(!no_model.iter().any(|arg| arg == "--effort"));
        assert!(!no_model.iter().any(|arg| arg == "--settings"));
        assert!(no_model.iter().any(|arg| arg == "--dangerously-skip-permissions"));
        assert!(!no_model.iter().any(|arg| arg == "--resume"));
        assert!(pair(&no_model, "--output-format", "stream-json"));
        assert!(no_model.iter().any(|arg| arg == "--verbose"));
        assert!(!no_model
            .iter()
            .any(|arg| arg == "--include-partial-messages"));
        let defaults = print_mode_args(None, None, "read", None, false);
        assert!(!defaults.iter().any(|arg| arg == "--effort" || arg == "--settings"));
        for level in ["low", "medium", "high", "xhigh", "max"] {
            let level_args = print_mode_args(None, None, "read", Some(level), false);
            assert!(pair(&level_args, "--effort", level), "{level}");
            assert!(!level_args.iter().any(|arg| arg == "--settings"));
            assert!(!level_args.iter().any(|arg| arg.contains("hello") || arg.contains('&')));
        }
        assert!(normalize_effort(Some("  xhigh ")).as_deref() == Some("xhigh"));
        assert!(normalize_effort(None).is_none());
    }

    #[test]
    fn stream_lines_keep_result_cost_and_tool_events() {
        let long = format!("{}€", "y".repeat(8191));
        let stdout = ndjson(&[
            json!({
                "type": "system",
                "subtype": "init",
                "session_id": "init-sid",
                "total_cost_usd": 99
            }),
            json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "calling the tool"},
                        {
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "Read",
                            "input": {"path": "a.txt"}
                        }
                    ]
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "is_error": true,
                        "content": long
                    }]
                }
            }),
            json!({
                "type": "assistant",
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_3",
                        "name": "Grep",
                        "input": {"pattern": "x"}
                    }]
                }
            }),
            json!({
                "type": "user",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_3",
                        "content": [{"type": "text", "text": "from-blocks"}]
                    }]
                }
            }),
            json!({"type": "not_a_real_event", "text": "ignore me"}),
            json!({
                "type": "result",
                "subtype": "success",
                "is_error": false,
                "result": "Done now",
                "session_id": "sess-9",
                "total_cost_usd": 1.5
            }),
            json!({"type": "rate_limit_event", "status": "ok"}),
        ]);
        let out = parse_output(0, stdout, String::new());
        assert!(out.parse_ok);
        assert_eq!(out.result_text, "Done now");
        assert!(!out.result_text.contains("calling the tool"));
        assert!(!out.result_text.contains("yyyy"));
        assert_eq!(out.session_id.as_deref(), Some("sess-9"));
        assert_eq!(out.total_cost_usd, Some(1.5));
        assert_eq!(out.events.len(), 4);
        assert_eq!(out.events[0].kind, TurnEventKind::ToolUse);
        assert_eq!(out.events[0].tool_use_id, "toolu_1");
        assert_eq!(out.events[0].name, "Read");
        assert!(!out.events[0].is_error);
        assert_eq!(out.events[0].preview, r#"{"path":"a.txt"}"#);
        assert_eq!(out.events[1].kind, TurnEventKind::ToolResult);
        assert_eq!(out.events[1].tool_use_id, "toolu_1");
        assert_eq!(out.events[1].name, "Read");
        assert!(out.events[1].is_error);
        assert_eq!(out.events[1].preview, "y".repeat(8191));
        assert!(out.events[1].preview.len() <= 8192);
        assert!(!out.events[1].preview.contains('€'));
        let wire = serde_json::to_value(&out.events).unwrap();
        assert_eq!(wire[0]["kind"], "tool_use");
        assert_eq!(wire[0]["toolUseId"], "toolu_1");
        assert_eq!(wire[0]["name"], "Read");
        assert_eq!(wire[0]["isError"], false);
        assert_eq!(wire[1]["kind"], "tool_result");
        assert_eq!(wire[1]["toolUseId"], "toolu_1");
        assert_eq!(wire[1]["isError"], true);
        assert_eq!(out.events[2].kind, TurnEventKind::ToolUse);
        assert_eq!(out.events[2].name, "Grep");
        assert_eq!(out.events[3].kind, TurnEventKind::ToolResult);
        assert_eq!(out.events[3].tool_use_id, "toolu_3");
        assert_eq!(out.events[3].name, "Grep");
        assert!(!out.events[3].is_error);
        assert_eq!(out.events[3].preview, "from-blocks");

        let failed = parse_output(
            1,
            ndjson(&[
                json!({
                    "type": "assistant",
                    "message": {
                        "content": [{
                            "type": "tool_use",
                            "id": "toolu_2",
                            "name": "Bash",
                            "input": {}
                        }]
                    }
                }),
                json!({
                    "type": "result",
                    "subtype": "error_during_execution",
                    "is_error": true,
                    "errors": ["disk full", "gave up"],
                    "session_id": "err-1",
                    "total_cost_usd": 0.25
                }),
                json!({"type": "system", "subtype": "status"}),
            ]),
            String::new(),
        );
        assert!(failed.parse_ok);
        assert_eq!(failed.result_text, "disk full\ngave up");
        assert!(!failed.result_text.contains('{'));
        assert_eq!(failed.session_id.as_deref(), Some("err-1"));
        assert_eq!(failed.total_cost_usd, Some(0.25));
        assert_eq!(failed.parsed.unwrap().is_error, Some(true));
        assert_eq!(failed.events.len(), 1);
        assert_eq!(failed.events[0].kind, TurnEventKind::ToolUse);
        assert_eq!(failed.events[0].name, "Bash");
        assert_eq!(failed.events[0].tool_use_id, "toolu_2");
    }

    fn ndjson(lines: &[Value]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }
}
