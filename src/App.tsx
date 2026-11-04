import { useCallback, useEffect, useMemo, useState } from "react";
import { Folder, Plus, Settings, Terminal } from "lucide-react";
import { api } from "@/api";
import { folderBase, formatUsd, presentTurn, sumSessionCost } from "@/chat-format";
import { ChatThread, type Bubble } from "@/components/chat-thread";
import { DiscoverDialog } from "@/components/discover-dialog";
import { NewChatDialog } from "@/components/new-chat-dialog";
import { SessionCostsDialog } from "@/components/session-costs-dialog";
import { SettingsView } from "@/components/settings-view";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { LanguageSelect, useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type {
  AppSettings,
  ClaudeDetectResult,
  GuiSession,
  KeyStatus,
  ModelCatalog,
  SessionCostReport,
  SessionSummary,
} from "@/types";

type View = "chat" | "settings";

const DEFAULT_SETTINGS: AppSettings = {
  claudeBinaryPath: null,
  defaultCwd: null,
  model: null,
  timeoutSecs: 1800,
  allowedToolsPreset: "read",
};

function EntryNotices() {
  const { t } = useI18n();
  return (
    <div className="border-b border-amber-900/70 bg-amber-950/40 text-[12.5px] leading-snug text-amber-100/90">
      <p className="px-4 py-2">{t("printModeWarning")}</p>
      <p className="border-t border-amber-900/50 px-4 py-2">{t("communityNotice")}</p>
    </div>
  );
}

export default function App() {
  const { t } = useI18n();
  const [view, setView] = useState<View>("chat");
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [keyStatus, setKeyStatus] = useState<KeyStatus>({ configured: false, masked: null });
  const [costReport, setCostReport] = useState<SessionCostReport | null>(null);
  const [showCosts, setShowCosts] = useState(false);
  const [catalog, setCatalog] = useState<ModelCatalog>({ models: [], errorCode: null });
  const [detect, setDetect] = useState<ClaudeDetectResult | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [active, setActive] = useState<GuiSession | null>(null);
  const [prompt, setPrompt] = useState("");
  const [draftUser, setDraftUser] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showNewChat, setShowNewChat] = useState(false);
  const [showDiscover, setShowDiscover] = useState(false);
  const [projectFilter, setProjectFilter] = useState("all");
  const [bootstrapped, setBootstrapped] = useState(false);

  const refreshSessions = useCallback(async () => {
    setSessions(await api.listSessions());
  }, []);

  const refreshMoney = useCallback(async () => {
    setCostReport(await api.sessionCosts());
  }, []);

  const refreshModels = useCallback(async () => {
    try {
      setCatalog(await api.listModels());
    } catch {
      setCatalog({ models: [], errorCode: "failed" });
    }
  }, []);

  const bootstrap = useCallback(async () => {
    try {
      const [s, k, d] = await Promise.all([
        api.getSettings(),
        api.getKeyStatus(),
        api.detectClaude(),
      ]);
      setSettings(s);
      setKeyStatus(k);
      setDetect(d);
      await refreshSessions();
      try {
        await refreshMoney();
      } catch {
        // Session costs can refresh later.
      }
      await refreshModels();
      if (!k.configured || !d.found) setView("settings");
    } catch (e) {
      setError(String(e));
    } finally {
      setBootstrapped(true);
    }
  }, [refreshModels, refreshMoney, refreshSessions]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  const selectSession = async (id: string) => {
    setError(null);
    setView("chat");
    setActive(await api.getSession(id));
  };

  const readyHint = !keyStatus.configured
    ? t("apiKeyRequired")
    : !detect?.found
      ? t("cliMissingLong")
      : null;

  const handleSend = async () => {
    const text = prompt.trim();
    if (!text || busy || readyHint) return;
    if (!active) {
      setShowNewChat(true);
      return;
    }
    setBusy(true);
    setError(null);
    setPrompt("");
    setDraftUser(text);
    try {
      const res = await api.sendMessage({
        sessionId: active.id,
        prompt: text,
        cwd: active.cwd,
        model: active.model,
        allowedToolsPreset: active.allowedToolsPreset,
      });
      setActive(res.session);
      setDraftUser(null);
      await refreshSessions();
      try {
        await refreshMoney();
      } catch {
        // The reply is already on screen. Cost totals can refresh later.
      }
    } catch (e) {
      setError(String(e));
      setPrompt(text);
      setDraftUser(null);
      try {
        setActive(await api.getSession(active.id));
      } catch {
        // Keep the session already on screen.
      }
    } finally {
      setBusy(false);
    }
  };

  const costThisSession = useMemo(() => sumSessionCost(active?.turns), [active]);

  const projects = useMemo(() => {
    const set = new Set(sessions.map((s) => s.cwd).filter(Boolean));
    return [...set].sort((a, b) => a.localeCompare(b));
  }, [sessions]);

  const visible = useMemo(
    () => (projectFilter === "all" ? sessions : sessions.filter((s) => s.cwd === projectFilter)),
    [sessions, projectFilter],
  );

  const grouped = useMemo(() => {
    const map = new Map<string, SessionSummary[]>();
    for (const session of visible) {
      const list = map.get(session.cwd) ?? [];
      list.push(session);
      map.set(session.cwd, list);
    }
    return [...map.entries()];
  }, [visible]);

  const bubbles: Bubble[] = useMemo(() => {
    const fromSession = (active?.turns ?? []).map((turn) => {
      const shown = presentTurn(turn);
      return {
        id: turn.id,
        role: turn.role,
        text: shown.text,
        costUsd: shown.costUsd,
        finishedAt: turn.finishedAt,
      };
    });
    if (draftUser) {
      fromSession.push({
        id: "draft-user",
        role: "user",
        text: draftUser,
        costUsd: null,
        finishedAt: null,
      });
    }
    return fromSession;
  }, [active, draftUser]);

  const openDiscovered = async (args: { cwd: string; sessionId: string; title: string }) => {
    const existing = sessions.find((s) => s.cliSessionId === args.sessionId);
    if (existing) {
      await selectSession(existing.id);
      setShowDiscover(false);
      return;
    }
    const session = await api.createSession({
      cwd: args.cwd,
      cliSessionId: args.sessionId,
      title: args.title,
      model: settings.model,
      allowedToolsPreset: settings.allowedToolsPreset,
    });
    setActive(session);
    setShowDiscover(false);
    setView("chat");
    await refreshSessions();
  };

  if (!bootstrapped) {
    return (
      <div className="grid h-full place-items-center font-mono text-xs text-muted-foreground">
        {t("loading")}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 bg-background text-foreground">
      <aside className="hidden w-[272px] shrink-0 flex-col border-r border-border bg-[#101318] min-[821px]:flex">
        <div className="border-b border-border px-3 py-3">
          <h1 className="font-mono text-[15px] tracking-tight text-primary">UseUpClaude</h1>
          <p className="mt-1 font-mono text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
            {t("brandTagline")}
          </p>
          <LanguageSelect className="mt-3" />
        </div>
        <div className="flex gap-2 px-3 py-3">
          <Button className="flex-1" onClick={() => setShowNewChat(true)}>
            <Plus className="size-3.5" />
            {t("newChat")}
          </Button>
          <Button variant="outline" size="icon" aria-label={t("settings")} onClick={() => setView("settings")}>
            <Settings className="size-3.5" />
          </Button>
        </div>
        <div className="px-3 pb-2">
          <Button variant="outline" className="w-full justify-start" onClick={() => setShowDiscover(true)}>
            <Terminal className="size-3.5" />
            {t("openClaudeSessions")}
          </Button>
        </div>
        <div className="px-3 pb-2">
          <select
            aria-label={t("allProjects")}
            className="h-8 w-full rounded-md border border-input bg-[#0c0e11] px-2 font-mono text-[11px] outline-none"
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
          >
            <option value="all">{t("allProjects")}</option>
            {projects.map((cwd) => (
              <option key={cwd} value={cwd}>
                {folderBase(cwd)}
              </option>
            ))}
          </select>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {grouped.length === 0 ? (
            <p className="px-2 py-2 text-[12px] text-muted-foreground">{t("noSessions")}</p>
          ) : (
            grouped.map(([cwd, items]) => (
              <section key={cwd || "none"} className="mb-3">
                <h2
                  className="flex items-center gap-1 truncate px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-muted-foreground"
                  title={cwd}
                >
                  <Folder className="size-3 shrink-0" />
                  {folderBase(cwd)}
                </h2>
                {items.map((session) => (
                  <button
                    key={session.id}
                    type="button"
                    className={cn(
                      "mb-0.5 w-full rounded-md px-2 py-1.5 text-left hover:bg-accent",
                      active?.id === session.id && "bg-accent",
                    )}
                    onClick={() => void selectSession(session.id)}
                  >
                    <span className="block truncate text-[13px]">{session.title}</span>
                    <span className="block truncate font-mono text-[10px] text-muted-foreground">
                      {session.cwd}
                    </span>
                  </button>
                ))}
              </section>
            ))
          )}
        </div>
        <footer className="border-t border-border px-3 py-2">
          <button
            type="button"
            className="text-left font-mono text-[11px] text-primary hover:underline"
            onClick={() => setShowCosts(true)}
          >
            {t("allSessionsCost", {
              cost: costReport?.totalUsd != null ? formatUsd(costReport.totalUsd) : "—",
            })}
          </button>
          <p className="mt-1 text-[11px] leading-snug text-muted-foreground">{t("balanceNote")}</p>
        </footer>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <EntryNotices />
        {view === "settings" ? (
          <SettingsView
            settings={settings}
            keyStatus={keyStatus}
            catalog={catalog}
            detect={detect}
            onClose={() => setView("chat")}
            onSaved={async () => {
              await bootstrap();
              setView("chat");
            }}
            onSettings={setSettings}
            onKeyStatus={(status) => {
              setKeyStatus(status);
              void refreshModels();
            }}
            onDetect={setDetect}
          />
        ) : (
          <>
            <header className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
              <div className="min-w-0">
                <h2 className="truncate text-[14px]">{active?.title ?? t("startTitle")}</h2>
                {active && (
                  <p className="truncate font-mono text-[10px] text-muted-foreground">
                    {active.cwd}
                    {" · "}
                    {active.cliSessionId ? t("resumeReady") : t("newSession")}
                  </p>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <Badge className={keyStatus.configured && detect?.found ? "text-primary" : "text-amber-200"}>
                  {keyStatus.configured ? t("keyOk") : t("noKey")}
                  {" · "}
                  {detect?.found ? t("cliOk") : t("cliMissing")}
                </Badge>
                <Badge>{t("tools", { preset: active?.allowedToolsPreset ?? settings.allowedToolsPreset })}</Badge>
                <Button
                  variant="outline"
                  size="icon"
                  className="min-[821px]:hidden"
                  aria-label={t("settings")}
                  onClick={() => setView("settings")}
                >
                  <Settings className="size-3.5" />
                </Button>
              </div>
            </header>

            <div className="min-h-0 flex-1 overflow-y-auto">
              {!active ? (
                <div className="mx-auto flex h-full max-w-md flex-col items-center justify-center px-6 text-center">
                  <p className="font-mono text-xs uppercase tracking-[0.2em] text-primary">UseUpClaude</p>
                  <h3 className="mt-3 text-lg">{t("startTitle")}</h3>
                  <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">{t("startBody")}</p>
                  <div className="mt-4 flex gap-2">
                    <Button onClick={() => setShowNewChat(true)}>
                      <Plus className="size-3.5" />
                      {t("newChat")}
                    </Button>
                    <Button variant="outline" onClick={() => setShowDiscover(true)}>
                      <Terminal className="size-3.5" />
                      {t("openClaudeSessions")}
                    </Button>
                  </div>
                </div>
              ) : (
                <>
                  {bubbles.length === 0 && !busy && (
                    <p className="mx-auto max-w-xl px-6 pt-8 text-[13px] leading-relaxed text-muted-foreground">
                      {active.cliSessionId ? t("resumedEmpty") : t("emptyThread")}
                    </p>
                  )}
                  <ChatThread bubbles={bubbles} thinking={busy} />
                </>
              )}
            </div>

            <form
              className="shrink-0 border-t border-border bg-background/95 px-4 py-3"
              onSubmit={(e) => {
                e.preventDefault();
                void handleSend();
              }}
            >
              {active && (
                <div className="mx-auto mb-2 max-w-3xl">
                  <p className="font-mono text-[11px] text-primary">
                    {costThisSession != null
                      ? t("sessionCost", { cost: formatUsd(costThisSession) })
                      : t("noTurnCostYet")}
                  </p>
                  <div className="min-[821px]:hidden">
                    <button
                      type="button"
                      className="mt-1 text-left font-mono text-[11px] text-primary hover:underline"
                      onClick={() => setShowCosts(true)}
                    >
                      {t("allSessionsCost", {
                        cost: costReport?.totalUsd != null ? formatUsd(costReport.totalUsd) : "—",
                      })}
                    </button>
                  </div>
                </div>
              )}
              {!active && (
                <div className="mx-auto mb-2 max-w-3xl min-[821px]:hidden">
                  <button
                    type="button"
                    className="text-left font-mono text-[11px] text-primary hover:underline"
                    onClick={() => setShowCosts(true)}
                  >
                    {t("allSessionsCost", {
                      cost: costReport?.totalUsd != null ? formatUsd(costReport.totalUsd) : "—",
                    })}
                  </button>
                </div>
              )}
              {error && <p className="mb-2 text-[13px] text-destructive">{error}</p>}
              {readyHint && !error && (
                <p className="mb-2 text-[13px] text-destructive">{t("setupHint", { hint: readyHint })}</p>
              )}
              <div className="mx-auto flex max-w-3xl items-end gap-2">
                <Textarea
                  value={prompt}
                  placeholder={active ? t("placeholderActive") : t("placeholderIdle")}
                  disabled={busy || !!readyHint}
                  onChange={(e) => setPrompt(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                      e.preventDefault();
                      void handleSend();
                    }
                  }}
                />
                <div className="flex flex-col gap-2">
                  {busy ? (
                    <Button type="button" variant="destructive" onClick={() => void api.cancelActive()}>
                      {t("cancel")}
                    </Button>
                  ) : (
                    <Button type="submit" disabled={!prompt.trim() || !!readyHint}>
                      {t("send")}
                    </Button>
                  )}
                </div>
              </div>
              <p className="mx-auto mt-2 max-w-3xl font-mono text-[10px] text-muted-foreground">
                {t("composerHint")}
              </p>
            </form>
          </>
        )}
      </section>

      <NewChatDialog
        open={showNewChat}
        onOpenChange={setShowNewChat}
        defaultCwd={settings.defaultCwd}
        defaultModel={settings.model}
        defaultTools={settings.allowedToolsPreset}
        catalog={catalog}
        onCreate={async ({ cwd, model, tools }) => {
          const session = await api.createSession({
            cwd,
            model,
            allowedToolsPreset: tools,
            title: t("newChat"),
          });
          if (settings.defaultCwd !== cwd) {
            const next = { ...settings, defaultCwd: cwd };
            await api.saveSettings(next);
            setSettings(next);
          }
          setActive(session);
          setShowNewChat(false);
          setView("chat");
          await refreshSessions();
        }}
      />
      <DiscoverDialog
        open={showDiscover}
        onOpenChange={setShowDiscover}
        onOpenSession={openDiscovered}
      />
      <SessionCostsDialog
        open={showCosts}
        report={costReport}
        onOpenChange={setShowCosts}
        onOpenSession={(id) => {
          setShowCosts(false);
          void selectSession(id);
        }}
      />
    </div>
  );
}
