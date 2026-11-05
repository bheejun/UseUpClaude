import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  ClaudeDetectResult,
  ConnectionTestResult,
  DiscoveredProject,
  GuiSession,
  KeyStatus,
  ProjectFileView,
  ModelCatalog,
  ProjectSkill,
  SendMessageResponse,
  SessionCostReport,
  SessionSummary,
} from "./types";

export const api = {
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<AppSettings>("save_settings", { settings }),
  getKeyStatus: () => invoke<KeyStatus>("get_key_status"),
  saveApiKey: (key: string) => invoke<KeyStatus>("save_api_key", { key }),
  deleteApiKey: () => invoke<KeyStatus>("delete_api_key"),
  sessionCosts: () => invoke<SessionCostReport>("session_costs"),
  listModels: () => invoke<ModelCatalog>("list_models"),
  listProjectSkills: (cwd: string) => invoke<ProjectSkill[]>("list_project_skills", { cwd }),
  setSessionModel: (id: string, model: string | null) =>
    invoke<GuiSession>("set_session_model", { id, model }),
  setSessionEffort: (id: string, effort: string | null) =>
    invoke<GuiSession>("set_session_effort", { id, effort }),
  setSessionFastMode: (id: string, fastMode: boolean) =>
    invoke<GuiSession>("set_session_fast_mode", { id, fastMode }),
  detectClaude: () => invoke<ClaudeDetectResult>("detect_claude"),
  listSessions: () => invoke<SessionSummary[]>("list_sessions"),
  getSession: (id: string) => invoke<GuiSession>("get_session", { id }),
  readProjectFile: (sessionId: string, path: string) =>
    invoke<ProjectFileView>("read_project_file", { sessionId, path }),
  deleteSession: (id: string) => invoke<void>("delete_session", { id }),
  createSession: (args: {
    cwd: string;
    model?: string | null;
    allowedToolsPreset?: string | null;
    cliSessionId?: string | null;
    title?: string | null;
  }) => invoke<GuiSession>("create_session", args),
  discoverClaudeSessions: () => invoke<DiscoveredProject[]>("discover_claude_sessions"),
  testConnection: () => invoke<ConnectionTestResult>("test_connection"),
  sendMessage: (request: {
    sessionId?: string | null;
    prompt: string;
    cwd?: string | null;
    model?: string | null;
    effort?: string | null;
    fastMode?: boolean;
    allowedToolsPreset?: string | null;
    skill?: string | null;
  }) => invoke<SendMessageResponse>("send_message", { request }),
  cancelSession: (id: string) => invoke<void>("cancel_session", { id }),
  dataDir: () => invoke<string>("data_dir"),
};
