use crate::models::{
    AppSettings, BoardCard, BoardStatus, GuiSession, LibraryDoc, ProjectWorkspace, SessionSummary,
};
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

    fn workspace_index_path(&self) -> PathBuf {
        self.root.join("workspaces").join("index.json")
    }

    fn workspace_path(&self, id: Uuid) -> PathBuf {
        self.root.join("workspaces").join(format!("{id}.json"))
    }

    /// Board and library for one project folder. Missing folders start empty.
    pub fn load_workspace(&self, cwd: &str) -> Result<ProjectWorkspace, StoreError> {
        let key = workspace_key(cwd).ok_or_else(|| {
            StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project folder is empty",
            ))
        })?;
        let index = self.load_workspace_index()?;
        let Some(id) = index
            .entries
            .iter()
            .find(|entry| entry.cwd == key)
            .map(|entry| entry.id)
        else {
            return Ok(ProjectWorkspace::default());
        };
        let path = self.workspace_path(id);
        if !path.exists() {
            return Ok(ProjectWorkspace::default());
        }
        let raw = fs::read_to_string(path)?;
        let workspace = serde_json::from_str(&raw).unwrap_or_default();
        Ok(sanitize_workspace(workspace))
    }

    pub fn save_workspace(
        &self,
        cwd: &str,
        workspace: &ProjectWorkspace,
    ) -> Result<ProjectWorkspace, StoreError> {
        let key = workspace_key(cwd).ok_or_else(|| {
            StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "project folder is empty",
            ))
        })?;
        let clean = sanitize_workspace(workspace.clone());
        fs::create_dir_all(self.root.join("workspaces"))?;
        let mut index = self.load_workspace_index()?;
        let id = if let Some(entry) = index.entries.iter().find(|entry| entry.cwd == key) {
            entry.id
        } else {
            let id = Uuid::new_v4();
            index.entries.push(WorkspaceIndexEntry { cwd: key, id });
            self.save_workspace_index(&index)?;
            id
        };
        let raw = serde_json::to_string_pretty(&clean)?;
        fs::write(self.workspace_path(id), raw)?;
        Ok(clean)
    }

    fn load_workspace_index(&self) -> Result<WorkspaceIndex, StoreError> {
        let path = self.workspace_index_path();
        if !path.exists() {
            return Ok(WorkspaceIndex::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save_workspace_index(&self, index: &WorkspaceIndex) -> Result<(), StoreError> {
        fs::create_dir_all(self.root.join("workspaces"))?;
        let raw = serde_json::to_string_pretty(index)?;
        fs::write(self.workspace_index_path(), raw)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkspaceIndex {
    entries: Vec<WorkspaceIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceIndexEntry {
    cwd: String,
    id: Uuid,
}

fn workspace_key(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut key = trimmed.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    Some(key)
}

const MAX_CARDS: usize = 200;
const MAX_DOCS: usize = 100;
const MAX_TITLE: usize = 200;
const MAX_TAG: usize = 40;
const MAX_BODY: usize = 100_000;

fn sanitize_workspace(workspace: ProjectWorkspace) -> ProjectWorkspace {
    let mut next_number = workspace
        .cards
        .iter()
        .map(|card| card.number)
        .max()
        .unwrap_or(0);
    let mut cards = Vec::new();
    for card in workspace.cards.into_iter().take(MAX_CARDS) {
        let title = cut_chars(&card.title, MAX_TITLE);
        if title.trim().is_empty() {
            continue;
        }
        let mut number = card.number;
        if number == 0 {
            next_number = next_number.saturating_add(1);
            number = next_number;
        }
        cards.push(BoardCard {
            id: card.id,
            number,
            title,
            tag: cut_chars(card.tag.trim(), MAX_TAG),
            status: match card.status {
                BoardStatus::Done => BoardStatus::Done,
                BoardStatus::Progress => BoardStatus::Progress,
            },
        });
    }
    let mut docs = Vec::new();
    for doc in workspace.docs.into_iter().take(MAX_DOCS) {
        let title = cut_chars(&doc.title, MAX_TITLE);
        if title.trim().is_empty() {
            continue;
        }
        docs.push(LibraryDoc {
            id: doc.id,
            title,
            body: cut_chars(&doc.body, MAX_BODY),
        });
    }
    ProjectWorkspace { cards, docs }
}

fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> AppStore {
        let root = std::env::temp_dir().join(format!(
            "useupclaude-workspace-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        AppStore { root }
    }

    #[test]
    fn workspace_roundtrip_keeps_cards_and_docs_for_one_folder() {
        let store = scratch();
        let empty = store.load_workspace("/proj/app").unwrap();
        assert!(empty.cards.is_empty());
        assert!(empty.docs.is_empty());

        let saved = store
            .save_workspace(
                "/proj/app/",
                &ProjectWorkspace {
                    cards: vec![BoardCard {
                        id: Uuid::new_v4(),
                        number: 0,
                        title: "  Write the notes  ".into(),
                        tag: "docs".into(),
                        status: BoardStatus::Progress,
                    }],
                    docs: vec![LibraryDoc {
                        id: Uuid::new_v4(),
                        title: "Spec".into(),
                        body: "hello".into(),
                    }],
                },
            )
            .unwrap();
        assert_eq!(saved.cards.len(), 1);
        assert_eq!(saved.cards[0].number, 1);
        assert_eq!(saved.cards[0].title, "  Write the notes  ");
        assert_eq!(saved.cards[0].status, BoardStatus::Progress);
        assert_eq!(saved.docs[0].body, "hello");

        let loaded = store.load_workspace(r"\proj\app").unwrap();
        assert_eq!(loaded, saved);

        let other = store.load_workspace("/other").unwrap();
        assert!(other.cards.is_empty());
        let _ = fs::remove_dir_all(store.data_root());
    }

    #[test]
    fn blank_titles_are_dropped() {
        let store = scratch();
        let saved = store
            .save_workspace(
                "/proj",
                &ProjectWorkspace {
                    cards: vec![BoardCard {
                        id: Uuid::new_v4(),
                        number: 4,
                        title: "   ".into(),
                        tag: String::new(),
                        status: BoardStatus::Done,
                    }],
                    docs: vec![LibraryDoc {
                        id: Uuid::new_v4(),
                        title: " ".into(),
                        body: "gone".into(),
                    }],
                },
            )
            .unwrap();
        assert!(saved.cards.is_empty());
        assert!(saved.docs.is_empty());
        let _ = fs::remove_dir_all(store.data_root());
    }
}
