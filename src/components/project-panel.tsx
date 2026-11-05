import { Kanban, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type { BoardCard, BoardStatus, LibraryDoc, ProjectWorkspace } from "@/types";

const EMPTY: ProjectWorkspace = { cards: [], docs: [] };

export function ProjectPanel({ cwd, onClose }: { cwd: string; onClose: () => void }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<"board" | "library">("board");
  const [workspace, setWorkspace] = useState<ProjectWorkspace>(EMPTY);
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const [loading, setLoading] = useState(true);
  const [saveFailed, setSaveFailed] = useState(false);
  const [title, setTitle] = useState("");
  const [tag, setTag] = useState("");
  const [selectedDoc, setSelectedDoc] = useState<string | null>(null);
  const saveSeq = useRef(0);
  const saveChain = useRef(Promise.resolve());
  const docTimer = useRef<number | null>(null);

  const save = (next: ProjectWorkspace) => {
    const seq = ++saveSeq.current;
    saveChain.current = saveChain.current
      .catch(() => undefined)
      .then(async () => {
        try {
          const saved = await api.saveProjectWorkspace(cwd, next);
          if (saveSeq.current === seq) {
            workspaceRef.current = saved;
            setWorkspace(saved);
            setSaveFailed(false);
          }
        } catch {
          if (saveSeq.current === seq) setSaveFailed(true);
        }
      });
  };

  const commit = (next: ProjectWorkspace) => {
    workspaceRef.current = next;
    setWorkspace(next);
    save(next);
  };

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSaveFailed(false);
    setSelectedDoc(null);
    void api
      .getProjectWorkspace(cwd)
      .then((next) => {
        if (cancelled) return;
        workspaceRef.current = next;
        setWorkspace(next);
        setSelectedDoc(next.docs[0]?.id ?? null);
      })
      .catch(() => {
        if (!cancelled) setSaveFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
      if (docTimer.current) window.clearTimeout(docTimer.current);
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
    };
    commit({ ...workspaceRef.current, cards: [card, ...workspaceRef.current.cards] });
    setTitle("");
    setTag("");
  };

  const moveCard = (id: string, status: BoardStatus) => {
    commit({
      ...workspaceRef.current,
      cards: workspaceRef.current.cards.map((card) => (card.id === id ? { ...card, status } : card)),
    });
  };

  const deleteCard = (id: string) => {
    commit({
      ...workspaceRef.current,
      cards: workspaceRef.current.cards.filter((card) => card.id !== id),
    });
  };

  const createDoc = () => {
    const doc: LibraryDoc = {
      id: crypto.randomUUID(),
      title: t("untitledDocument"),
      body: "",
    };
    setSelectedDoc(doc.id);
    commit({ ...workspaceRef.current, docs: [doc, ...workspaceRef.current.docs] });
  };

  const editDoc = (id: string, patch: Partial<Pick<LibraryDoc, "title" | "body">>) => {
    const next = {
      ...workspaceRef.current,
      docs: workspaceRef.current.docs.map((doc) => (doc.id === id ? { ...doc, ...patch } : doc)),
    };
    workspaceRef.current = next;
    setWorkspace(next);
    if (docTimer.current) window.clearTimeout(docTimer.current);
    docTimer.current = window.setTimeout(() => save(next), 350);
  };

  const deleteDoc = (id: string) => {
    if (docTimer.current) window.clearTimeout(docTimer.current);
    const next = {
      ...workspaceRef.current,
      docs: workspaceRef.current.docs.filter((doc) => doc.id !== id),
    };
    if (selectedDoc === id) setSelectedDoc(next.docs[0]?.id ?? null);
    commit(next);
  };

  const selected = workspace.docs.find((doc) => doc.id === selectedDoc) ?? null;

  return (
    <aside
      className="project-panel absolute inset-y-0 right-0 z-30 flex shrink-0 flex-col border-l border-border bg-[#101318] shadow-xl min-[821px]:relative min-[821px]:inset-auto min-[821px]:z-auto min-[821px]:shadow-none"
      aria-label={t("projectBoard")}
    >
      <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
        <h3 className="flex min-w-0 items-center gap-1.5 font-mono text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
          <Kanban className="size-3.5 shrink-0" />
          <span className="truncate">{t("projectBoard")}</span>
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
            aria-selected={tab === id}
            aria-controls={`project-panel-${id}`}
            className={cn(
              "border-b-2 px-3 py-2 text-[13px]",
              tab === id
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>
      {saveFailed && (
        <p className="border-b border-border px-3 py-2 text-[12px] text-destructive" role="alert">
          {t("workspaceSaveFailed")}
        </p>
      )}
      {tab === "board" ? (
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
          <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
            <Button type="button" size="sm" onClick={createDoc}>
              {t("newDocument")}
            </Button>
          </div>
          {workspace.docs.length === 0 ? (
            <p className="px-3 py-3 text-[13px] leading-relaxed text-muted-foreground">{t("libraryEmpty")}</p>
          ) : (
            <ul className="max-h-36 shrink-0 overflow-y-auto border-b border-border py-1">
              {workspace.docs.map((doc) => (
                <li key={doc.id}>
                  <button
                    type="button"
                    className={cn(
                      "block w-full truncate px-3 py-1.5 text-left text-[13px] hover:bg-accent",
                      doc.id === selected?.id && "bg-accent text-primary",
                    )}
                    aria-current={doc.id === selected?.id ? "true" : undefined}
                    onClick={() => setSelectedDoc(doc.id)}
                  >
                    {doc.title.trim() || t("untitledDocument")}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {selected && (
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-2">
              <Input
                value={selected.title}
                aria-label={t("documentTitle")}
                onChange={(event) => editDoc(selected.id, { title: event.target.value })}
              />
              <Textarea
                value={selected.body}
                aria-label={t("documentBody")}
                className="min-h-48 flex-1 resize-none font-mono text-[12px]"
                onChange={(event) => editDoc(selected.id, { body: event.target.value })}
              />
              <Button type="button" variant="outline" size="sm" onClick={() => deleteDoc(selected.id)}>
                {t("deleteDocument")}
              </Button>
            </div>
          )}
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
            {card.tag && (
              <Badge className="mt-1 max-w-full truncate normal-case tracking-normal">{card.tag}</Badge>
            )}
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
