use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub claude_binary_path: Option<String>,
    pub default_cwd: Option<String>,
    pub model: Option<String>,
    pub timeout_secs: u64,
    /// MVP default: read-only safer tools
    pub allowed_tools_preset: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            claude_binary_path: None,
            default_cwd: None,
            model: None,
            timeout_secs: 1800,
            allowed_tools_preset: "read".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    User,
    Assistant,
    System,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnStatus {
    Pending,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnEventKind {
    ToolUse,
    ToolResult,
    TaskStarted,
    TaskProgress,
    TaskUpdated,
    TaskNotification,
    BackgroundTasksChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnEvent {
    pub kind: TurnEventKind,
    pub tool_use_id: String,
    pub name: String,
    pub is_error: bool,
    pub preview: String,
    /// Path a Write or Edit touched. Absent for other tools and for older sessions.
    /// File bodies stay out of the session file; the panel reads them from the project folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectFileKind {
    Markdown,
    Text,
    Unsupported,
}

/// Live read of a project file. This is not stored on the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileView {
    pub path: String,
    pub kind: ProjectFileKind,
    pub content: Option<String>,
    pub truncated: bool,
}

/// One child of a project folder. Names and paths only; the body stays on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Unix milliseconds. Zero when the filesystem does not report a time.
    pub modified_ms: u64,
}

/// A project file named for the picker. No body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileRef {
    pub name: String,
    pub path: String,
    pub modified_ms: u64,
}

/// Files under the session folder, skipping heavy dependency directories.
/// This is not stored on the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileIndex {
    pub files: Vec<ProjectFileRef>,
    pub truncated: bool,
}

/// A recent project file plus a short display snippet.
/// The snippet is for the panel only and is never written to the session file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectFile {
    pub name: String,
    pub path: String,
    pub modified_ms: u64,
    pub kind: ProjectFileKind,
    pub preview: String,
}

/// One level of the session project folder. This is not stored on the session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirListing {
    pub path: String,
    pub entries: Vec<ProjectDirEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiTurn {
    pub id: Uuid,
    pub role: TurnRole,
    pub content: String,
    pub exit_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: TurnStatus,
    /// Cost of this turn only. Not the CLI session's running total.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Completed tool calls from this turn. `content` stays the result text.
    #[serde(default)]
    pub events: Vec<TurnEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiSession {
    pub id: Uuid,
    pub cli_session_id: Option<String>,
    pub cwd: String,
    pub title: String,
    pub model: Option<String>,
    /// Reasoning effort for this chat. `None` omits `--effort` on `claude -p`.
    #[serde(default)]
    pub effort: Option<String>,
    /// Fast mode for this chat. `false` omits `--settings` on `claude -p`.
    #[serde(default)]
    pub fast_mode: bool,
    pub allowed_tools_preset: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub turns: Vec<GuiTurn>,
    /// Older files stored the CLI running total on every turn. After one rewrite,
    /// each `cost_usd` is that turn only.
    #[serde(default)]
    pub turn_costs_are_incremental: bool,
    /// Latest Claude Code JSONL tip (`uuid`) this GUI session last synced from.
    /// Used to detect a newer branch tip before `--resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_tip_uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: Uuid,
    pub cli_session_id: Option<String>,
    pub cwd: String,
    pub title: String,
    pub model: Option<String>,
    pub allowed_tools_preset: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub turn_count: usize,
}

impl From<&GuiSession> for SessionSummary {
    fn from(s: &GuiSession) -> Self {
        Self {
            id: s.id,
            cli_session_id: s.cli_session_id.clone(),
            cwd: s.cwd.clone(),
            title: s.title.clone(),
            model: s.model.clone(),
            allowed_tools_preset: s.allowed_tools_preset.clone(),
            created_at: s.created_at,
            updated_at: s.updated_at,
            turn_count: s.turns.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDetectResult {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub configured: bool,
    pub masked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub session_id: Option<Uuid>,
    pub prompt: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    /// `None` or an unknown level omits `--effort` for this turn only.
    #[serde(default)]
    pub effort: Option<String>,
    /// `true` passes inline `--settings {"fastMode": true}` for this turn only.
    #[serde(default)]
    pub fast_mode: bool,
    pub allowed_tools_preset: Option<String>,
    /// Project skill to invoke for this send only. Empty means no skill.
    #[serde(default)]
    pub skill: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub session: GuiSession,
    pub assistant_turn: GuiTurn,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSession {
    pub session_id: String,
    pub title: String,
    pub modified_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredProject {
    /// Project directory when a session record included `cwd`.
    pub cwd: Option<String>,
    pub label: String,
    pub sessions: Vec<DiscoveredSession>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostLine {
    pub id: Uuid,
    pub title: String,
    pub cwd: String,
    pub updated_at: DateTime<Utc>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCostReport {
    pub total_usd: Option<f64>,
    pub sessions: Vec<SessionCostLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub message: String,
    pub session_id: Option<String>,
    pub result_preview: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeJsonOutput {
    pub session_id: Option<String>,
    pub result: Option<serde_json::Value>,
    pub is_error: Option<bool>,
    pub error: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
}

/// Parsed Claude CLI `--output-format json` envelope.
/// `result_text` is the assistant message, never the raw object.
#[derive(Debug, Clone, PartialEq)]
pub struct CliEnvelope {
    pub result_text: String,
    pub session_id: Option<String>,
    pub total_cost_usd: Option<f64>,
    pub is_error: bool,
}
