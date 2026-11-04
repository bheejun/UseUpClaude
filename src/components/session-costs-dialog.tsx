import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { formatUsd, folderBase } from "@/chat-format";
import { useI18n } from "@/i18n";
import type { SessionCostReport } from "@/types";

export function SessionCostsDialog({
  open,
  report,
  onOpenChange,
  onOpenSession,
}: {
  open: boolean;
  report: SessionCostReport | null;
  onOpenChange: (open: boolean) => void;
  onOpenSession: (id: string) => void;
}) {
  const { t } = useI18n();
  const sessions = report?.sessions ?? [];
  const total = report?.totalUsd;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("costBreakdown")}
            {total != null ? ` · ${formatUsd(total)}` : ""}
          </DialogTitle>
          <DialogDescription>{t("costBreakdownHint")}</DialogDescription>
        </DialogHeader>
        {sessions.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">{t("noSessions")}</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {sessions.map((session) => (
              <li key={session.id}>
                <button
                  type="button"
                  className="flex w-full items-baseline justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-accent"
                  onClick={() => onOpenSession(session.id)}
                >
                  <span className="min-w-0">
                    <span className="block truncate text-[13px]">{session.title}</span>
                    <span className="block truncate font-mono text-[10px] text-muted-foreground">
                      {folderBase(session.cwd)}
                    </span>
                  </span>
                  <span className="shrink-0 font-mono text-[11px] text-primary">
                    {session.costUsd != null ? formatUsd(session.costUsd) : t("noSessionCost")}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
