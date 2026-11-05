use crate::models::{
    AppSettings, BoardCard, BoardStatus, GuiSession, ProjectWorkspace, SessionSummary,
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

    /// Board for one project folder. Library files are the folder itself.
    /// A leftover document list from older builds is deleted on load.
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
        let raw = fs::read_to_string(&path)?;
        let mut value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => return Ok(ProjectWorkspace::default()),
        };
        let had_docs = value
            .as_object_mut()
            .is_some_and(|obj| obj.remove("docs").is_some());
        let workspace: ProjectWorkspace = serde_json::from_value(value).unwrap_or_default();
        let clean = sanitize_workspace(workspace);
        if had_docs {
            let encoded = serde_json::to_string_pretty(&clean)?;
            fs::write(&path, encoded)?;
        }
        Ok(clean)
    }

    /// Create or update the card for one chat turn. The same folder shares one board.
    pub fn upsert_turn_card(
        &self,
        cwd: &str,
        turn_id: Uuid,
        title: &str,
        phase: TurnCardPhase,
    ) -> Result<ProjectWorkspace, StoreError> {
        let mut workspace = self.load_workspace(cwd)?;
        if let Some(card) = workspace
            .cards
            .iter_mut()
            .find(|card| card.turn_id == Some(turn_id))
        {
            match phase {
                TurnCardPhase::Progress => {
                    card.status = BoardStatus::Progress;
                    card.failed = false;
                }
                TurnCardPhase::Done => {
                    card.status = BoardStatus::Done;
                    card.failed = false;
                }
                TurnCardPhase::Failed => {
                    card.status = BoardStatus::Progress;
                    card.failed = true;
                }
            }
        } else {
            let (status, failed) = match phase {
                TurnCardPhase::Progress => (BoardStatus::Progress, false),
                TurnCardPhase::Done => (BoardStatus::Done, false),
                TurnCardPhase::Failed => (BoardStatus::Progress, true),
            };
            let trimmed = title.trim();
            let title = if trimmed.is_empty() { "…" } else { trimmed };
            workspace.cards.insert(
                0,
                BoardCard {
                    id: Uuid::new_v4(),
                    number: 0,
                    title: title.to_string(),
                    tag: String::new(),
                    status,
                    failed,
                    turn_id: Some(turn_id),
                },
            );
        }
        self.save_workspace(cwd, &workspace)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCardPhase {
    Progress,
    Done,
    Failed,
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
const MAX_TITLE: usize = 200;
const MAX_TAG: usize = 40;

fn sanitize_workspace(workspace: ProjectWorkspace) -> ProjectWorkspace {
    let mut next_number = workspace
        .cards
        .iter()
        .map(|card| card.number)
        .max()
        .unwrap_or(0);
    let mut seen_turns = Vec::new();
    let mut cards = Vec::new();
    for card in workspace.cards.into_iter().take(MAX_CARDS) {
        let title = cut_chars(&card.title, MAX_TITLE);
        if title.trim().is_empty() {
            continue;
        }
        if let Some(turn_id) = card.turn_id {
            if seen_turns.contains(&turn_id) {
                continue;
            }
            seen_turns.push(turn_id);
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
            failed: card.failed,
            turn_id: card.turn_id,
        });
    }
    ProjectWorkspace { cards }
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

    fn card(title: &str, status: BoardStatus) -> BoardCard {
        BoardCard {
            id: Uuid::new_v4(),
            number: 0,
            title: title.into(),
            tag: String::new(),
            status,
            failed: false,
            turn_id: None,
        }
    }

    #[test]
    fn workspace_roundtrip_keeps_cards_for_one_folder() {
        let store = scratch();
        let empty = store.load_workspace("/proj/app").unwrap();
        assert!(empty.cards.is_empty());

        let saved = store
            .save_workspace(
                "/proj/app/",
                &ProjectWorkspace {
                    cards: vec![card("  Write the notes  ", BoardStatus::Progress)],
                },
            )
            .unwrap();
        assert_eq!(saved.cards.len(), 1);
        assert_eq!(saved.cards[0].number, 1);
        assert_eq!(saved.cards[0].title, "  Write the notes  ");
        assert_eq!(saved.cards[0].status, BoardStatus::Progress);
        assert!(!saved.cards[0].failed);
        assert!(saved.cards[0].turn_id.is_none());

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
                    cards: vec![card("   ", BoardStatus::Done)],
                },
            )
            .unwrap();
        assert!(saved.cards.is_empty());
        let _ = fs::remove_dir_all(store.data_root());
    }

    #[test]
    fn stored_documents_are_removed_from_the_board_file() {
        let store = scratch();
        store
            .save_workspace(
                "/proj",
                &ProjectWorkspace {
                    cards: vec![card("Keep", BoardStatus::Progress)],
                },
            )
            .unwrap();
        let index = store.load_workspace_index().unwrap();
        let path = store.workspace_path(index.entries[0].id);
        let mut raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        raw["docs"] = serde_json::json!([{
            "id": "11111111-1111-1111-1111-111111111111",
            "title": "Spec",
            "body": "secret body that must not stay"
        }]);
        fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

        let loaded = store.load_workspace("/proj").unwrap();
        assert_eq!(loaded.cards.len(), 1);
        assert_eq!(loaded.cards[0].title, "Keep");
        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("secret body"));
        assert!(!after.contains("\"docs\""));
        let _ = fs::remove_dir_all(store.data_root());
    }

    #[test]
    fn turn_card_updates_in_place_on_the_shared_folder() {
        let store = scratch();
        let turn = Uuid::new_v4();
        let progress = store
            .upsert_turn_card(
                "/proj/app/",
                turn,
                "Fix the board",
                TurnCardPhase::Progress,
            )
            .unwrap();
        assert_eq!(progress.cards.len(), 1);
        assert_eq!(progress.cards[0].status, BoardStatus::Progress);
        assert!(!progress.cards[0].failed);
        assert_eq!(progress.cards[0].number, 1);
        assert_eq!(progress.cards[0].turn_id, Some(turn));
        assert_eq!(progress.cards[0].title, "Fix the board");

        let again = store
            .upsert_turn_card(r"\proj\app", turn, "Other title", TurnCardPhase::Progress)
            .unwrap();
        assert_eq!(again.cards.len(), 1);
        assert_eq!(again.cards[0].id, progress.cards[0].id);
        assert_eq!(again.cards[0].title, "Fix the board");

        let failed = store
            .upsert_turn_card("/proj/app", turn, "ignored", TurnCardPhase::Failed)
            .unwrap();
        assert_eq!(failed.cards[0].status, BoardStatus::Progress);
        assert!(failed.cards[0].failed);
        assert_eq!(failed.cards[0].title, "Fix the board");

        let done = store
            .upsert_turn_card("/proj/app", turn, "ignored", TurnCardPhase::Done)
            .unwrap();
        assert_eq!(done.cards[0].status, BoardStatus::Done);
        assert!(!done.cards[0].failed);
        assert_eq!(done.cards[0].id, progress.cards[0].id);

        let other = Uuid::new_v4();
        let two = store
            .upsert_turn_card("/proj/app", other, "Second turn", TurnCardPhase::Failed)
            .unwrap();
        assert_eq!(two.cards.len(), 2);
        let second = two
            .cards
            .iter()
            .find(|card| card.turn_id == Some(other))
            .unwrap();
        assert_eq!(second.status, BoardStatus::Progress);
        assert!(second.failed);
        assert!(store.load_workspace("/other").unwrap().cards.is_empty());
        let _ = fs::remove_dir_all(store.data_root());
    }
}
