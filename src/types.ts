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
  /** This turn's own cost in USD. Not the CLI session running total. */
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

export interface ListedModel {
  id: string;
  displayName: string;
}

export interface ProjectSkill {
  name: string;
  description: string;
}

export interface ModelCatalog {
  models: ListedModel[];
  errorCode: "no_key" | "unauthorized" | "failed" | null;
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

export interface SessionCostLine {
  id: string;
  title: string;
  cwd: string;
  updatedAt: string;
  costUsd: number | null;
}

export interface SessionCostReport {
  totalUsd: number | null;
  sessions: SessionCostLine[];
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
