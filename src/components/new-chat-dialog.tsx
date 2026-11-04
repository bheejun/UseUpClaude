import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useI18n } from "@/i18n";

export function NewChatDialog({
  open: isOpen,
  onOpenChange,
  defaultCwd,
  defaultModel,
  defaultTools,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defaultCwd: string | null;
  defaultModel: string | null;
  defaultTools: string;
  onCreate: (args: { cwd: string; model: string | null; tools: string }) => Promise<void>;
}) {
  const { t } = useI18n();
  const [cwd, setCwd] = useState(defaultCwd ?? "");
  const [model, setModel] = useState(defaultModel ?? "");
  const [tools, setTools] = useState(defaultTools || "read");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    setCwd(defaultCwd ?? "");
    setModel(defaultModel ?? "");
    setTools(defaultTools || "read");
    setErr(null);
  }, [isOpen, defaultCwd, defaultModel, defaultTools]);

  const pick = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") setCwd(selected);
  };

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("newChat")}</DialogTitle>
          <DialogDescription>{t("newChatHint")}</DialogDescription>
        </DialogHeader>
        <p className="mb-3 rounded-md border border-amber-900/70 bg-amber-950/50 px-3 py-2 text-[12.5px] leading-snug text-amber-100/90">
          {t("printModeWarning")}
        </p>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="cwd">{t("projectFolder")}</Label>
            <div className="flex gap-2">
              <Input
                id="cwd"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                className="font-mono text-xs"
                placeholder="C:\project"
              />
              <Button type="button" variant="outline" onClick={() => void pick()}>
                {t("browse")}
              </Button>
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="model">{t("modelOptional")}</Label>
            <Input
              id="model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={t("modelPlaceholder")}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="tools">{t("allowedTools")}</Label>
            <select
              id="tools"
              className="h-8 w-full rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
              value={tools}
              onChange={(e) => setTools(e.target.value)}
            >
              <option value="read">{t("toolsRead")}</option>
              <option value="read_edit">{t("toolsReadEdit")}</option>
              <option value="full">{t("toolsFull")}</option>
            </select>
          </div>
          {err && <p className="text-[13px] text-destructive">{err}</p>}
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t("close")}
            </Button>
            <Button
              type="button"
              disabled={!cwd.trim() || busy}
              onClick={() => {
                setBusy(true);
                setErr(null);
                void onCreate({
                  cwd: cwd.trim(),
                  model: model.trim() || null,
                  tools,
                })
                  .catch((e) => setErr(String(e)))
                  .finally(() => setBusy(false));
              }}
            >
              {t("create")}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
