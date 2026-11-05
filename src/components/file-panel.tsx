import { X } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { api } from "@/api";
import { MarkdownMessage } from "@/components/markdown-message";
import { projectRelativePath } from "@/produced-files";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type { ProjectFileView } from "@/types";

export function FilePanel({
  sessionId,
  cwd,
  paths,
  activePath,
  onActivePath,
  onClose,
}: {
  sessionId: string;
  cwd: string;
  paths: string[];
  activePath: string | null;
  onActivePath: (path: string | null) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [view, setView] = useState<ProjectFileView | null>(null);
  const [loading, setLoading] = useState(false);
  const [missing, setMissing] = useState(false);
  const [width, setWidth] = useState(352);
  const widthRef = useRef(width);
  widthRef.current = width;
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);

  useEffect(() => {
    if (activePath && !paths.includes(activePath)) onActivePath(null);
  }, [activePath, paths, onActivePath]);

  useEffect(() => {
    if (!activePath) {
      setView(null);
      setMissing(false);
      setLoading(false);
      setEditing(false);
      setSaveError(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setMissing(false);
    setView(null);
    setEditing(false);
    setSaveError(false);
    void api
      .readProjectFile(sessionId, activePath)
      .then((next) => {
        if (!cancelled) setView(next);
      })
      .catch(() => {
        if (!cancelled) setMissing(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activePath, sessionId]);

  const editable = Boolean(activePath && view && !view.truncated && view.content != null && view.kind !== "unsupported");

  const clampWidth = (next: number) => {
    const max = Math.max(280, Math.round(window.innerWidth * 0.72));
    return Math.min(max, Math.max(280, next));
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

  const saveEdit = () => {
    if (!activePath || saving) return;
    setSaving(true);
    setSaveError(false);
    void api
      .writeProjectFile(sessionId, activePath, draft)
      .then(() => {
        setView((current) => (current ? { ...current, content: draft, truncated: false } : current));
        setEditing(false);
      })
      .catch(() => {
        setSaveError(true);
      })
      .finally(() => {
        setSaving(false);
      });
  };

  return (
    <aside
      className="file-panel absolute inset-y-0 right-0 z-20 flex shrink-0 flex-col border-l border-border bg-[#101318] shadow-xl min-[821px]:relative min-[821px]:inset-auto min-[821px]:z-auto min-[821px]:shadow-none"
      style={{ "--file-panel-width": `${width}px` } as CSSProperties}
      aria-label={t("producedFiles")}
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
        <h3 className="font-mono text-[11px] uppercase tracking-[0.14em] text-muted-foreground">
          {t("producedFiles")}
        </h3>
        <button
          type="button"
          className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={t("closeFilePanel")}
          onClick={onClose}
        >
          <X className="size-3.5" />
        </button>
      </div>
      <ul className="max-h-[40%] shrink-0 overflow-y-auto border-b border-border py-1" aria-label={t("producedFiles")}>
        {paths.map((path) => {
          const selected = path === activePath;
          return (
            <li key={path}>
              <button
                type="button"
                className={cn(
                  "block w-full truncate px-3 py-1.5 text-left font-mono text-[12px] hover:bg-accent",
                  selected ? "bg-accent text-primary" : "text-foreground",
                )}
                title={path}
                aria-current={selected ? "true" : undefined}
                onClick={() => onActivePath(path)}
              >
                {projectRelativePath(path, cwd)}
              </button>
            </li>
          );
        })}
      </ul>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {!activePath && <p className="text-[13px] leading-relaxed text-muted-foreground">{t("producedFilesHint")}</p>}
        {activePath && loading && <p className="text-[13px] text-muted-foreground">{t("filePreviewLoading")}</p>}
        {activePath && missing && (
          <p className="text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewMissing")}</p>
        )}
        {activePath && view?.kind === "unsupported" && (
          <div>
            <p className="break-all font-mono text-[12px] text-foreground">{view.path}</p>
            <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewUnsupported")}</p>
          </div>
        )}
        {activePath && view && view.kind !== "unsupported" && view.content != null && (
          <div>
            {view.truncated && (
              <p className="mb-2 text-[12px] text-muted-foreground">{t("filePreviewTruncated")}</p>
            )}
            {editable && (
              <div className="mb-2 flex items-center gap-2">
                {editing ? (
                  <>
                    <button
                      type="button"
                      className="rounded-md border border-border px-2 py-1 font-mono text-[11px] hover:bg-accent disabled:opacity-50"
                      disabled={saving}
                      onClick={saveEdit}
                    >
                      {saving ? t("savingFile") : t("saveFile")}
                    </button>
                    <button
                      type="button"
                      className="rounded-md border border-border px-2 py-1 font-mono text-[11px] text-muted-foreground hover:bg-accent disabled:opacity-50"
                      disabled={saving}
                      onClick={() => {
                        setEditing(false);
                        setSaveError(false);
                      }}
                    >
                      {t("cancelFileEdit")}
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="rounded-md border border-border px-2 py-1 font-mono text-[11px] hover:bg-accent"
                    onClick={() => {
                      setDraft(view.content ?? "");
                      setSaveError(false);
                      setEditing(true);
                    }}
                  >
                    {view.kind === "markdown" ? t("editMarkdownSource") : t("editFile")}
                  </button>
                )}
              </div>
            )}
            {saveError && (
              <p className="mb-2 text-[12px] leading-relaxed text-destructive">{t("fileSaveFailed")}</p>
            )}
            {editing ? (
              <textarea
                className="min-h-64 w-full resize-y rounded-md border border-border bg-[#0c0e11] p-2 font-mono text-[12px] leading-relaxed text-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring"
                value={draft}
                spellCheck={false}
                aria-label={view.kind === "markdown" ? t("editMarkdownSource") : t("editFile")}
                onChange={(event) => setDraft(event.target.value)}
              />
            ) : view.kind === "markdown" ? (
              <MarkdownMessage text={view.content} linkBase={activePath} onOpenFile={onActivePath} />
            ) : (
              <pre className="m-0 whitespace-pre-wrap break-words font-mono text-[12px] leading-relaxed text-foreground">
                {view.content}
              </pre>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
