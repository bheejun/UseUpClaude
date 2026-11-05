import { Folder, X } from "lucide-react";
import { useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { LibraryView } from "@/components/library-view";
import { useI18n } from "@/i18n";

export function ProjectPanel({
  sessionId,
  cwd,
  activePath,
  onActivePath,
  onClose,
}: {
  sessionId: string;
  cwd: string;
  activePath: string | null;
  onActivePath: (path: string | null) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [width, setWidth] = useState(672);
  const widthRef = useRef(width);
  widthRef.current = width;

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
      aria-label={t("projectLibrary")}
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
          <Folder className="size-3.5 shrink-0" />
          <span className="truncate">{t("projectLibrary")}</span>
        </h3>
        <button
          type="button"
          className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={t("closeFilePanel")}
          onClick={onClose}
        >
          <X className="size-3.5" />
        </button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col">
        <LibraryView sessionId={sessionId} cwd={cwd} openedPath={activePath} onActivePath={onActivePath} />
      </div>
    </aside>
  );
}
