export function formatUsd(n: number): string {
  const abs = Math.abs(n);
  const digits = abs > 0 && abs < 0.0001 ? 6 : 4;
  return `$${n.toFixed(digits)}`;
}

function messageFromField(value: unknown): string {
  if (typeof value === "string") return value;
  if (value == null) return "";
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
}

function envelopeFromValue(
  value: unknown,
): { result: string; costUsd: number | null } | null {
  if (Array.isArray(value)) {
    for (let i = value.length - 1; i >= 0; i -= 1) {
      const found = envelopeFromValue(value[i]);
      if (found) return found;
    }
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const obj = value as Record<string, unknown>;
  const typeResult = obj.type === "result";
  const marker =
    typeof obj.session_id === "string" ||
    "total_cost_usd" in obj ||
    "usage" in obj ||
    typeResult;
  if (!marker) return null;
  if (!("result" in obj) && !("error" in obj) && !("is_error" in obj) && !typeResult) {
    return null;
  }
  const result = messageFromField(obj.result);
  const error = messageFromField(obj.error);
  const text =
    result ||
    error ||
    (obj.is_error === true ? "Claude returned an error." : "(empty response)");
  const costUsd = typeof obj.total_cost_usd === "number" ? obj.total_cost_usd : null;
  return { result: text, costUsd };
}

/** Show `result` when an older transcript stored the raw CLI envelope. */
export function presentTurn(turn: { content: string; costUsd?: number | null }): {
  text: string;
  costUsd: number | null;
} {
  const trimmed = turn.content.trim();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    try {
      const extracted = envelopeFromValue(JSON.parse(trimmed) as unknown);
      if (extracted) {
        return {
          text: extracted.result,
          costUsd: turn.costUsd ?? extracted.costUsd,
        };
      }
    } catch {
      // Not a CLI envelope. Show the stored text.
    }
  }
  return { text: turn.content, costUsd: turn.costUsd ?? null };
}

export function sumSessionCost(
  turns: { content: string; costUsd?: number | null }[] | undefined,
): number | null {
  if (!turns) return null;
  let sum = 0;
  let any = false;
  for (const turn of turns) {
    const cost = presentTurn(turn).costUsd;
    if (typeof cost === "number" && Number.isFinite(cost)) {
      sum += cost;
      any = true;
    }
  }
  return any ? sum : null;
}

/** Readable transcript. CLI JSON envelopes become the result text. */
export function conversationText(
  session: {
    title: string;
    cwd: string;
    turns: { role: string; content: string; costUsd?: number | null }[];
  },
  labels: { user: string; assistant: string; error: string },
): string {
  const parts: string[] = [session.title.trim() || "Chat"];
  const cwd = session.cwd.trim();
  if (cwd) parts.push(cwd);
  for (const turn of session.turns) {
    const text = presentTurn(turn).text.trim();
    if (!text) continue;
    const label =
      turn.role === "user" ? labels.user : turn.role === "error" ? labels.error : labels.assistant;
    parts.push("", label, text);
  }
  return `${parts.join("\n")}\n`;
}

export function folderBase(cwd: string): string {
  const parts = cwd.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || cwd || "—";
}
