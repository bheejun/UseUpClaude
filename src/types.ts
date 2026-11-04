export type TurnRole = "user" | "assistant" | "system" | "error";
export type TurnStatus = "pending" | "done" | "failed" | "cancelled";

export interface AppSettings {
  claudeBinaryPath: string | null;
  defaultCwd: string | null;
  model: string | null;
  timeoutSecs: number;
  allowedToolsPreset: string;
}

export interface GuiTurn {
  id: string;
  role: TurnRole;
  content: string;
  exitCode: number | null;
  startedAt: string;
  finishedAt: string | null;
  status: TurnStatus;
  /** This turn's CLI total_cost_usd, when reported. */
  costUsd?: number | null;
}

export interface GuiSession {
  id: string;
  cliSessionId: string | null;
  cwd: string;
  title: string;
  model: string | null;
  allowedToolsPreset: string;
  createdAt: string;
  updatedAt: string;
  turns: GuiTurn[];
}

export interface SessionSummary {
  id: string;
  cliSessionId: string | null;
  cwd: string;
  title: string;
  model: string | null;
  allowedToolsPreset: string;
  createdAt: string;
  updatedAt: string;
  turnCount: number;
}

export interface ClaudeDetectResult {
  found: boolean;
  path: string | null;
  version: string | null;
  error: string | null;
}

export interface KeyStatus {
  configured: boolean;
  masked: string | null;
}

export interface ConnectionTestResult {
  ok: boolean;
  message: string;
  sessionId: string | null;
  resultPreview: string | null;
}

export interface SendMessageResponse {
  session: GuiSession;
  assistantTurn: GuiTurn;
}

export interface DiscoveredSession {
  sessionId: string;
  title: string;
  modifiedMs: number;
}

export interface DiscoveredProject {
  cwd: string | null;
  label: string;
  sessions: DiscoveredSession[];
}
