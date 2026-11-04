import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  ClaudeDetectResult,
  ConnectionTestResult,
  DiscoveredProject,
  GuiSession,
  KeyStatus,
  SendMessageResponse,
  SessionCostReport,
  SessionSummary,
  WorkspaceSpend,
} from "./types";

export const api = {
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<AppSettings>("save_settings", { settings }),
  getKeyStatus: () => invoke<KeyStatus>("get_key_status"),
  saveApiKey: (key: string) => invoke<KeyStatus>("save_api_key", { key }),
  deleteApiKey: () => invoke<KeyStatus>("delete_api_key"),
  getAdminKeyStatus: () => invoke<KeyStatus>("get_admin_key_status"),
  saveAdminApiKey: (key: string) => invoke<KeyStatus>("save_admin_api_key", { key }),
  deleteAdminApiKey: () => invoke<KeyStatus>("delete_admin_api_key"),
  getWorkspaceSpend: () => invoke<WorkspaceSpend>("get_workspace_spend"),
  sessionCosts: () => invoke<SessionCostReport>("session_costs"),
  detectClaude: () => invoke<ClaudeDetectResult>("detect_claude"),
  listSessions: () => invoke<SessionSummary[]>("list_sessions"),
  getSession: (id: string) => invoke<GuiSession>("get_session", { id }),
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
    allowedToolsPreset?: string | null;
  }) => invoke<SendMessageResponse>("send_message", { request }),
  cancelActive: () => invoke<void>("cancel_active"),
  dataDir: () => invoke<string>("data_dir"),
};
