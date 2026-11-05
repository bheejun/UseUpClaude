mod catalog;
mod cli;
mod discover;
mod models;
mod project_file;
mod secrets;
mod skills;
mod store;

use chrono::Utc;
use cli::{ActiveRun, RunSpec};
use models::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use store::AppStore;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

struct AppState {
    store: AppStore,
    runs: Arc<RunSet>,
}

/// One in-flight `claude -p` per chat. Different chats run together.
struct RunSet {
    inner: Mutex<HashMap<Uuid, Arc<ActiveRun>>>,
}

struct RunningTurn {
    runs: Arc<RunSet>,
    id: Uuid,
}

impl Drop for RunningTurn {
    fn drop(&mut self) {
        if let Ok(mut map) = self.runs.inner.lock() {
            map.remove(&self.id);
        }
    }
}

impl RunSet {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn begin(self: &Arc<Self>, id: Uuid) -> Result<(RunningTurn, Arc<ActiveRun>), String> {
        let mut map = self
            .inner
            .lock()
            .map_err(|_| "run lock poisoned".to_string())?;
        if map.contains_key(&id) {
            return Err("This chat already has a turn in progress.".into());
        }
        let run = Arc::new(ActiveRun::new());
        map.insert(id, run.clone());
        Ok((
            RunningTurn {
                runs: Arc::clone(self),
                id,
            },
            run,
        ))
    }

    fn get(&self, id: Uuid) -> Option<Arc<ActiveRun>> {
        self.inner.lock().ok()?.get(&id).cloned()
    }

    fn contains(&self, id: Uuid) -> bool {
        self.inner
            .lock()
            .map(|map| map.contains_key(&id))
            .unwrap_or(false)
    }
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
fn session_costs(state: State<'_, AppState>) -> Result<SessionCostReport, String> {
    let mut sessions = state.store.load_sessions().map_err(err)?;
    let mut lines = Vec::with_capacity(sessions.len());
    let mut total = 0.0;
    let mut any = false;
    for session in &mut sessions {
        if prepare_session(session) && !state.runs.contains(session.id) {
            state.store.save_session(session).map_err(err)?;
        }
        let cost_usd = session_cost_usd(session);
        if let Some(cost) = cost_usd {
            total += cost;
            any = true;
        }
        lines.push(SessionCostLine {
            id: session.id,
            title: session.title.clone(),
            cwd: session.cwd.clone(),
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
fn set_session_model(
    state: State<'_, AppState>,
    id: Uuid,
    model: Option<String>,
) -> Result<GuiSession, String> {
    if state.runs.contains(id) {
        return Err("This chat already has a turn in progress.".into());
    }
    let mut session = load_session(&state, id)?;
    session.model = normalize_model(model);
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;
    Ok(session)
}

#[tauri::command]
fn set_session_effort(
    state: State<'_, AppState>,
    id: Uuid,
    effort: Option<String>,
) -> Result<GuiSession, String> {
    if state.runs.contains(id) {
        return Err("This chat already has a turn in progress.".into());
    }
    let mut session = load_session(&state, id)?;
    session.effort = cli::normalize_effort(effort.as_deref());
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;
    Ok(session)
}

#[tauri::command]
fn set_session_fast_mode(
    state: State<'_, AppState>,
    id: Uuid,
    fast_mode: bool,
) -> Result<GuiSession, String> {
    if state.runs.contains(id) {
        return Err("This chat already has a turn in progress.".into());
    }
    let mut session = load_session(&state, id)?;
    session.fast_mode = fast_mode;
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;
    Ok(session)
}

#[tauri::command]
fn list_project_skills(cwd: String) -> Vec<skills::ProjectSkill> {
    skills::list_available_skills(std::path::Path::new(&cwd))
}

#[tauri::command]
fn delete_session(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    if state.runs.contains(id) {
        return Err("This chat already has a turn in progress.".into());
    }
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
        effort: None,
        fast_mode: false,
        allowed_tools_preset: allowed_tools_preset.unwrap_or(settings.allowed_tools_preset),
        created_at: now,
        updated_at: now,
        turns: vec![],
        turn_costs_are_incremental: true,
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
            model: None,
            effort: None,
            fast_mode: false,
            allowed_tools_preset: "read",
            timeout_secs: 120,
            api_key: None,
        },
        Arc::new(ActiveRun::new()),
        &|_| {},
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

const TURN_EVENT: &str = "turn-event";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TurnEventPayload {
    session_id: Uuid,
    #[serde(flatten)]
    event: TurnEvent,
}

fn turn_event_payload(session_id: Uuid, event: &TurnEvent) -> TurnEventPayload {
    TurnEventPayload {
        session_id,
        event: event.clone(),
    }
}

#[tauri::command]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SendMessageRequest,
) -> Result<SendMessageResponse, String> {
    let typed = request.prompt.trim().to_string();
    let requested_skill = request
        .skill
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    if typed.is_empty() && requested_skill.is_none() {
        return Err("Prompt is empty".into());
    }

    let settings = state.store.load_settings().map_err(err)?;
    if secrets::load_api_key().map_err(err)?.is_none() {
        return Err(
            "No API key configured. Open Settings and paste your Anthropic API key.".into(),
        );
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
            request.model.clone(),
            request
                .allowed_tools_preset
                .clone()
                .or(Some(settings.allowed_tools_preset.clone())),
        )
        .map_err(err)?
    };

    let cwd = PathBuf::from(&session.cwd);
    let prompt = match requested_skill.as_deref() {
        Some(name) if skills::is_skill_name(name) && skills::has_skill(&cwd, name) => {
            skills::compose_prompt(&typed, Some(name))
        }
        Some(_) => return Err("That skill is not available.".into()),
        None => typed,
    };
    // The composer chooses model, effort, and fast mode for this chat.
    // Null model omits --model. Null effort omits --effort. Fast mode off omits --settings.
    session.model = normalize_model(request.model.clone());
    session.effort = cli::normalize_effort(request.effort.as_deref());
    session.fast_mode = request.fast_mode;

    let fresh_gui_chat = session.cli_session_id.is_none()
        && !session
            .turns
            .iter()
            .any(|t| matches!(t.role, TurnRole::User));
    if fresh_gui_chat {
        session.title = truncate(&prompt, 60);
    }

    // Block a second turn on this chat before any transcript write.
    let (_running, active_run) = state.runs.begin(session.id)?;

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
        events: Vec::new(),
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
        events: Vec::new(),
    };
    let pending_id = pending.id;
    session.turns.push(pending);
    session.updated_at = Utc::now();
    state.store.save_session(&session).map_err(err)?;

    if !cwd.is_dir() {
        let msg = format!("cwd does not exist: {}", session.cwd);
        return fail_pending(&state, session, pending_id, msg, None, Vec::new());
    }

    let resume = session.cli_session_id.clone();
    let model = session.model.clone();
    let effort = session.effort.clone();
    let fast_mode = session.fast_mode;
    let tools = request
        .allowed_tools_preset
        .unwrap_or_else(|| session.allowed_tools_preset.clone());
    let session_id = session.id;
    let seen: Arc<Mutex<Vec<TurnEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_emit = Arc::clone(&seen);
    let on_event = move |event: &TurnEvent| {
        let mut stored = event.clone();
        stored.preview = redact_secrets(&stored.preview);
        if let Ok(mut guard) = seen_emit.lock() {
            guard.push(stored.clone());
        }
        let _ = app.emit(TURN_EVENT, turn_event_payload(session_id, &stored));
    };

    let run = cli::run_claude_managed(
        RunSpec {
            binary: &binary,
            prompt: &prompt,
            cwd: &cwd,
            resume: resume.as_deref(),
            model: model.as_deref(),
            effort: effort.as_deref(),
            fast_mode,
            allowed_tools_preset: &tools,
            timeout_secs: settings.timeout_secs,
            api_key: None,
        },
        active_run,
        &on_event,
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

            let prior = session_cost_usd(&session).unwrap_or(0.0);
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
            // CLI total_cost_usd is the running session total. Keep this turn's increase.
            turn.cost_usd = incremental_turn_cost(out.total_cost_usd, prior);
            turn.events = out
                .events
                .into_iter()
                .map(|mut event| {
                    event.preview = redact_secrets(&event.preview);
                    event
                })
                .collect();
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
        Err(cli::CliError::Cancelled) => fail_pending(
            &state,
            session,
            pending_id,
            "Cancelled".into(),
            None,
            seen_events(&seen),
        ),
        Err(e) => fail_pending(
            &state,
            session,
            pending_id,
            e.to_string(),
            None,
            seen_events(&seen),
        ),
    }
}

fn seen_events(seen: &Mutex<Vec<TurnEvent>>) -> Vec<TurnEvent> {
    seen.lock().map(|guard| guard.clone()).unwrap_or_default()
}

#[tauri::command]
async fn cancel_session(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    if let Some(run) = state.runs.get(id) {
        cli::cancel_run(&run).await.map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn data_dir(state: State<'_, AppState>) -> String {
    state.store.data_root().display().to_string()
}

#[tauri::command]
fn read_project_file(
    state: State<'_, AppState>,
    session_id: Uuid,
    path: String,
) -> Result<ProjectFileView, String> {
    let session = load_session(&state, session_id)?;
    project_file::read_in_project(Path::new(&session.cwd), &path)
}

/// Write a text file inside the session project. The session file never stores the body.
#[tauri::command]
fn write_project_file(
    state: State<'_, AppState>,
    session_id: Uuid,
    path: String,
    content: String,
) -> Result<(), String> {
    let session = load_session(&state, session_id)?;
    project_file::write_in_project(Path::new(&session.cwd), &path, &content)
}

/// One folder level inside the session project. No file bodies.
#[tauri::command]
fn list_project_dir(
    state: State<'_, AppState>,
    session_id: Uuid,
    path: String,
) -> Result<ProjectDirListing, String> {
    let session = load_session(&state, session_id)?;
    project_file::list_in_project(Path::new(&session.cwd), &path)
}

/// Paths from an answer that are files inside the session project. No file bodies.
#[tauri::command]
fn locate_project_files(
    state: State<'_, AppState>,
    session_id: Uuid,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let session = load_session(&state, session_id)?;
    let cwd = Path::new(&session.cwd);
    let mut kept = Vec::new();
    for path in paths.into_iter().take(48) {
        if project_file::locate_in_project(cwd, &path).is_ok() {
            kept.push(path);
        }
    }
    Ok(kept)
}

#[tauri::command]
fn get_project_workspace(
    state: State<'_, AppState>,
    cwd: String,
) -> Result<ProjectWorkspace, String> {
    state.store.load_workspace(&cwd).map_err(err)
}

#[tauri::command]
fn save_project_workspace(
    state: State<'_, AppState>,
    cwd: String,
    workspace: ProjectWorkspace,
) -> Result<ProjectWorkspace, String> {
    state.store.save_workspace(&cwd, &workspace).map_err(err)
}

fn load_session(state: &AppState, id: Uuid) -> Result<GuiSession, String> {
    let mut session = state.store.get_session(id).map_err(err)?;
    if prepare_session(&mut session) {
        state.store.save_session(&session).map_err(err)?;
    }
    Ok(session)
}

fn prepare_session(session: &mut GuiSession) -> bool {
    let sanitized = sanitize_session(session);
    let normalized = normalize_stored_turn_costs(session);
    sanitized || normalized
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
        effort: None,
        fast_mode: false,
        allowed_tools_preset: allowed_tools_preset.unwrap_or(settings.allowed_tools_preset),
        created_at: now,
        updated_at: now,
        turns: vec![],
        turn_costs_are_incremental: true,
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
    events: Vec<TurnEvent>,
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
    turn.events = events;
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
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(trimmed)
}

#[tauri::command]
async fn list_models() -> Result<catalog::ModelCatalog, String> {
    let stored = secrets::load_api_key().map_err(err)?;
    let Some(key) = catalog::chat_key_for_catalog(stored.as_deref()) else {
        return Ok(catalog::ModelCatalog::no_key());
    };
    Ok(catalog::fetch_models(&key).await)
}

const COST_EPS: f64 = 1e-6;

/// `reported` is Claude's running `total_cost_usd` for the resumed CLI session.
/// `prior` is the sum of turn costs already stored. The result is this turn only.
fn incremental_turn_cost(reported: Option<f64>, prior: f64) -> Option<f64> {
    let reported = reported.filter(|cost| cost.is_finite() && *cost >= 0.0)?;
    if !prior.is_finite() || prior <= COST_EPS {
        return Some(reported);
    }
    if reported + COST_EPS >= prior {
        Some((reported - prior).max(0.0))
    } else {
        Some(reported)
    }
}

/// Rewrite a session that stored the CLI running total on every turn.
fn normalize_stored_turn_costs(session: &mut GuiSession) -> bool {
    if session.turn_costs_are_incremental {
        return false;
    }
    let mut previous = 0.0;
    for turn in &mut session.turns {
        let Some(cost) = turn.cost_usd else {
            continue;
        };
        if !cost.is_finite() || cost < 0.0 {
            turn.cost_usd = None;
            continue;
        }
        let delta = if cost + COST_EPS >= previous {
            let next = (cost - previous).max(0.0);
            previous = cost;
            next
        } else {
            previous += cost;
            cost
        };
        turn.cost_usd = Some(delta);
    }
    session.turn_costs_are_incremental = true;
    true
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
    if any && sum.is_finite() {
        Some(sum)
    } else {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = secrets::forget_admin_api_key();
    let store = AppStore::open().expect("failed to open app store");
    let runs = Arc::new(RunSet::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState { store, runs })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_key_status,
            save_api_key,
            delete_api_key,
            session_costs,
            list_models,
            detect_claude,
            list_sessions,
            get_session,
            read_project_file,
            write_project_file,
            list_project_dir,
            locate_project_files,
            get_project_workspace,
            save_project_workspace,
            set_session_model,
            set_session_effort,
            set_session_fast_mode,
            list_project_skills,
            delete_session,
            create_session,
            discover_claude_sessions,
            test_connection,
            send_message,
            cancel_session,
            data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(cost: Option<f64>) -> GuiTurn {
        let now = Utc::now();
        GuiTurn {
            id: Uuid::new_v4(),
            role: TurnRole::Assistant,
            content: String::new(),
            exit_code: Some(0),
            started_at: now,
            finished_at: Some(now),
            status: TurnStatus::Done,
            cost_usd: cost,
            events: Vec::new(),
        }
    }

    fn session(costs: &[f64]) -> GuiSession {
        let now = Utc::now();
        GuiSession {
            id: Uuid::new_v4(),
            cli_session_id: None,
            cwd: "C:/tmp".into(),
            title: "t".into(),
            model: None,
            effort: None,
            fast_mode: false,
            allowed_tools_preset: "read".into(),
            created_at: now,
            updated_at: now,
            turns: costs.iter().copied().map(|cost| turn(Some(cost))).collect(),
            turn_costs_are_incremental: false,
        }
    }

    #[test]
    fn running_cli_total_becomes_per_turn_cost() {
        let raw = [
            0.4228374,
            2.4065111499999996,
            2.4638268999999995,
            21.409503899999997,
            21.939228899999996,
        ];
        let mut session = session(&raw);
        assert!(normalize_stored_turn_costs(&mut session));
        let sum = session_cost_usd(&session).unwrap();
        assert!((sum - raw[raw.len() - 1]).abs() < 1e-9);
        let last = session.turns.last().unwrap().cost_usd.unwrap();
        assert!(last < 1.0);
        assert!(!normalize_stored_turn_costs(&mut session));
        assert!((session_cost_usd(&session).unwrap() - sum).abs() < 1e-12);
    }

    #[test]
    fn old_turn_json_without_events_loads() {
        let raw = r#"{"id":"11111111-1111-1111-1111-111111111111","role":"assistant","content":"hi","exitCode":0,"startedAt":"2026-01-01T00:00:00Z","finishedAt":"2026-01-01T00:00:00Z","status":"done","costUsd":0.1}"#;
        let turn: GuiTurn = serde_json::from_str(raw).unwrap();
        assert!(turn.events.is_empty());
        assert_eq!(turn.content, "hi");
        let event: TurnEvent = serde_json::from_str(
            r#"{"kind":"tool_use","toolUseId":"toolu_1","name":"Write","isError":false,"preview":"{}"}"#,
        )
        .unwrap();
        assert!(event.path.is_none());
        let with_path: TurnEvent = serde_json::from_str(
            r#"{"kind":"tool_use","toolUseId":"toolu_2","name":"Edit","isError":false,"preview":"{}","path":"src/main.rs"}"#,
        )
        .unwrap();
        assert_eq!(with_path.path.as_deref(), Some("src/main.rs"));
        let saved = serde_json::to_value(&turn).unwrap();
        assert!(saved["events"].as_array().unwrap().is_empty());
    }

    #[test]
    fn turn_event_payload_matches_stored_event() {
        let session_id = Uuid::new_v4();
        let event = TurnEvent {
            kind: TurnEventKind::ToolResult,
            tool_use_id: "toolu_1".into(),
            name: "Read".into(),
            is_error: true,
            preview: "missing file".into(),
            path: None,
        };
        let payload = turn_event_payload(session_id, &event);
        assert_eq!(payload.event, event);
        let stored = serde_json::to_value(&event).unwrap();
        let emitted = serde_json::to_value(&payload).unwrap();
        for key in ["kind", "toolUseId", "name", "isError", "preview"] {
            assert_eq!(emitted.get(key), stored.get(key), "{key}");
        }
        assert_eq!(
            emitted.get("sessionId").and_then(|value| value.as_str()),
            Some(session_id.to_string()).as_deref()
        );
    }

    #[test]
    fn new_turn_stores_only_the_increase() {
        let prior = 21.409503899999997;
        let reported = 21.939228899999996;
        let delta = incremental_turn_cost(Some(reported), prior).unwrap();
        assert!((delta - (reported - prior)).abs() < 1e-12);
        assert!(delta < 1.0);
        assert!((incremental_turn_cost(Some(0.42), 0.0).unwrap() - 0.42).abs() < 1e-12);
        assert!((incremental_turn_cost(Some(0.5), 21.0).unwrap() - 0.5).abs() < 1e-12);
    }

    #[test]
    fn older_session_without_effort_or_fast_mode_still_loads() {
        let raw = r#"{
            "id":"11111111-1111-1111-1111-111111111111",
            "cliSessionId":null,
            "cwd":"C:/tmp",
            "title":"t",
            "model":"claude-opus",
            "allowedToolsPreset":"read",
            "createdAt":"2026-01-01T00:00:00Z",
            "updatedAt":"2026-01-01T00:00:00Z",
            "turns":[]
        }"#;
        let session: GuiSession = serde_json::from_str(raw).unwrap();
        assert!(session.effort.is_none());
        assert!(!session.fast_mode);
        assert_eq!(session.model.as_deref(), Some("claude-opus"));
    }

    #[test]
    fn one_run_per_session() {
        let runs = Arc::new(RunSet::new());
        let id = Uuid::new_v4();
        let (guard, _) = runs.begin(id).unwrap();
        assert!(runs.begin(id).is_err());
        let other = Uuid::new_v4();
        let (other_guard, _) = runs.begin(other).unwrap();
        drop(guard);
        assert!(runs.begin(id).is_ok());
        drop(other_guard);
    }
}
