mod catalog;
mod cli;
mod cost;
mod discover;
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
fn save_settings(state: State<'_, AppState>, mut settings: AppSettings) -> Result<AppSettings, String> {
    settings.workspace_id = normalize_workspace_id(settings.workspace_id)?;
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
fn get_admin_key_status() -> Result<KeyStatus, String> {
    match secrets::load_admin_api_key() {
        Ok(Some(key)) => Ok(KeyStatus {
            configured: true,
            masked: Some(secrets::mask_key(&key)),
        }),
        Ok(None) => Ok(KeyStatus {
            configured: false,
            masked: None,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_admin_api_key(key: String) -> Result<KeyStatus, String> {
    secrets::save_admin_api_key(&key).map_err(err)?;
    get_admin_key_status()
}

#[tauri::command]
fn delete_admin_api_key() -> Result<KeyStatus, String> {
    secrets::delete_admin_api_key().map_err(err)?;
    Ok(KeyStatus {
        configured: false,
        masked: None,
    })
}

#[tauri::command]
async fn get_workspace_spend(state: State<'_, AppState>) -> Result<cost::WorkspaceSpend, String> {
    let settings = state.store.load_settings().map_err(err)?;
    let workspace = settings
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let key = secrets::load_admin_api_key().map_err(err)?;
    match (key, workspace) {
        (Some(key), Some(workspace)) => Ok(cost::fetch_month_spend(&key, workspace).await),
        _ => Ok(cost::WorkspaceSpend::hidden()),
    }
}

#[tauri::command]
fn session_costs(state: State<'_, AppState>) -> Result<SessionCostReport, String> {
    let sessions = state.store.load_sessions().map_err(err)?;
    let mut lines = Vec::with_capacity(sessions.len());
    let mut total = 0.0;
    let mut any = false;
    for session in sessions {
        let cost_usd = session_cost_usd(&session);
        if let Some(cost) = cost_usd {
            total += cost;
            any = true;
        }
        lines.push(SessionCostLine {
            id: session.id,
            title: session.title,
            cwd: session.cwd,
            updated_at: session.updated_at,
            cost_usd,
        });
    }
    Ok(SessionCostReport {
        total_usd: if any { Some(total) } else { None },
        sessions: lines,
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
    load_session(&state, id)
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
    cli_session_id: Option<String>,
    title: Option<String>,
) -> Result<GuiSession, String> {
    let settings = state.store.load_settings().map_err(err)?;
    let now = Utc::now();
    let cli_session_id = match cli_session_id {
        Some(raw) if !raw.trim().is_empty() => Some(
            Uuid::parse_str(raw.trim())
                .map_err(|_| "Claude session id must be a UUID".to_string())?
                .to_string(),
        ),
        _ => None,
    };
    let title = title
        .map(|t| truncate(t.trim(), 80))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "New chat".into());
    let session = GuiSession {
        id: Uuid::new_v4(),
        cli_session_id,
        cwd,
        title,
        model: normalize_model(model),
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
fn discover_claude_sessions() -> Vec<DiscoveredProject> {
    discover::discover_claude_sessions()
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

    let ok = out.parse_ok
        && out.exit_code == 0
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
        load_session(&state, id)?
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

    let fresh_gui_chat = session.cli_session_id.is_none()
        && !session
            .turns
            .iter()
            .any(|t| matches!(t.role, TurnRole::User));
    if fresh_gui_chat {
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
        cost_usd: None,
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
        cost_usd: None,
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
    // Existing chats keep the model stored on the session. Null means CLI default, so --model is omitted.
    let model = if request.session_id.is_some() {
        normalize_model(session.model.clone())
    } else {
        normalize_model(
            request
                .model
                .clone()
                .or(session.model.clone())
                .or(settings.model.clone()),
        )
    };
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
            let is_cli_error = !out.parse_ok
                || out.exit_code != 0
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

            let mut content = redact_secrets(&out.result_text);
            if content.trim().is_empty() {
                content = format!("Claude CLI failed (exit {}).", out.exit_code);
            }

            if is_cli_error {
                turn.role = TurnRole::Error;
                turn.status = TurnStatus::Failed;
            } else {
                turn.role = TurnRole::Assistant;
                turn.status = TurnStatus::Done;
            }
            turn.content = content;
            turn.cost_usd = out.total_cost_usd;
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

fn load_session(state: &AppState, id: Uuid) -> Result<GuiSession, String> {
    let mut session = state.store.get_session(id).map_err(err)?;
    if sanitize_session(&mut session) {
        state.store.save_session(&session).map_err(err)?;
    }
    Ok(session)
}

/// Older builds stored the raw `--output-format json` object as the assistant
/// message. Keep `session_id` for `--resume` and show `result` instead.
fn sanitize_session(session: &mut GuiSession) -> bool {
    let mut changed = false;
    let mut recovered_session: Option<String> = None;
    let missing_cli_session = session.cli_session_id.is_none();
    for turn in &mut session.turns {
        if !matches!(turn.role, TurnRole::Assistant | TurnRole::Error) {
            continue;
        }
        let Some(env) = cli::envelope_if_whole_document(&turn.content) else {
            continue;
        };
        if turn.content != env.result_text {
            turn.content = env.result_text;
            changed = true;
        }
        if turn.cost_usd.is_none() && env.total_cost_usd.is_some() {
            turn.cost_usd = env.total_cost_usd;
            changed = true;
        }
        if env.is_error && turn.role != TurnRole::Error {
            turn.role = TurnRole::Error;
            turn.status = TurnStatus::Failed;
            changed = true;
        }
        if missing_cli_session {
            if let Some(sid) = env.session_id {
                recovered_session = Some(sid);
            }
        }
    }
    if missing_cli_session {
        if let Some(sid) = recovered_session {
            session.cli_session_id = Some(sid);
            changed = true;
        }
    }
    changed
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
        model: normalize_model(model),
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
    for key in [secrets::load_api_key(), secrets::load_admin_api_key()] {
        if let Ok(Some(key)) = key {
            if !key.is_empty() {
                out = out.replace(&key, "[REDACTED]");
            }
        }
    }
    out
}

fn normalize_workspace_id(raw: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > 128
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("Workspace ID is invalid.".into());
    }
    if trimmed.starts_with("sk-ant") {
        return Err("Workspace ID is not an API key.".into());
    }
    Ok(Some(trimmed.to_string()))
}

fn turn_cost_usd(turn: &GuiTurn) -> Option<f64> {
    if let Some(cost) = turn.cost_usd {
        if cost.is_finite() {
            return Some(cost);
        }
    }
    cli::envelope_if_whole_document(&turn.content)
        .and_then(|env| env.total_cost_usd)
        .filter(|cost| cost.is_finite())
}

fn normalize_model(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || trimmed.starts_with("sk-ant")
        || trimmed.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(trimmed)
}

#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<catalog::ModelCatalog, String> {
    let settings = state.store.load_settings().map_err(err)?;
    let stored = secrets::load_api_key().map_err(err)?;
    let Some(key) = catalog::chat_key_for_catalog(stored.as_deref()) else {
        return Ok(catalog::ModelCatalog::no_key());
    };
    Ok(catalog::fetch_models(&key, settings.workspace_id.as_deref()).await)
}

fn session_cost_usd(session: &GuiSession) -> Option<f64> {
    let mut sum = 0.0;
    let mut any = false;
    for turn in &session.turns {
        if let Some(cost) = turn_cost_usd(turn) {
            sum += cost;
            any = true;
        }
    }
    if any && sum.is_finite() { Some(sum) } else { None }
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
            get_admin_key_status,
            save_admin_api_key,
            delete_admin_api_key,
            get_workspace_spend,
            session_costs,
            list_models,
            detect_claude,
            list_sessions,
            get_session,
            delete_session,
            create_session,
            discover_claude_sessions,
            test_connection,
            send_message,
            cancel_active,
            data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
