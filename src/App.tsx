import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { Folder, Plus, Settings, Terminal } from "lucide-react";
import { api } from "@/api";
import { folderBase, formatUsd, presentTurn, sumSessionCost } from "@/chat-format";
import { EffortSelect, ModelSelect, modelListMessage, normalizeEffort, supportsFastMode } from "@/components/model-select";
import { ChatThread, type Bubble } from "@/components/chat-thread";
import { ProjectPanel } from "@/components/project-panel";
import { producedFilePaths, sameProjectFile } from "@/produced-files";
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
  GuiTurn,
  KeyStatus,
  ModelCatalog,
  ProjectSkill,
  SessionCostReport,
  SessionSummary,
  TurnEvent,
} from "@/types";

type View = "chat" | "settings";

const DEFAULT_SETTINGS: AppSettings = {
  claudeBinaryPath: null,
  defaultCwd: null,
  model: null,
  timeoutSecs: 1800,
  allowedToolsPreset: "read",
};

type TurnEventPayload = TurnEvent & { sessionId: string };

function withPendingEvents(session: GuiSession, events: TurnEvent[]): GuiSession {
  if (events.length === 0) return session;
  const turns = [...session.turns];
  const index = turns.findIndex((turn) => turn.status === "pending");
  const pending: GuiTurn =
    index === -1
      ? {
          id: `pending-${session.id}`,
          role: "assistant",
          content: "",
          exitCode: null,
          startedAt: new Date().toISOString(),
          finishedAt: null,
          status: "pending",
          costUsd: null,
          events: [...events],
        }
      : { ...turns[index], events: [...events] };
  if (index === -1) turns.push(pending);
  else turns[index] = pending;
  return { ...session, turns };
}

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
  const [skill, setSkill] = useState<ProjectSkill | null>(null);
  const [projectSkills, setProjectSkills] = useState<ProjectSkill[]>([]);
  const [slashDismissed, setSlashDismissed] = useState(false);
  const [skillIndex, setSkillIndex] = useState(0);
  const composerWrite = useRef(Promise.resolve());
  const focusId = useRef<string | null>(null);
  const busyRef = useRef(new Set<string>());
  const liveEvents = useRef(new Map<string, TurnEvent[]>());
  const [draft, setDraft] = useState<{ sessionId: string; text: string } | null>(null);
  const [busyIds, setBusyIds] = useState<Set<string>>(() => new Set());
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

  useEffect(() => {
    setSkill(null);
    setSlashDismissed(false);
  }, [active?.id]);

  useEffect(() => {
    if (!active?.cwd) {
      setProjectSkills([]);
      return;
    }
    let cancelled = false;
    void api
      .listProjectSkills(active.cwd)
      .then((rows) => {
        if (!cancelled) setProjectSkills(rows);
      })
      .catch(() => {
        if (!cancelled) setProjectSkills([]);
      });
    return () => {
      cancelled = true;
    };
  }, [active?.cwd]);

  const selectSession = async (id: string) => {
    focusId.current = id;
    setDraft(null);
    setError(null);
    setView("chat");
    try {
      let session = await api.getSession(id);
      if (focusId.current !== id) return;
      if (busyRef.current.has(id)) {
        const events = liveEvents.current.get(id);
        if (events?.length) session = withPendingEvents(session, events);
      }
      setActive(session);
    } catch (e) {
      if (focusId.current === id) setError(String(e));
    }
  };

  const notifyReply = async (sessionId: string, title: string) => {
    if (focusId.current === sessionId) {
      try {
        if (await getCurrentWindow().isFocused()) return;
      } catch {
        return;
      }
    }
    try {
      let granted = await isPermissionGranted();
      if (!granted) granted = (await requestPermission()) === "granted";
      if (!granted) return;
      await sendNotification({
        title: "UseUpClaude",
        body: t("replyReady", { title: title || "Chat" }),
      });
    } catch {
      // A missing notification permission should not block the reply.
    }
  };

  const readyHint = !keyStatus.configured
    ? t("apiKeyRequired")
    : !detect?.found
      ? t("cliMissingLong")
      : null;

  const slashQuery = prompt.startsWith("/") && !prompt.includes(" ") && !prompt.includes("\n") ? prompt.slice(1) : null;
  const skillMenuOpen = slashQuery !== null && !slashDismissed && !!active && !skill;
  const skillMatches = useMemo(() => {
    if (slashQuery === null) return [];
    const query = slashQuery.toLowerCase();
    return projectSkills.filter(
      (item) => item.name.toLowerCase().includes(query) || item.description.toLowerCase().includes(query),
    );
  }, [projectSkills, slashQuery]);

  useEffect(() => {
    setSkillIndex(0);
  }, [slashQuery]);

  const queueComposer = (write: () => Promise<void>) => {
    composerWrite.current = composerWrite.current
      .catch(() => undefined)
      .then(write)
      .catch((e) => setError(String(e)))
      .then(() => undefined);
  };

  const chooseModel = (model: string | null) => {
    if (!active) return;
    const id = active.id;
    const fastMode = supportsFastMode(model) ? active.fastMode : false;
    setActive((current) => (current && current.id === id ? { ...current, model, fastMode } : current));
    queueComposer(async () => {
      const session = await api.setSessionModel(id, model);
      if (!supportsFastMode(model) && session.fastMode) {
        await api.setSessionFastMode(id, false);
      }
      setActive((current) => {
        if (!current || current.id !== id || current.model !== model) return current;
        return {
          ...current,
          model: session.model,
          fastMode: supportsFastMode(current.model) ? current.fastMode : false,
        };
      });
    });
  };

  const chooseEffort = (effort: string | null) => {
    if (!active) return;
    const id = active.id;
    const next = normalizeEffort(effort);
    setActive((current) => (current && current.id === id ? { ...current, effort: next } : current));
    queueComposer(async () => {
      const session = await api.setSessionEffort(id, next);
      setActive((current) => {
        if (!current || current.id !== id || normalizeEffort(current.effort) !== next) return current;
        return { ...current, effort: session.effort };
      });
    });
  };

  const chooseFastMode = (fastMode: boolean) => {
    if (!active) return;
    const id = active.id;
    setActive((current) => (current && current.id === id ? { ...current, fastMode } : current));
    queueComposer(async () => {
      const session = await api.setSessionFastMode(id, fastMode);
      setActive((current) => {
        if (!current || current.id !== id || current.fastMode !== fastMode) return current;
        return { ...current, fastMode: session.fastMode };
      });
    });
  };

  const pickSkill = (next: ProjectSkill) => {
    setSkill(next);
    setPrompt("");
    setSlashDismissed(false);
  };

  const handleSend = async () => {
    const text = prompt.trim();
    const chosen = skill;
    if (!active) {
      setShowNewChat(true);
      return;
    }
    const sessionId = active.id;
    const sessionTitle = active.title;
    if ((!text && !chosen) || busyRef.current.has(sessionId) || readyHint) return;
    busyRef.current.add(sessionId);
    setBusyIds(new Set(busyRef.current));
    liveEvents.current.set(sessionId, []);
    setError(null);
    setPrompt("");
    setSkill(null);
    setDraft({
      sessionId,
      text: chosen ? (text ? `/${chosen.name}\n${text}` : `/${chosen.name}`) : text,
    });
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<TurnEventPayload>("turn-event", (message) => {
        const { sessionId: eventSession, ...event } = message.payload;
        if (eventSession !== sessionId || !busyRef.current.has(sessionId)) return;
        const next = [...(liveEvents.current.get(sessionId) ?? []), event];
        liveEvents.current.set(sessionId, next);
        setActive((current) => {
          if (!current || current.id !== sessionId || !busyRef.current.has(sessionId)) return current;
          return withPendingEvents(current, next);
        });
      });
      await composerWrite.current;
      const res = await api.sendMessage({
        sessionId,
        prompt: text,
        cwd: active.cwd,
        model: active.model,
        effort: normalizeEffort(active.effort),
        fastMode: supportsFastMode(active.model) && active.fastMode,
        allowedToolsPreset: active.allowedToolsPreset,
        skill: chosen?.name ?? null,
      });
      busyRef.current.delete(sessionId);
      liveEvents.current.delete(sessionId);
      if (focusId.current === sessionId) setActive(res.session);
      setDraft((current) => (current?.sessionId === sessionId ? null : current));
      await refreshSessions();
      try {
        await refreshMoney();
      } catch {
        // The reply is already stored. Cost totals can refresh later.
      }
      await notifyReply(sessionId, res.session.title || sessionTitle);
    } catch (e) {
      busyRef.current.delete(sessionId);
      liveEvents.current.delete(sessionId);
      setDraft((current) => (current?.sessionId === sessionId ? null : current));
      if (focusId.current === sessionId) {
        setError(String(e));
        setPrompt(text);
        setSkill(chosen);
        try {
          setActive(await api.getSession(sessionId));
        } catch {
          // Keep the session already on screen.
        }
      }
      try {
        await refreshSessions();
      } catch {
        // The sidebar can refresh on the next successful load.
      }
      await notifyReply(sessionId, sessionTitle);
    } finally {
      unlisten?.();
      busyRef.current.delete(sessionId);
      liveEvents.current.delete(sessionId);
      setBusyIds(new Set(busyRef.current));
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

  const sessionBusy = !!active && busyIds.has(active.id);
  const producedPaths = useMemo(
    () => producedFilePaths((active?.turns ?? []).flatMap((turn) => turn.events ?? [])),
    [active],
  );
  const fileSignature = `${active?.id ?? ""}:${producedPaths.join("\0")}`;
  const [dismissedFiles, setDismissedFiles] = useState<string | null>(null);
  const [treeOpenFor, setTreeOpenFor] = useState<string | null>(null);
  const [projectOpen, setProjectOpen] = useState(false);
  const [openedFile, setOpenedFile] = useState<{ sessionId: string; path: string } | null>(null);
  const openedPath = active && openedFile?.sessionId === active.id ? openedFile.path : null;
  const filesOpen =
    openedPath != null ||
    (active != null && treeOpenFor === active.id) ||
    (producedPaths.length > 0 && dismissedFiles !== fileSignature);
  const panelOpen = projectOpen || filesOpen;
  const libraryShown = panelOpen;
  const closeFilePanel = () => {
    setOpenedFile(null);
    setDismissedFiles(fileSignature);
    setTreeOpenFor(null);
  };
  const closeProject = () => {
    setProjectOpen(false);
    closeFilePanel();
  };
  const openProjectFile = useCallback(
    (path: string) => {
      if (!active) return;
      const match = producedPaths.find((item) => sameProjectFile(item, path, active.cwd)) ?? path;
      setDismissedFiles(null);
      setTreeOpenFor(active.id);
      setOpenedFile({ sessionId: active.id, path: match });
    },
    [active, producedPaths],
  );
  const choosePanelPath = useCallback(
    (path: string | null) => {
      if (!active || !path) {
        setOpenedFile(null);
        return;
      }
      const match = producedPaths.find((item) => sameProjectFile(item, path, active.cwd)) ?? path;
      setOpenedFile({ sessionId: active.id, path: match });
    },
    [active, producedPaths],
  );

  useEffect(() => {
    if (!active || sessionBusy || !active.fastMode || supportsFastMode(active.model)) return;
    chooseFastMode(false);
  }, [active?.id, active?.model, active?.fastMode, sessionBusy]);

  const bubbles: Bubble[] = useMemo(() => {
    const turns = active?.turns ?? [];
    const toBubble = (turn: GuiTurn): Bubble => {
      const shown = presentTurn(turn);
      return {
        id: turn.id,
        role: turn.role,
        text: shown.text,
        costUsd: shown.costUsd,
        finishedAt: turn.finishedAt,
        events: turn.events ?? [],
      };
    };
    const settled = turns.filter((turn) => turn.status !== "pending").map(toBubble);
    if (draft && draft.sessionId === active?.id) {
      settled.push({
        id: "draft-user",
        role: "user",
        text: draft.text,
        costUsd: null,
        finishedAt: null,
        events: [],
      });
    }
    for (const turn of turns) {
      if (turn.status === "pending" && (turn.events?.length ?? 0) > 0) {
        settled.push(toBubble(turn));
      }
    }
    return settled;
  }, [active, draft]);

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
      model: null,
      allowedToolsPreset: settings.allowedToolsPreset,
    });
    focusId.current = session.id;
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
                    <span className="flex items-center gap-1.5">
                      {busyIds.has(session.id) && (
                        <span className="size-1.5 shrink-0 rounded-full bg-primary" aria-hidden />
                      )}
                      <span className="block truncate text-[13px]">{session.title}</span>
                    </span>
                    <span className="block truncate font-mono text-[10px] text-muted-foreground">
                      {busyIds.has(session.id) ? t("busyChat") : session.cwd}
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
                {active && (
                  <Button
                    type="button"
                    variant={libraryShown ? "secondary" : "outline"}
                    size="sm"
                    aria-pressed={libraryShown}
                    aria-label={libraryShown ? t("closeFilePanel") : t("showProjectFolder")}
                    onClick={() => {
                      if (libraryShown) closeProject();
                      else {
                        setProjectOpen(true);
                        setTreeOpenFor(active.id);
                        setDismissedFiles(null);
                      }
                    }}
                  >
                    <Folder className="size-3.5" />
                    {t("projectLibrary")}
                  </Button>
                )}
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

            <div className="relative flex min-h-0 flex-1">
            <div className="flex min-w-0 flex-1 flex-col">
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
                  {bubbles.length === 0 && !sessionBusy && (
                    <p className="mx-auto max-w-xl px-6 pt-8 text-[13px] leading-relaxed text-muted-foreground">
                      {active.cliSessionId ? t("resumedEmpty") : t("emptyThread")}
                    </p>
                  )}
                  <ChatThread
                    bubbles={bubbles}
                    thinking={sessionBusy}
                    cwd={active.cwd}
                    sessionId={active.id}
                    onOpenFile={openProjectFile}
                  />
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
              <div className="mx-auto mb-2 flex max-w-3xl flex-wrap items-center gap-x-3 gap-y-2">
                <label htmlFor="composer-model" className="font-mono text-[10px] uppercase tracking-wide text-muted-foreground">
                  {t("modelOptional")}
                </label>
                <ModelSelect
                  id="composer-model"
                  value={active?.model ?? null}
                  models={catalog.models}
                  disabled={!active || sessionBusy}
                  className="h-8 w-[220px] rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                  onChange={chooseModel}
                />
                <label htmlFor="composer-effort" className="font-mono text-[10px] uppercase tracking-wide text-muted-foreground">
                  {t("effort")}
                </label>
                <EffortSelect
                  id="composer-effort"
                  value={active?.effort ?? null}
                  disabled={!active || sessionBusy}
                  className="h-8 w-[148px] rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                  onChange={chooseEffort}
                />
                {supportsFastMode(active?.model) && (
                  <label htmlFor="composer-fast-mode" className="inline-flex items-center gap-2 text-[12px] text-muted-foreground">
                    <input
                      id="composer-fast-mode"
                      type="checkbox"
                      className="size-3.5 accent-primary disabled:opacity-50"
                      checked={!!active?.fastMode}
                      disabled={!active || sessionBusy}
                      onChange={(e) => chooseFastMode(e.target.checked)}
                    />
                    {t("fastMode")}
                  </label>
                )}
                <p className="text-[11px] text-muted-foreground">{t("modelHint")}</p>
                <p className="text-[11px] text-muted-foreground">{t("effortHint")}</p>
              </div>
              {active && modelListMessage(catalog, t) && (
                <p className="mx-auto mb-2 max-w-3xl text-[11px] text-muted-foreground">{modelListMessage(catalog, t)}</p>
              )}
              <div className="relative mx-auto flex max-w-3xl items-end gap-2">
                <div className="relative min-w-0 flex-1">
                  {skill && (
                    <div className="mb-1.5 flex">
                      <button
                        type="button"
                        className="inline-flex items-center gap-1 rounded-full border border-border bg-accent px-2 py-0.5 font-mono text-[11px] text-primary"
                        onClick={() => setSkill(null)}
                      >
                        /{skill.name}
                        <span className="text-muted-foreground">{t("removeSkill")}</span>
                      </button>
                    </div>
                  )}
                  {skillMenuOpen && (
                    <div className="absolute bottom-full left-0 z-10 mb-1 max-h-56 w-full overflow-y-auto rounded-md border border-border bg-[#101318] py-1 shadow-lg">
                      {skillMatches.length === 0 ? (
                        <p className="px-3 py-2 text-[12px] text-muted-foreground">{t("skillsNone")}</p>
                      ) : (
                        skillMatches.map((item, index) => (
                          <button
                            key={item.name}
                            type="button"
                            className={cn(
                              "block w-full px-3 py-1.5 text-left hover:bg-accent",
                              index === skillIndex && "bg-accent",
                            )}
                            onMouseEnter={() => setSkillIndex(index)}
                            onClick={() => pickSkill(item)}
                          >
                            <span className="font-mono text-[13px]">/{item.name}</span>
                            {item.description && (
                              <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">{item.description}</span>
                            )}
                          </button>
                        ))
                      )}
                    </div>
                  )}
                  <Textarea
                    value={prompt}
                    placeholder={active ? t("placeholderActive") : t("placeholderIdle")}
                    disabled={sessionBusy || !!readyHint}
                    onChange={(e) => {
                      setPrompt(e.target.value);
                      setSlashDismissed(false);
                    }}
                    onKeyDown={(e) => {
                      if (skillMenuOpen && skillMatches.length > 0) {
                        if (e.key === "ArrowDown") {
                          e.preventDefault();
                          setSkillIndex((index) => Math.min(skillMatches.length - 1, index + 1));
                          return;
                        }
                        if (e.key === "ArrowUp") {
                          e.preventDefault();
                          setSkillIndex((index) => Math.max(0, index - 1));
                          return;
                        }
                        if ((e.key === "Enter" || e.key === "Tab") && !e.shiftKey) {
                          e.preventDefault();
                          const next = skillMatches[skillIndex] ?? skillMatches[0];
                          if (next) pickSkill(next);
                          return;
                        }
                      }
                      if (skillMenuOpen && e.key === "Escape") {
                        e.preventDefault();
                        setSlashDismissed(true);
                        return;
                      }
                      if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                        e.preventDefault();
                        void handleSend();
                      }
                    }}
                  />
                </div>
                <div className="flex flex-col gap-2">
                  {sessionBusy ? (
                    <Button
                      type="button"
                      variant="destructive"
                      onClick={() => {
                        if (active) void api.cancelSession(active.id);
                      }}
                    >
                      {t("cancel")}
                    </Button>
                  ) : (
                    <Button type="submit" disabled={(!prompt.trim() && !skill) || !!readyHint}>
                      {t("send")}
                    </Button>
                  )}
                </div>
              </div>
              <p className="mx-auto mt-2 max-w-3xl font-mono text-[10px] text-muted-foreground">
                {t("composerHint")}
              </p>
            </form>
            </div>
            {panelOpen && active && (
              <ProjectPanel
                key={active.cwd}
                sessionId={active.id}
                cwd={active.cwd}
                activePath={openedPath}
                onActivePath={choosePanelPath}
                onClose={closeProject}
              />
            )}
            </div>
          </>
        )}
      </section>

      <NewChatDialog
        open={showNewChat}
        onOpenChange={setShowNewChat}
        defaultCwd={settings.defaultCwd}
        defaultTools={settings.allowedToolsPreset}
        onCreate={async ({ cwd, tools }) => {
          const session = await api.createSession({
            cwd,
            model: null,
            allowedToolsPreset: tools,
            title: t("newChat"),
          });
          if (settings.defaultCwd !== cwd) {
            const next = { ...settings, defaultCwd: cwd };
            await api.saveSettings(next);
            setSettings(next);
          }
          focusId.current = session.id;
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
