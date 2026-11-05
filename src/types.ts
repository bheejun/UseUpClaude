export type TurnRole = "user" | "assistant" | "system" | "error";
export type TurnStatus = "pending" | "done" | "failed" | "cancelled";
export type TurnEventKind =
  | "tool_use"
  | "tool_result"
  | "task_started"
  | "task_progress"
  | "task_updated"
  | "task_notification"
  | "background_tasks_changed";

export interface TurnEvent {
  kind: TurnEventKind;
  toolUseId: string;
  name: string;
  isError: boolean;
  preview: string;
  /** Set when a Write or Edit path would otherwise be missing from the stored event. */
  path?: string | null;
}

export type ProjectFileKind = "markdown" | "text" | "unsupported";

export interface ProjectFileView {
  path: string;
  kind: ProjectFileKind;
  content: string | null;
  truncated: boolean;
}

export interface ProjectDirEntry {
  name: string;
  path: string;
  isDir: boolean;
}

export interface ProjectDirListing {
  path: string;
  entries: ProjectDirEntry[];
  truncated: boolean;
}

export type BoardStatus = "progress" | "done";

export interface BoardCard {
  id: string;
  number: number;
  title: string;
  tag: string;
  status: BoardStatus;
}

export interface LibraryDoc {
  id: string;
  title: string;
  body: string;
}

export interface ProjectWorkspace {
  cards: BoardCard[];
  docs: LibraryDoc[];
}

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
  /** Completed tool calls from this turn. `content` stays the result text. */
  events?: TurnEvent[];
}

export interface GuiSession {
  id: string;
  cliSessionId: string | null;
  cwd: string;
  title: string;
  model: string | null;
  /** Reasoning effort for this chat. Null omits `--effort`. */
  effort: string | null;
  /** Fast mode for this chat. False omits `--settings`. */
  fastMode: boolean;
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
