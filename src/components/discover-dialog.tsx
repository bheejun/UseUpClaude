import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { api } from "@/api";
import { useI18n } from "@/i18n";
import type { DiscoveredProject } from "@/types";

export function DiscoverDialog({
  open,
  onOpenChange,
  onOpenSession,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenSession: (args: { cwd: string; sessionId: string; title: string }) => Promise<void>;
}) {
  const { t, locale } = useI18n();
  const [projects, setProjects] = useState<DiscoveredProject[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setBusy(true);
    setErr(null);
    void api
      .discoverClaudeSessions()
      .then(setProjects)
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  }, [open]);

  const lang = locale === "zh" ? "zh-CN" : locale;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(40rem,calc(100vw-1.5rem))]">
        <DialogHeader>
          <DialogTitle>{t("discoverTitle")}</DialogTitle>
          <DialogDescription>{t("discoverHint")}</DialogDescription>
        </DialogHeader>
        <p className="mb-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
          {t("discoverPrivacy")}
        </p>
        {err && <p className="mb-2 text-[13px] text-destructive">{err}</p>}
        {busy && <p className="text-[13px] text-muted-foreground">{t("loading")}</p>}
        {!busy && projects.length === 0 && (
          <p className="font-mono text-[12px] text-muted-foreground">{t("discoverEmpty")}</p>
        )}
        <div className="max-h-80 space-y-4 overflow-y-auto pr-1">
          {projects.map((project) => (
            <section key={`${project.cwd ?? "unknown"}-${project.label}`}>
              <h3 className="truncate font-mono text-[11px] text-primary" title={project.cwd ?? project.label}>
                {project.label}
              </h3>
              {project.cwd && (
                <p className="truncate font-mono text-[10px] text-muted-foreground" title={project.cwd}>
                  {project.cwd}
                </p>
              )}
              <ul className="mt-1">
                {project.sessions.map((session) => (
                  <li
                    key={session.sessionId}
                    className="flex items-center gap-2 border-t border-border py-2"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13px]">{session.title}</p>
                      {session.modifiedMs > 0 && (
                        <p className="font-mono text-[10px] text-muted-foreground">
                          {new Date(session.modifiedMs).toLocaleString(lang)}
                        </p>
                      )}
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={!project.cwd || opening === session.sessionId}
                      onClick={() => {
                        if (!project.cwd) return;
                        setOpening(session.sessionId);
                        void onOpenSession({
                          cwd: project.cwd,
                          sessionId: session.sessionId,
                          title: session.title,
                        })
                          .catch((e) => setErr(String(e)))
                          .finally(() => setOpening(null));
                      }}
                    >
                      {project.cwd ? t("discoverOpen") : t("discoverNoCwd")}
                    </Button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
