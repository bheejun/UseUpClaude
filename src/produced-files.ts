import type { TurnEvent } from "@/types";

const FILE_CHANGE_TOOLS = new Set(["write", "edit", "multiedit", "notebookedit"]);

const PATH_KEYS = ["file_path", "filePath", "notebook_path", "notebookPath", "path"] as const;

function isFileChangeTool(name: string): boolean {
  return FILE_CHANGE_TOOLS.has(name.trim().toLowerCase());
}

function pathFromPreview(preview: string): string | null {
  const trimmed = preview.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    return pathFromRecord(parsed);
  } catch {
    const match = trimmed.match(
      /"(?:file_path|filePath|notebook_path|notebookPath|path)"\s*:\s*"((?:\\.|[^"\\])*)"/,
    );
    if (!match) return null;
    try {
      const value = JSON.parse(`"${match[1]}"`) as unknown;
      return typeof value === "string" && value.trim() ? value.trim() : null;
    } catch {
      return null;
    }
  }
}

function pathFromRecord(value: unknown): string | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const obj = value as Record<string, unknown>;
  for (const key of PATH_KEYS) {
    const found = obj[key];
    if (typeof found === "string" && found.trim()) return found.trim();
  }
  return null;
}

/** Path kept on the event, or still visible inside an older capped preview. */
export function eventFilePath(event: TurnEvent): string | null {
  const direct = event.path?.trim();
  if (direct) return direct;
  if (!isFileChangeTool(event.name)) return null;
  return pathFromPreview(event.preview);
}

function slash(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+$/, "");
}

export function projectRelativePath(path: string, cwd: string): string {
  const target = slash(path);
  const base = slash(cwd);
  if (target === base) return target;
  const prefix = `${base}/`;
  if (target.startsWith(prefix)) return target.slice(prefix.length);
  return path;
}

/** Same file when one path is absolute and the other is relative to `cwd`. */
export function sameProjectFile(path: string, other: string, cwd: string): boolean {
  const rel = (value: string) => {
    const full = slash(value).toLowerCase();
    const base = slash(cwd).toLowerCase();
    if (!base) return full;
    if (full === base) return "";
    const prefix = `${base}/`;
    return full.startsWith(prefix) ? full.slice(prefix.length) : full;
  };
  const left = rel(path);
  const right = rel(other);
  return left.length > 0 && left === right;
}

/** Paths a turn wrote or edited, in first-seen order. Failed calls are left out. */
export function producedFilePaths(events: TurnEvent[]): string[] {
  const errorIds = new Set<string>();
  for (const event of events) {
    if (event.kind === "tool_result" && event.isError && event.toolUseId) {
      errorIds.add(event.toolUseId);
    }
  }
  const seen = new Set<string>();
  const paths: string[] = [];
  for (const event of events) {
    if (event.kind !== "tool_use" || !isFileChangeTool(event.name)) continue;
    if (event.toolUseId && errorIds.has(event.toolUseId)) continue;
    const path = eventFilePath(event);
    if (!path) continue;
    const key = path.replace(/\\/g, "/");
    if (seen.has(key)) continue;
    seen.add(key);
    paths.push(path);
  }
  return paths;
}
