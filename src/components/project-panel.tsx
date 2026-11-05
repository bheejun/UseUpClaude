import { listen } from "@tauri-apps/api/event";
import { Kanban, X } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { api } from "@/api";
import { LibraryView } from "@/components/library-view";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { folderKey } from "@/produced-files";
import type { BoardCard, BoardStatus, ProjectWorkspace } from "@/types";

const EMPTY: ProjectWorkspace = { cards: [] };

export function ProjectPanel({
  sessionId,
  cwd,
  section,
  onSection,
  activePath,
  onActivePath,
  onClose,
}: {
  sessionId: string;
  cwd: string;
  section: "board" | "library";
  onSection: (section: "board" | "library") => void;
  activePath: string | null;
  onActivePath: (path: string | null) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [workspace, setWorkspace] = useState<ProjectWorkspace>(EMPTY);
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const [loading, setLoading] = useState(true);
  const [saveFailed, setSaveFailed] = useState(false);
  const [title, setTitle] = useState("");
  const [tag, setTag] = useState("");
  const saveSeq = useRef(0);
  const saveChain = useRef(Promise.resolve());
  const reloadQueued = useRef(false);
  const [width, setWidth] = useState(672);
  const widthRef = useRef(width);
  widthRef.current = width;

  const apply = (next: ProjectWorkspace) => {
    workspaceRef.current = next;
    setWorkspace(next);
  };

  const reload = () => {
    const seq = saveSeq.current;
    void api
      .getProjectWorkspace(cwd)
      .then((next) => {
        if (saveSeq.current !== seq) {
          reloadQueued.current = true;
          return;
        }
        apply(next);
        setSaveFailed(false);
      })
      .catch(() => setSaveFailed(true));
  };
  const reloadRef = useRef(reload);
  reloadRef.current = reload;

  const save = (next: ProjectWorkspace) => {
    const seq = ++saveSeq.current;
    saveChain.current = saveChain.current
      .catch(() => undefined)
      .then(async () => {
        try {
          const saved = await api.saveProjectWorkspace(cwd, next);
          if (saveSeq.current !== seq) return;
          apply(saved);
          setSaveFailed(false);
          if (reloadQueued.current) {
            reloadQueued.current = false;
            reloadRef.current();
          }
        } catch {
          if (saveSeq.current === seq) setSaveFailed(true);
        }
      });
  };

  const commit = (next: ProjectWorkspace) => {
    apply(next);
    save(next);
  };

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSaveFailed(false);
    void api
      .getProjectWorkspace(cwd)
      .then((next) => {
        if (cancelled) return;
        apply(next);
      })
      .catch(() => {
        if (!cancelled) setSaveFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [cwd]);

  useEffect(() => {
    const key = folderKey(cwd);
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<{ cwd: string }>("project-workspace", (event) => {
      if (folderKey(event.payload.cwd) !== key) return;
      reloadRef.current();
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [cwd]);

  const addCard = () => {
    const nextTitle = title.trim();
    if (!nextTitle) return;
    const card: BoardCard = {
      id: crypto.randomUUID(),
      number: 0,
      title: nextTitle,
      tag: tag.trim(),
      status: "progress",
      failed: false,
    };
    commit({ cards: [card, ...workspaceRef.current.cards] });
    setTitle("");
    setTag("");
  };

  const moveCard = (id: string, status: BoardStatus) => {
    commit({
      cards: workspaceRef.current.cards.map((card) =>
        card.id === id ? { ...card, status, failed: status === "done" ? false : card.failed } : card,
      ),
    });
  };

  const deleteCard = (id: string) => {
    commit({ cards: workspaceRef.current.cards.filter((card) => card.id !== id) });
  };

  const clampWidth = (next: number) => {
    const max = Math.max(420, Math.round(window.innerWidth * 0.72));
    return Math.min(max, Math.max(420, next));
  };

  const onResizePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (window.innerWidth < 821) return;
    event.preventDefault();
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startW = widthRef.current;
    const move = (ev: PointerEvent) => {
      setWidth(clampWidth(startW - (ev.clientX - startX)));
    };
    const up = (ev: PointerEvent) => {
      if (handle.hasPointerCapture(ev.pointerId)) handle.releasePointerCapture(ev.pointerId);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  };

  return (
    <aside
      className="project-panel absolute inset-y-0 right-0 z-30 flex shrink-0 flex-col border-l border-border bg-[#101318] shadow-xl min-[821px]:relative min-[821px]:inset-auto min-[821px]:z-auto min-[821px]:shadow-none"
      style={{ "--project-panel-width": `${width}px` } as CSSProperties}
      aria-label={t("projectBoard")}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t("resizeFilePanel")}
        aria-valuenow={width}
        tabIndex={0}
        className="absolute inset-y-0 left-0 z-10 hidden w-1.5 -translate-x-1/2 cursor-col-resize min-[821px]:block hover:bg-primary/50"
        onPointerDown={onResizePointerDown}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft") setWidth((current) => clampWidth(current + 24));
          if (event.key === "ArrowRight") setWidth((current) => clampWidth(current - 24));
        }}
      />
      <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
        <h3 className="flex min-w-0 items-center gap-1.5 font-mono text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
          <Kanban className="size-3.5 shrink-0" />
          <span className="truncate">{section === "board" ? t("projectBoard") : t("projectLibrary")}</span>
        </h3>
        <button
          type="button"
          className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={t("closeProjectBoard")}
          onClick={onClose}
        >
          <X className="size-3.5" />
        </button>
      </div>
      <div role="tablist" aria-label={t("projectBoard")} className="flex border-b border-border px-2">
        {(
          [
            ["board", t("projectBoard")],
            ["library", t("projectLibrary")],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`project-tab-${id}`}
            aria-selected={section === id}
            aria-controls={`project-panel-${id}`}
            className={cn(
              "border-b-2 px-3 py-2 text-[13px]",
              section === id
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
            onClick={() => onSection(id)}
          >
            {label}
          </button>
        ))}
      </div>
      {saveFailed && section === "board" && (
        <p className="border-b border-border px-3 py-2 text-[12px] text-destructive" role="alert">
          {t("workspaceSaveFailed")}
        </p>
      )}
      {section === "board" ? (
        <div
          role="tabpanel"
          id="project-panel-board"
          aria-labelledby="project-tab-board"
          className="flex min-h-0 flex-1 flex-col"
        >
          <form
            className="flex flex-col gap-2 border-b border-border px-3 py-2"
            onSubmit={(event) => {
              event.preventDefault();
              addCard();
            }}
          >
            <Input
              value={title}
              placeholder={t("cardTitle")}
              aria-label={t("cardTitle")}
              onChange={(event) => setTitle(event.target.value)}
            />
            <div className="flex gap-2">
              <Input
                value={tag}
                placeholder={t("cardTag")}
                aria-label={t("cardTag")}
                onChange={(event) => setTag(event.target.value)}
              />
              <Button type="submit" size="sm" disabled={!title.trim()}>
                {t("addCard")}
              </Button>
            </div>
          </form>
          <div className="grid min-h-0 flex-1 grid-cols-2 gap-2 overflow-hidden p-2">
            {(["progress", "done"] as const).map((status) => (
              <BoardColumn
                key={status}
                status={status}
                cards={workspace.cards.filter((card) => card.status === status)}
                loading={loading}
                onMove={moveCard}
                onDelete={deleteCard}
              />
            ))}
          </div>
        </div>
      ) : (
        <div
          role="tabpanel"
          id="project-panel-library"
          aria-labelledby="project-tab-library"
          className="flex min-h-0 flex-1 flex-col"
        >
          <LibraryView sessionId={sessionId} cwd={cwd} openedPath={activePath} onActivePath={onActivePath} />
        </div>
      )}
    </aside>
  );
}

function BoardColumn({
  status,
  cards,
  loading,
  onMove,
  onDelete,
}: {
  status: BoardStatus;
  cards: BoardCard[];
  loading: boolean;
  onMove: (id: string, status: BoardStatus) => void;
  onDelete: (id: string) => void;
}) {
  const { t } = useI18n();
  const label = status === "progress" ? t("boardInProgress") : t("boardDone");
  const next: BoardStatus = status === "progress" ? "done" : "progress";
  return (
    <section
      className="flex min-h-0 min-w-0 flex-col rounded-md border border-border bg-background/40"
      aria-label={label}
      onDragOver={(event) => {
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
      }}
      onDrop={(event) => {
        event.preventDefault();
        const id = event.dataTransfer.getData("text/plain");
        if (id) onMove(id, status);
      }}
    >
      <header className="flex items-center justify-between gap-1 px-2 py-1.5">
        <h4 className="truncate font-mono text-[10px] uppercase tracking-[0.12em] text-muted-foreground">{label}</h4>
        <span className="font-mono text-[10px] text-muted-foreground">{cards.length}</span>
      </header>
      <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-1.5 pb-2">
        {loading && cards.length === 0 && (
          <p className="px-1 text-[12px] text-muted-foreground">{t("loading")}</p>
        )}
        {!loading && cards.length === 0 && (
          <p className="px-1 text-[12px] leading-relaxed text-muted-foreground">{t("boardEmpty")}</p>
        )}
        {cards.map((card) => (
          <article
            key={card.id}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData("text/plain", card.id);
              event.dataTransfer.effectAllowed = "move";
            }}
            className="cursor-grab rounded-md border border-border bg-card px-2 py-1.5 active:cursor-grabbing"
          >
            <div className="flex items-start gap-1.5">
              <span className="shrink-0 font-mono text-[10px] text-muted-foreground">#{card.number || "–"}</span>
              <p className="min-w-0 flex-1 text-[13px] leading-snug">{card.title}</p>
            </div>
            <div className="mt-1 flex flex-wrap gap-1">
              {card.failed && (
                <Badge className="border-destructive/40 text-destructive normal-case tracking-normal">{t("cardFailed")}</Badge>
              )}
              {card.tag && <Badge className="max-w-full truncate normal-case tracking-normal">{card.tag}</Badge>}
            </div>
            <div className="mt-1.5 flex gap-1">
              <button
                type="button"
                className="rounded-md px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-accent hover:text-foreground"
                onClick={() => onMove(card.id, next)}
              >
                {status === "progress" ? t("moveToDone") : t("moveToProgress")}
              </button>
              <button
                type="button"
                className="rounded-md px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-accent hover:text-destructive"
                aria-label={t("deleteCard")}
                onClick={() => onDelete(card.id)}
              >
                {t("deleteCard")}
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
