import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api } from "@/api";
import { LanguageSelect, useI18n } from "@/i18n";
import type { AppSettings, ClaudeDetectResult, KeyStatus } from "@/types";

export function SettingsView({
  settings,
  keyStatus,
  detect,
  onClose,
  onSaved,
  onSettings,
  onKeyStatus,
  onDetect,
}: {
  settings: AppSettings;
  keyStatus: KeyStatus;
  detect: ClaudeDetectResult | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
  onSettings: (s: AppSettings) => void;
  onKeyStatus: (k: KeyStatus) => void;
  onDetect: (d: ClaudeDetectResult) => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(settings);
  const [keyInput, setKeyInput] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [statusKind, setStatusKind] = useState<"ok" | "error" | null>(null);
  const [busy, setBusy] = useState(false);
  const [dataDir, setDataDir] = useState("");

  useEffect(() => {
    void api.dataDir().then(setDataDir).catch(() => undefined);
  }, []);

  const pickBinary = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Binary", extensions: ["*"] }],
    });
    if (typeof selected === "string") {
      setDraft((d) => ({ ...d, claudeBinaryPath: selected }));
    }
  };

  const pickCwd = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setDraft((d) => ({ ...d, defaultCwd: selected }));
    }
  };

  const toolOptions = (
    <select
      className="h-8 w-full rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
      value={draft.allowedToolsPreset}
      onChange={(e) => setDraft({ ...draft, allowedToolsPreset: e.target.value })}
    >
      <option value="read">{t("toolsRead")}</option>
      <option value="read_edit">{t("toolsReadEdit")}</option>
      <option value="full">{t("toolsFull")}</option>
    </select>
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <h2 className="font-mono text-sm">{t("settings")}</h2>
          <p className="text-[12px] text-muted-foreground">{t("settingsSubtitle")}</p>
        </div>
        <Button variant="outline" onClick={onClose}>
          {t("back")}
        </Button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-xl flex-col gap-4 px-4 py-4">
          <div className="space-y-1.5">
            <Label htmlFor="lang">{t("language")}</Label>
            <LanguageSelect />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="api-key">{t("apiKey")}</Label>
            <Input
              id="api-key"
              type="password"
              value={keyInput}
              autoComplete="off"
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder={
                keyStatus.configured
                  ? t("apiKeySaved", { mask: keyStatus.masked ?? "" })
                  : t("apiKeyEmpty")
              }
            />
            <div className="flex flex-wrap gap-2">
              <Button
                disabled={!keyInput.trim() || busy}
                onClick={() => {
                  setBusy(true);
                  void api
                    .saveApiKey(keyInput)
                    .then((k) => {
                      onKeyStatus(k);
                      setKeyInput("");
                      setStatus(t("keySaved"));
                      setStatusKind("ok");
                    })
                    .catch((e) => {
                      setStatus(String(e));
                      setStatusKind("error");
                    })
                    .finally(() => setBusy(false));
                }}
              >
                {t("saveKey")}
              </Button>
              <Button
                variant="destructive"
                disabled={!keyStatus.configured || busy}
                onClick={() => {
                  setBusy(true);
                  void api
                    .deleteApiKey()
                    .then((k) => {
                      onKeyStatus(k);
                      setStatus(t("keyDeleted"));
                      setStatusKind("ok");
                    })
                    .catch((e) => {
                      setStatus(String(e));
                      setStatusKind("error");
                    })
                    .finally(() => setBusy(false));
                }}
              >
                {t("deleteKey")}
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => {
                  setBusy(true);
                  void api
                    .testConnection()
                    .then((r) => {
                      setStatus(r.message + (r.resultPreview ? ` — ${r.resultPreview}` : ""));
                      setStatusKind(r.ok ? "ok" : "error");
                    })
                    .catch((e) => {
                      setStatus(String(e));
                      setStatusKind("error");
                    })
                    .finally(() => setBusy(false));
                }}
              >
                {t("testConnection")}
              </Button>
            </div>
            <p className="text-[12px] text-muted-foreground">{t("balanceNote")}</p>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="binary">{t("binaryPath")}</Label>
            <div className="flex gap-2">
              <Input
                id="binary"
                className="font-mono text-xs"
                value={draft.claudeBinaryPath ?? ""}
                placeholder={t("binaryPlaceholder")}
                onChange={(e) =>
                  setDraft({ ...draft, claudeBinaryPath: e.target.value || null })
                }
              />
              <Button type="button" variant="outline" onClick={() => void pickBinary()}>
                {t("browse")}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  void api.detectClaude().then((d) => {
                    onDetect(d);
                    const detail = `${d.path ?? ""}${d.version ? ` (${d.version})` : ""}`;
                    setStatus(d.found ? t("foundCli", { detail }) : d.error ?? t("notFound"));
                    setStatusKind(d.found ? "ok" : "error");
                  });
                }}
              >
                {t("detect")}
              </Button>
            </div>
            {detect && (
              <p className={detect.found ? "text-[12px] text-primary" : "text-[12px] text-destructive"}>
                {detect.found
                  ? t("detectedCli", {
                      detail: `${detect.path ?? ""}${detect.version ? ` · ${detect.version}` : ""}`,
                    })
                  : detect.error}
              </p>
            )}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="folder">{t("defaultFolder")}</Label>
            <div className="flex gap-2">
              <Input
                id="folder"
                className="font-mono text-xs"
                value={draft.defaultCwd ?? ""}
                onChange={(e) => setDraft({ ...draft, defaultCwd: e.target.value || null })}
              />
              <Button type="button" variant="outline" onClick={() => void pickCwd()}>
                {t("browse")}
              </Button>
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="model">{t("defaultModel")}</Label>
            <Input
              id="model"
              value={draft.model ?? ""}
              placeholder={t("cliDefault")}
              onChange={(e) => setDraft({ ...draft, model: e.target.value || null })}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="default-tools">{t("defaultTools")}</Label>
            {toolOptions}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="timeout">{t("timeoutSeconds")}</Label>
            <Input
              id="timeout"
              type="number"
              min={30}
              value={draft.timeoutSecs}
              onChange={(e) =>
                setDraft({ ...draft, timeoutSecs: Number(e.target.value) || 1800 })
              }
            />
          </div>
          {dataDir && (
            <p className="break-all font-mono text-[11px] text-muted-foreground">
              {t("localHistory", { path: dataDir })}
            </p>
          )}
          {status && (
            <p className={statusKind === "error" ? "text-[13px] text-destructive" : "text-[13px] text-primary"}>
              {status}
            </p>
          )}
          <div>
            <Button
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void api
                  .saveSettings(draft)
                  .then(async (s) => {
                    onSettings(s);
                    setStatus(t("settingsSaved"));
                    setStatusKind("ok");
                    await onSaved();
                  })
                  .catch((e) => {
                    setStatus(String(e));
                    setStatusKind("error");
                  })
                  .finally(() => setBusy(false));
              }}
            >
              {t("saveSettings")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
