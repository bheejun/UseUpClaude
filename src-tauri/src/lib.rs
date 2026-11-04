mod cli;
mod models;
mod secrets;
mod store;

use chrono::Utc;
use cli::{ActiveRun, RunSpec};
use models::*;
use std::path::PathBuf;
use std::sync::Arc;
use store::AppStore;
use tauri::State;
use uuid::Uuid;

struct AppState {
    store: AppStore,
    active: Arc<ActiveRun>,
}

fn err(e: impl ToString) -> String {
    e.to_string()
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state.store.load_settings().map_err(err)
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    state.store.save_settings(&settings).map_err(err)?;
    Ok(settings)
}

#[tauri::command]
fn get_key_status() -> Result<KeyStatus, String> {
    match secrets::load_api_key() {
        Ok(Some(k)) => Ok(KeyStatus {
            configured: true,
            masked: Some(secrets::mask_key(&k)),
        }),
        Ok(None) => Ok(KeyStatus {
            configured: false,
            masked: None,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_api_key(key: String) -> Result<KeyStatus, String> {
    secrets::save_api_key(&key).map_err(err)?;
    get_key_status()
}

#[tauri::command]
fn delete_api_key() -> Result<KeyStatus, String> {
    secrets::delete_api_key().map_err(err)?;
    Ok(KeyStatus {
        configured: false,
        masked: None,
    })
}

#[tauri::command]
async fn detect_claude(state: State<'_, AppState>) -> Result<ClaudeDetectResult, String> {
    let settings = state.store.load_settings().map_err(err)?;
    Ok(cli::detect_claude(settings.claude_binary_path.as_deref()).await)
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    state.store.list_sessions().map_err(err)
}

#[tauri::command]
fn get_session(state: State<'_, AppState>, id: Uuid) -> Result<GuiSession, String> {
    state.store.get_session(id).map_err(err)
}

#[tauri::command]
fn delete_session(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    state.store.delete_session(id).map_err(err)
}

#[tauri::command]
fn create_session(
    state: State<'_, AppState>,
    cwd: String,
    model: Option<String>,
    allowed_tools_preset: Option<String>,
) -> Result<GuiSession, String> {
    let settings = state.store.load_settings().map_err(err)?;
    let now = Utc::now();
    let session = GuiSession {
        id: Uuid::new_v4(),
        cli_session_id: None,
        cwd,
        title: "New chat".into(),
        model: model.or(settings.model),
        allowed_tools_preset: allowed_tools_preset
            .unwrap_or(settings.allowed_tools_preset),
        created_at: now,
        updated_at: now,
        turns: vec![],
    };
    state.store.save_session(&session).map_err(err)?;
    Ok(session)
}

#[tauri::command]
async fn test_connection(state: State<'_, AppState>) -> Result<ConnectionTestResult, String> {
    let settings = state.store.load_settings().map_err(err)?;
    let key = secrets::load_api_key().map_err(err)?;
    if key.is_none() {
        return Ok(ConnectionTestResult {
            ok: false,
            message: "No API key saved. Paste an Anthropic API key in Settings.".into(),
            session_id: None,
            result_preview: None,
        });
    }

    let binary = match cli::resolve_binary(settings.claude_binary_path.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return Ok(ConnectionTestResult {
                ok: false,
                message: e.to_string(),
                session_id: None,
                result_preview: None,
            });
        }
    };

    let cwd = settings
        .default_cwd
        .as_ref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::env::temp_dir());

    let out = cli::run_claude_managed(
        RunSpec {
            binary: &binary,
            prompt: "Reply with exactly: pong",
            cwd: &cwd,
            resume: None,
            model: settings.model.as_deref(),
            allowed_tools_preset: "read",
            timeout_secs: 120,
            api_key: None,
        },
        state.active.clone(),
    )
    .await
    .map_err(err)?;

    let ok = out.exit_code == 0
        && out
            .parsed
            .as_ref()
            .map(|p| p.is_error != Some(true))
            .unwrap_or(true);

    Ok(ConnectionTestResult {
        ok,
        message: if ok {
            "Connection OK".into()
        } else {
            format!(
                "CLI exited {} — {}",
                out.exit_code,
                truncate(&out.result_text, 400)
            )
        },
        session_id: out.session_id,
        result_preview: Some(truncate(&out.result_text, 200)),
    })
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    request: SendMessageRequest,
) -> Result<SendMessageResponse, String> {
    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Prompt is empty".into());
    }

    let settings = state.store.load_settings().map_err(err)?;
    if secrets::load_api_key().map_err(err)?.is_none() {
        return Err("No API key configured. Open Settings and paste your Anthropic API key.".into());
    }

    let binary = cli::resolve_binary(settings.claude_binary_path.as_deref()).map_err(err)?;

    let mut session = if let Some(id) = request.session_id {
        state.store.get_session(id).map_err(err)?
    } else {
        let cwd = request
            .cwd
            .clone()
            .or(settings.default_cwd.clone())
            .ok_or_else(|| "Working directory required".to_string())?;
        create_session_inner(
            &state.store,
            cwd,
            request.model.clone().or(settings.model.clone()),
            request
                .allowed_tools_preset
                .clone()
                .or(Some(settings.allowed_tools_preset.clone())),
        )
        .map_err(err)?
    };

    if session.title == "New chat" {
        session.title = truncate(&prompt, 60);
    }

    let now = Utc::now();
    let user_turn = GuiTurn {
        id: Uuid::new_v4(),
        role: TurnRole::User,
        content: prompt.clone(),
        exit_code: None,
        started_at: now,
        finished_at: Some(now),
        status: TurnStatus::Done,
    };
    session.turns.push(user_turn);

    let pending = GuiTurn {
        id: Uuid::new_v4(),
        role: TurnRole::Assistant,
        content: String::new(),
        exit_code: None,
        started_at: Utc::now(),
        finished_at: None,
        status: TurnStatus::Pending,
    };
    let pending_id = pending.id;
    session.turns.push(pending);
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;

    let cwd = PathBuf::from(&session.cwd);
    if !cwd.is_dir() {
        let msg = format!("cwd does not exist: {}", session.cwd);
        return fail_pending(&state, session, pending_id, msg, None);
    }

    let resume = session.cli_session_id.clone();
    let model = request
        .model
        .or(session.model.clone())
        .or(settings.model.clone());
    let tools = request
        .allowed_tools_preset
        .unwrap_or_else(|| session.allowed_tools_preset.clone());

    let run = cli::run_claude_managed(
        RunSpec {
            binary: &binary,
            prompt: &prompt,
            cwd: &cwd,
            resume: resume.as_deref(),
            model: model.as_deref(),
            allowed_tools_preset: &tools,
            timeout_secs: settings.timeout_secs,
            api_key: None,
        },
        state.active.clone(),
    )
    .await;

    match run {
        Ok(out) => {
            let is_cli_error = out.exit_code != 0
                || out
                    .parsed
                    .as_ref()
                    .and_then(|p| p.is_error)
                    .unwrap_or(false);

            if let Some(sid) = &out.session_id {
                session.cli_session_id = Some(sid.clone());
            }

            let turn = session
                .turns
                .iter_mut()
                .find(|t| t.id == pending_id)
                .ok_or_else(|| "pending turn missing".to_string())?;

            if is_cli_error {
                turn.role = TurnRole::Error;
                turn.status = TurnStatus::Failed;
                turn.content = if out.result_text.trim().is_empty() {
                    format!(
                        "CLI failed (exit {}). {}",
                        out.exit_code,
                        redact_secrets(&out.stderr)
                    )
                } else {
                    out.result_text.clone()
                };
            } else {
                turn.role = TurnRole::Assistant;
                turn.status = TurnStatus::Done;
                turn.content = out.result_text.clone();
            }
            turn.exit_code = Some(out.exit_code);
            turn.finished_at = Some(Utc::now());

            let assistant_turn = turn.clone();
            session.updated_at = Utc::now();
            state.store.save_session(&session).map_err(err)?;

            if is_cli_error {
                // Still return session so UI can show the error bubble;
                // use Err only for hard failures before spawn.
            }

            Ok(SendMessageResponse {
                session,
                assistant_turn,
            })
        }
        Err(cli::CliError::Cancelled) => {
            fail_pending(&state, session, pending_id, "Cancelled".into(), None)
        }
        Err(e) => fail_pending(&state, session, pending_id, e.to_string(), None),
    }
}

#[tauri::command]
async fn cancel_active(state: State<'_, AppState>) -> Result<(), String> {
    cli::cancel_run(&state.active).await.map_err(err)
}

#[tauri::command]
fn data_dir(state: State<'_, AppState>) -> String {
    state.store.data_root().display().to_string()
}

fn create_session_inner(
    store: &AppStore,
    cwd: String,
    model: Option<String>,
    allowed_tools_preset: Option<String>,
) -> Result<GuiSession, store::StoreError> {
    let settings = store.load_settings()?;
    let now = Utc::now();
    let session = GuiSession {
        id: Uuid::new_v4(),
        cli_session_id: None,
        cwd,
        title: "New chat".into(),
        model: model.or(settings.model),
        allowed_tools_preset: allowed_tools_preset
            .unwrap_or(settings.allowed_tools_preset),
        created_at: now,
        updated_at: now,
        turns: vec![],
    };
    store.save_session(&session)?;
    Ok(session)
}

fn fail_pending(
    state: &AppState,
    mut session: GuiSession,
    pending_id: Uuid,
    message: String,
    exit_code: Option<i32>,
) -> Result<SendMessageResponse, String> {
    let turn = session
        .turns
        .iter_mut()
        .find(|t| t.id == pending_id)
        .ok_or_else(|| "pending turn missing".to_string())?;
    turn.role = TurnRole::Error;
    turn.status = TurnStatus::Failed;
    turn.content = message;
    turn.exit_code = exit_code;
    turn.finished_at = Some(Utc::now());
    let assistant_turn = turn.clone();
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;
    Ok(SendMessageResponse {
        session,
        assistant_turn,
    })
}

fn truncate(s: &str, max: usize) -> String {
    let mut t = s.chars().take(max).collect::<String>();
    if s.chars().count() > max {
        t.push('…');
    }
    t
}

fn redact_secrets(s: &str) -> String {
    // Avoid echoing anything that looks like an API key in error panels.
    let mut out = s.to_string();
    if let Ok(Some(key)) = secrets::load_api_key() {
        if !key.is_empty() {
            out = out.replace(&key, "[REDACTED]");
        }
    }
    out
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = AppStore::open().expect("failed to open app store");
    let active = Arc::new(ActiveRun::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { store, active })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_key_status,
            save_api_key,
            delete_api_key,
            detect_claude,
            list_sessions,
            get_session,
            delete_session,
            create_session,
            test_connection,
            send_message,
            cancel_active,
            data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
