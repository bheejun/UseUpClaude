import { Bot, LoaderCircle, User } from "lucide-react";
import { useEffect, useRef } from "react";
import { MarkdownMessage } from "@/components/markdown-message";
import { formatUsd } from "@/chat-format";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type { TurnRole } from "@/types";

export type Bubble = {
  id: string;
  role: TurnRole;
  text: string;
  costUsd: number | null;
};

export function ChatThread({
  bubbles,
  thinking,
}: {
  bubbles: Bubble[];
  thinking: boolean;
}) {
  const { t } = useI18n();
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [bubbles, thinking]);

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 px-4 py-6">
      {bubbles.map((bubble) => {
        const mine = bubble.role === "user";
        const label =
          bubble.role === "user"
            ? t("you")
            : bubble.role === "error"
              ? t("roleError")
              : t("assistantName");
        return (
          <article key={bubble.id} className={cn("flex gap-3", mine && "flex-row-reverse")}>
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
              <div
                className={cn(
                  "rounded-md border px-3 py-2 text-left text-[14px] leading-relaxed",
                  mine && "whitespace-pre-wrap border-border bg-secondary",
                  bubble.role === "assistant" && "border-border bg-card",
                  bubble.role === "error" && "border-destructive/40 bg-destructive/10 text-red-100",
                  bubble.role === "system" && "border-border bg-card text-muted-foreground",
                )}
              >
                {mine || bubble.role === "system" ? (
                  bubble.text
                ) : (
                  <MarkdownMessage text={bubble.text} />
                )}
              </div>
              {bubble.costUsd != null && (
                <p className="mt-1 font-mono text-[10px] text-muted-foreground">
                  {t("thisTurn", { cost: formatUsd(bubble.costUsd) })}
                </p>
              )}
            </div>
          </article>
        );
      })}
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
