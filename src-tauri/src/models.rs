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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiSession {
    pub id: Uuid,
    pub cli_session_id: Option<String>,
    pub cwd: String,
    pub title: String,
    pub model: Option<String>,
    pub allowed_tools_preset: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub turns: Vec<GuiTurn>,
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
    pub allowed_tools_preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub session: GuiSession,
    pub assistant_turn: GuiTurn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub message: String,
    pub session_id: Option<String>,
    pub result_preview: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClaudeJsonOutput {
    pub session_id: Option<String>,
    pub result: Option<serde_json::Value>,
    pub is_error: Option<bool>,
    pub error: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
}
