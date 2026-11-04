use crate::models::{ClaudeDetectResult, ClaudeJsonOutput};
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
    pub stdout: String,
    pub stderr: String,
    pub parsed: Option<ClaudeJsonOutput>,
    pub result_text: String,
    pub session_id: Option<String>,
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
    let parsed: Option<ClaudeJsonOutput> = {
        let trimmed = stdout.trim();
        let candidate = if let Some(idx) = trimmed.rfind('{') {
            &trimmed[idx..]
        } else {
            trimmed
        };
        serde_json::from_str(candidate).ok()
    };

    let session_id = parsed.as_ref().and_then(|p| p.session_id.clone());
    let result_text = if let Some(p) = &parsed {
        if let Some(r) = &p.result {
            match r {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        } else if let Some(err) = &p.error {
            err.clone()
        } else if !stdout.trim().is_empty() {
            stdout.clone()
        } else {
            stderr.clone()
        }
    } else if !stdout.trim().is_empty() {
        stdout.clone()
    } else {
        stderr.clone()
    };

    RunOutput {
        exit_code,
        stdout,
        stderr,
        parsed,
        result_text,
        session_id,
    }
}

pub async fn cancel_run(active: &ActiveRun) -> Result<(), CliError> {
    active.cancel_flag.store(true, Ordering::SeqCst);
    if let Some(mut c) = active.child.lock().await.take() {
        let _ = c.kill().await;
    }
    Ok(())
}
