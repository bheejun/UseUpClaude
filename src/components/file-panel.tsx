import { X } from "lucide-react";
import { useEffect, useState } from "react";
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
  onClose,
}: {
  sessionId: string;
  cwd: string;
  paths: string[];
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [view, setView] = useState<ProjectFileView | null>(null);
  const [loading, setLoading] = useState(false);
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    if (openPath && !paths.includes(openPath)) setOpenPath(null);
  }, [openPath, paths]);

  useEffect(() => {
    if (!openPath) {
      setView(null);
      setMissing(false);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setMissing(false);
    setView(null);
    void api
      .readProjectFile(sessionId, openPath)
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
  }, [openPath, sessionId]);

  return (
    <aside
      className="absolute inset-y-0 right-0 z-20 flex w-full max-w-sm shrink-0 flex-col border-l border-border bg-[#101318] shadow-xl min-[821px]:static min-[821px]:z-auto min-[821px]:w-[min(22rem,34vw)] min-[821px]:max-w-none min-[821px]:shadow-none"
      aria-label={t("producedFiles")}
    >
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
          const selected = path === openPath;
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
                onClick={() => setOpenPath(path)}
              >
                {projectRelativePath(path, cwd)}
              </button>
            </li>
          );
        })}
      </ul>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {!openPath && <p className="text-[13px] leading-relaxed text-muted-foreground">{t("producedFilesHint")}</p>}
        {openPath && loading && <p className="text-[13px] text-muted-foreground">{t("filePreviewLoading")}</p>}
        {openPath && missing && (
          <p className="text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewMissing")}</p>
        )}
        {openPath && view?.kind === "unsupported" && (
          <div>
            <p className="break-all font-mono text-[12px] text-foreground">{view.path}</p>
            <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewUnsupported")}</p>
          </div>
        )}
        {openPath && view && view.kind !== "unsupported" && view.content != null && (
          <div>
            {view.truncated && (
              <p className="mb-2 text-[12px] text-muted-foreground">{t("filePreviewTruncated")}</p>
            )}
            {view.kind === "markdown" ? (
              <MarkdownMessage text={view.content} />
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
