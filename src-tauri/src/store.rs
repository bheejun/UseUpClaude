use crate::models::{AppSettings, GuiSession, SessionSummary};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session not found: {0}")]
    NotFound(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionIndex {
    sessions: Vec<Uuid>,
}

pub struct AppStore {
    root: PathBuf,
}

impl AppStore {
    pub fn open() -> Result<Self, StoreError> {
        let base = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-gui");
        let sessions = base.join("sessions");
        fs::create_dir_all(&sessions)?;
        Ok(Self { root: base })
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("sessions").join("index.json")
    }

    fn session_path(&self, id: Uuid) -> PathBuf {
        self.root.join("sessions").join(format!("{id}.json"))
    }

    pub fn load_settings(&self) -> Result<AppSettings, StoreError> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        let raw = fs::read_to_string(&path)?;
        let mut value: serde_json::Value = serde_json::from_str(&raw)?;
        let stripped = value
            .as_object_mut()
            .is_some_and(|obj| obj.remove("workspaceId").is_some());
        let settings = serde_json::from_value(value)?;
        if stripped {
            self.save_settings(&settings)?;
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StoreError> {
        let raw = serde_json::to_string_pretty(settings)?;
        fs::write(self.settings_path(), raw)?;
        Ok(())
    }

    fn load_index(&self) -> Result<SessionIndex, StoreError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(SessionIndex::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save_index(&self, index: &SessionIndex) -> Result<(), StoreError> {
        let raw = serde_json::to_string_pretty(index)?;
        fs::write(self.index_path(), raw)?;
        Ok(())
    }

    pub fn load_sessions(&self) -> Result<Vec<GuiSession>, StoreError> {
        let index = self.load_index()?;
        let mut out = Vec::new();
        for id in index.sessions {
            if let Ok(session) = self.get_session(id) {
                out.push(session);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>, StoreError> {
        let index = self.load_index()?;
        let mut out = Vec::new();
        for id in index.sessions {
            if let Ok(session) = self.get_session(id) {
                out.push(SessionSummary::from(&session));
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn get_session(&self, id: Uuid) -> Result<GuiSession, StoreError> {
        let path = self.session_path(id);
        if !path.exists() {
            return Err(StoreError::NotFound(id));
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_session(&self, session: &GuiSession) -> Result<(), StoreError> {
        let path = self.session_path(session.id);
        let raw = serde_json::to_string_pretty(session)?;
        fs::write(path, raw)?;

        let mut index = self.load_index()?;
        if !index.sessions.contains(&session.id) {
            index.sessions.insert(0, session.id);
            self.save_index(&index)?;
        }
        Ok(())
    }

    pub fn delete_session(&self, id: Uuid) -> Result<(), StoreError> {
        let path = self.session_path(id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        let mut index = self.load_index()?;
        index.sessions.retain(|x| *x != id);
        self.save_index(&index)?;
        Ok(())
    }

    pub fn data_root(&self) -> &Path {
        &self.root
    }
}
