import { Bot, Check, Copy, LoaderCircle, User } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { MarkdownMessage } from "@/components/markdown-message";
import { formatUsd } from "@/chat-format";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type { TurnEvent, TurnRole } from "@/types";

export type Bubble = {
  id: string;
  role: TurnRole;
  text: string;
  costUsd: number | null;
  finishedAt?: string | null;
  events?: TurnEvent[];
};

type ToolRow = {
  key: string;
  name: string;
  preview: string | null;
  isError: boolean;
};

function toolRows(events: TurnEvent[]): ToolRow[] {
  const rows: ToolRow[] = [];
  const byId = new Map<string, number>();
  events.forEach((event, index) => {
    if (event.kind === "tool_use") {
      if (event.toolUseId) byId.set(event.toolUseId, rows.length);
      rows.push({
        key: `${event.toolUseId || "use"}-${index}`,
        name: event.name,
        preview: null,
        isError: false,
      });
      return;
    }
    const slot = event.toolUseId ? byId.get(event.toolUseId) : undefined;
    if (slot != null && rows[slot].preview == null) {
      rows[slot] = { ...rows[slot], preview: event.preview, isError: event.isError };
      return;
    }
    rows.push({
      key: `${event.toolUseId || "result"}-${index}`,
      name: event.name,
      preview: event.preview,
      isError: event.isError,
    });
  });
  return rows;
}

function ToolEvents({ events }: { events: TurnEvent[] }) {
  const { t } = useI18n();
  const rows = toolRows(events);
  if (rows.length === 0) return null;
  return (
    <div className="mb-2 flex flex-col gap-1.5" role="group" aria-label={t("toolEvent")}>
      <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">{t("toolEvent")}</p>
      {rows.map((row) => (
        <div key={row.key} className="rounded-md border border-border bg-card px-3 py-2 text-left">
          <p className="font-mono text-[12px] text-foreground">{row.name}</p>
          {row.preview ? (
            <pre
              className={cn(
                "m-0 mt-1 whitespace-pre-wrap break-all font-mono text-[12px]",
                row.isError ? "text-red-100" : "text-muted-foreground",
              )}
            >
              {row.preview}
            </pre>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function relativeLabel(
  iso: string | null | undefined,
  now: number,
  t: (key: "justNow" | "minutesAgo" | "hoursAgo" | "daysAgo", vars?: Record<string, string | number>) => string,
): string | null {
  if (!iso) return null;
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return null;
  const minutes = Math.floor((now - then) / 60_000);
  if (minutes < 1) return t("justNow");
  if (minutes < 60) return t("minutesAgo", { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("hoursAgo", { n: hours });
  return t("daysAgo", { n: Math.floor(hours / 24) });
}

function ChatBubble({ bubble, now }: { bubble: Bubble; now: number }) {
  const { t } = useI18n();
  const bodyRef = useRef<HTMLDivElement>(null);
  const mine = bubble.role === "user";
  const events = bubble.events ?? [];
  const showBody = mine || bubble.role === "system" || bubble.text.trim().length > 0;
  const label =
    bubble.role === "user" ? t("you") : bubble.role === "error" ? t("roleError") : t("assistantName");

  return (
    <article className={cn("flex gap-3", mine && "flex-row-reverse")}>
      <div
        className={cn(
          "mt-0.5 grid size-7 shrink-0 place-items-center rounded-md border border-border",
          mine ? "bg-secondary text-foreground" : "bg-[#1a2218] text-primary",
        )}
        aria-hidden
      >
        {mine ? <User className="size-3.5" /> : <Bot className="size-3.5" />}
      </div>
      <div className={cn("min-w-0 max-w-[min(40rem,calc(100%-2.5rem))]", mine && "text-right")}>
        <div className="mb-1 font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
          {label}
        </div>
        {events.length > 0 && <ToolEvents events={events} />}
        {showBody && (
          <div
            ref={bodyRef}
            className={cn(
              "rounded-md border px-3 py-2 text-left text-[14px] leading-relaxed",
              mine && "whitespace-pre-wrap border-border bg-secondary",
              bubble.role === "assistant" && "border-border bg-card",
              bubble.role === "error" && "border-destructive/40 bg-destructive/10 text-red-100",
              bubble.role === "system" && "border-border bg-card text-muted-foreground",
            )}
          >
            {mine || bubble.role === "system" ? bubble.text : <MarkdownMessage text={bubble.text} />}
          </div>
        )}
        {bubble.role === "assistant" && bubble.text.trim() && (
          <AssistantActions
            now={now}
            finishedAt={bubble.finishedAt}
            getText={() => bodyRef.current?.innerText?.trim() || bubble.text.trim()}
          />
        )}
        {bubble.costUsd != null && (
          <p className="mt-1 font-mono text-[10px] text-muted-foreground">
            {t("thisTurn", { cost: formatUsd(bubble.costUsd) })}
          </p>
        )}
      </div>
    </article>
  );
}

function AssistantActions({
  getText,
  finishedAt,
  now,
}: {
  getText: () => string;
  finishedAt?: string | null;
  now: number;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  const when = relativeLabel(finishedAt, now, t);
  const label = failed ? t("copyMessageFailed") : copied ? t("copiedMessage") : t("copyMessage");

  return (
    <div className="mt-1 flex items-center gap-1.5">
      <button
        type="button"
        className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label={label}
        title={label}
        onClick={() => {
          const rendered = getText().trim();
          if (!rendered) return;
          void navigator.clipboard
            .writeText(rendered)
            .then(() => {
              setFailed(false);
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            })
            .catch(() => {
              setCopied(false);
              setFailed(true);
            });
        }}
      >
        {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
      </button>
      {when && <span className="font-mono text-[10px] text-muted-foreground">{when}</span>}
    </div>
  );
}

export function ChatThread({
  bubbles,
  thinking,
}: {
  bubbles: Bubble[];
  thinking: boolean;
}) {
  const { t } = useI18n();
  const endRef = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [bubbles, thinking]);

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, []);

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 px-4 py-6">
      {bubbles.map((bubble) => (
        <ChatBubble key={bubble.id} bubble={bubble} now={now} />
      ))}
      {thinking && (
        <article className="flex gap-3">
          <div className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-md border border-border bg-[#1a2218] text-primary">
            <LoaderCircle className="size-3.5 animate-spin" />
          </div>
          <div className="min-w-0">
            <div className="mb-1 font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
              {t("assistantName")}
            </div>
            <div className="rounded-md border border-dashed border-primary/40 bg-card px-3 py-2">
              <p className="text-[14px] text-primary">{t("working")}</p>
              <p className="mt-1 font-mono text-[11px] text-muted-foreground">{t("workingCommand")}</p>
            </div>
          </div>
        </article>
      )}
      <div ref={endRef} />
    </div>
  );
}
