import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type {
  AppSettings,
  ClaudeDetectResult,
  GuiSession,
  KeyStatus,
  SessionSummary,
} from "./types";
import "./App.css";

type View = "chat" | "settings";

const DEFAULT_SETTINGS: AppSettings = {
  claudeBinaryPath: null,
  defaultCwd: null,
  model: null,
  timeoutSecs: 1800,
  allowedToolsPreset: "read",
};

export default function App() {
  const [view, setView] = useState<View>("chat");
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [keyStatus, setKeyStatus] = useState<KeyStatus>({
    configured: false,
    masked: null,
  });
  const [detect, setDetect] = useState<ClaudeDetectResult | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [active, setActive] = useState<GuiSession | null>(null);
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showNewChat, setShowNewChat] = useState(false);
  const [bootstrapped, setBootstrapped] = useState(false);
  const chatEndRef = useRef<HTMLDivElement>(null);

  const refreshSessions = useCallback(async () => {
    const list = await api.listSessions();
    setSessions(list);
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
      if (!k.configured || !d.found) {
        setView("settings");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBootstrapped(true);
    }
  }, [refreshSessions]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [active?.turns, busy]);

  const selectSession = async (id: string) => {
    setError(null);
    setView("chat");
    const session = await api.getSession(id);
    setActive(session);
  };

  const handleSend = async () => {
    const text = prompt.trim();
    if (!text || busy) return;
    setBusy(true);
    setError(null);
    setPrompt("");
    try {
      const res = await api.sendMessage({
        sessionId: active?.id ?? null,
        prompt: text,
        cwd: active?.cwd ?? settings.defaultCwd,
        model: active?.model ?? settings.model,
        allowedToolsPreset:
          active?.allowedToolsPreset ?? settings.allowedToolsPreset,
      });
      setActive(res.session);
      await refreshSessions();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleCancel = async () => {
    try {
      await api.cancelActive();
    } catch (e) {
      setError(String(e));
    }
  };

  const readyHint = useMemo(() => {
    if (!keyStatus.configured) return "API key required";
    if (!detect?.found) return "claude binary not found";
    return null;
  }, [keyStatus, detect]);

  if (!bootstrapped) {
    return (
      <div className="app" style={{ placeItems: "center", display: "grid" }}>
        <p className="status-line">Loading…</p>
      </div>
    );
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <h1>Claude GUI</h1>
          <p>Claude Code CLI · local `-p` wrapper</p>
        </div>
        <div className="sidebar-actions">
          <button
            className="btn btn-primary"
            style={{ flex: 1 }}
            onClick={() => setShowNewChat(true)}
          >
            New chat
          </button>
          <button className="btn" onClick={() => setView("settings")}>
            Settings
          </button>
        </div>
        <div className="session-list">
          {sessions.length === 0 ? (
            <p className="status-line" style={{ padding: "8px 10px" }}>
              No sessions yet.
            </p>
          ) : (
            sessions.map((s) => (
              <button
                key={s.id}
                className={`session-item ${active?.id === s.id ? "active" : ""}`}
                onClick={() => void selectSession(s.id)}
              >
                <span className="title">{s.title}</span>
                <span className="meta">{s.cwd}</span>
              </button>
            ))
          )}
        </div>
      </aside>

      <section className="main">
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
            onKeyStatus={setKeyStatus}
            onDetect={setDetect}
          />
        ) : (
          <>
            <div className="topbar">
              <div>
                <h2>{active?.title ?? "Chat"}</h2>
                {active && (
                  <div className="sub">
                    {active.cwd}
                    {active.cliSessionId
                      ? ` · resume ${active.cliSessionId.slice(0, 8)}…`
                      : " · new session"}
                  </div>
                )}
              </div>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <span
                  className={`badge ${keyStatus.configured && detect?.found ? "ok" : "warn"}`}
                >
                  {keyStatus.configured ? "key ok" : "no key"} ·{" "}
                  {detect?.found ? "cli ok" : "cli missing"}
                </span>
                <span className="badge">
                  tools: {active?.allowedToolsPreset ?? settings.allowedToolsPreset}
                </span>
              </div>
            </div>

            <div className="chat">
              {!active ? (
                <div className="empty">
                  <h3>Start a chat</h3>
                  <p>
                    Pick a project folder, send a message, and the app calls{" "}
                    <code>claude -p … --output-format json</code>. Follow-ups use{" "}
                    <code>--resume</code>.
                  </p>
                  <button
                    className="btn btn-primary"
                    style={{ marginTop: 12 }}
                    onClick={() => setShowNewChat(true)}
                  >
                    New chat
                  </button>
                </div>
              ) : (
                active.turns.map((t) => (
                  <div
                    key={t.id}
                    className={`bubble ${t.role} ${t.status === "pending" ? "pending" : ""}`}
                  >
                    <span className="role">{t.role}</span>
                    {t.status === "pending" && !t.content
                      ? "Waiting for Claude Code…"
                      : t.content}
                  </div>
                ))
              )}
              <div ref={chatEndRef} />
            </div>

            <div className="composer">
              {error && <div className="status-line error">{error}</div>}
              {readyHint && !error && (
                <div className="status-line error">
                  {readyHint} — open Settings to finish setup.
                </div>
              )}
              <div className="composer-row">
                <textarea
                  value={prompt}
                  placeholder={
                    active
                      ? "Message Claude Code…"
                      : "Create a new chat first, or type after picking a folder…"
                  }
                  onChange={(e) => setPrompt(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                      e.preventDefault();
                      void handleSend();
                    }
                  }}
                  disabled={busy || !!readyHint}
                />
                <div className="composer-actions">
                  {busy ? (
                    <button className="btn btn-danger" onClick={() => void handleCancel()}>
                      Cancel
                    </button>
                  ) : (
                    <button
                      className="btn btn-primary"
                      disabled={!prompt.trim() || !!readyHint || (!active && !settings.defaultCwd)}
                      onClick={() => {
                        if (!active) {
                          setShowNewChat(true);
                          return;
                        }
                        void handleSend();
                      }}
                    >
                      Send
                    </button>
                  )}
                </div>
              </div>
              <div className="status-line">Ctrl/⌘+Enter to send · no streaming in MVP</div>
            </div>
          </>
        )}
      </section>

      {showNewChat && (
        <NewChatModal
          defaultCwd={settings.defaultCwd}
          defaultModel={settings.model}
          defaultTools={settings.allowedToolsPreset}
          onClose={() => setShowNewChat(false)}
          onCreate={async ({ cwd, model, tools }) => {
            const session = await api.createSession({
              cwd,
              model,
              allowedToolsPreset: tools,
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
      )}
    </div>
  );
}

function NewChatModal({
  defaultCwd,
  defaultModel,
  defaultTools,
  onClose,
  onCreate,
}: {
  defaultCwd: string | null;
  defaultModel: string | null;
  defaultTools: string;
  onClose: () => void;
  onCreate: (args: {
    cwd: string;
    model: string | null;
    tools: string;
  }) => Promise<void>;
}) {
  const [cwd, setCwd] = useState(defaultCwd ?? "");
  const [model, setModel] = useState(defaultModel ?? "");
  const [tools, setTools] = useState(defaultTools || "read");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const pick = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") setCwd(selected);
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>New chat</h3>
        <p className="hint">
          Working directory becomes the Claude Code cwd. Tools default to
          read-only for MVP.
        </p>
        <div className="field">
          <label>Project folder (cwd)</label>
          <div className="field-row">
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="/path/to/project" />
            <button className="btn" type="button" onClick={() => void pick()}>
              Browse
            </button>
          </div>
        </div>
        <div className="field">
          <label>Model (optional)</label>
          <input
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="Leave blank for CLI default"
          />
        </div>
        <div className="field">
          <label>Allowed tools</label>
          <select value={tools} onChange={(e) => setTools(e.target.value)}>
            <option value="read">Read only (safer)</option>
            <option value="read_edit">Read + Edit</option>
            <option value="full">Unrestricted (opt-in)</option>
          </select>
        </div>
        {err && <div className="status-line error">{err}</div>}
        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose}>
            Cancel
          </button>
          <button
            className="btn btn-primary"
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
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

function SettingsView({
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
  const [draft, setDraft] = useState(settings);
  const [keyInput, setKeyInput] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [statusKind, setStatusKind] = useState<"ok" | "error" | null>(null);
  const [busy, setBusy] = useState(false);
  const [dataDir, setDataDir] = useState<string>("");

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

  return (
    <>
      <div className="topbar">
        <div>
          <h2>Settings</h2>
          <div className="sub">Keys stay in OS secure storage · never logged</div>
        </div>
        <button className="btn" onClick={onClose}>
          Back
        </button>
      </div>
      <div className="chat" style={{ maxWidth: 640 }}>
        <div className="field">
          <label>Anthropic API key</label>
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={
              keyStatus.configured
                ? `Saved: ${keyStatus.masked}`
                : "Paste sk-ant-… then Save"
            }
            autoComplete="off"
          />
          <div className="field-row" style={{ marginTop: 8 }}>
            <button
              className="btn btn-primary"
              disabled={!keyInput.trim() || busy}
              onClick={() => {
                setBusy(true);
                void api
                  .saveApiKey(keyInput)
                  .then((k) => {
                    onKeyStatus(k);
                    setKeyInput("");
                    setStatus("API key saved to OS keychain / credential store");
                    setStatusKind("ok");
                  })
                  .catch((e) => {
                    setStatus(String(e));
                    setStatusKind("error");
                  })
                  .finally(() => setBusy(false));
              }}
            >
              Save key
            </button>
            <button
              className="btn btn-danger"
              disabled={!keyStatus.configured || busy}
              onClick={() => {
                setBusy(true);
                void api
                  .deleteApiKey()
                  .then((k) => {
                    onKeyStatus(k);
                    setStatus("API key deleted");
                    setStatusKind("ok");
                  })
                  .catch((e) => {
                    setStatus(String(e));
                    setStatusKind("error");
                  })
                  .finally(() => setBusy(false));
              }}
            >
              Delete key
            </button>
            <button
              className="btn"
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
              Test connection
            </button>
          </div>
        </div>

        <div className="field">
          <label>claude binary path</label>
          <div className="field-row">
            <input
              value={draft.claudeBinaryPath ?? ""}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  claudeBinaryPath: e.target.value || null,
                })
              }
              placeholder="Auto-detect from PATH if empty"
            />
            <button className="btn" onClick={() => void pickBinary()}>
              Browse
            </button>
            <button
              className="btn"
              onClick={() => {
                void api.detectClaude().then((d) => {
                  onDetect(d);
                  setStatus(
                    d.found
                      ? `Found: ${d.path}${d.version ? ` (${d.version})` : ""}`
                      : d.error ?? "Not found",
                  );
                  setStatusKind(d.found ? "ok" : "error");
                });
              }}
            >
              Detect
            </button>
          </div>
          {detect && (
            <div className={`status-line ${detect.found ? "ok" : "error"}`}>
              {detect.found
                ? `Detected ${detect.path}${detect.version ? ` · ${detect.version}` : ""}`
                : detect.error}
            </div>
          )}
        </div>

        <div className="field">
          <label>Default project folder</label>
          <div className="field-row">
            <input
              value={draft.defaultCwd ?? ""}
              onChange={(e) =>
                setDraft({ ...draft, defaultCwd: e.target.value || null })
              }
            />
            <button className="btn" onClick={() => void pickCwd()}>
              Browse
            </button>
          </div>
        </div>

        <div className="field">
          <label>Default model (optional)</label>
          <input
            value={draft.model ?? ""}
            onChange={(e) =>
              setDraft({ ...draft, model: e.target.value || null })
            }
            placeholder="CLI default"
          />
        </div>

        <div className="field">
          <label>Default allowed tools</label>
          <select
            value={draft.allowedToolsPreset}
            onChange={(e) =>
              setDraft({ ...draft, allowedToolsPreset: e.target.value })
            }
          >
            <option value="read">Read only (safer)</option>
            <option value="read_edit">Read + Edit</option>
            <option value="full">Unrestricted</option>
          </select>
        </div>

        <div className="field">
          <label>Timeout (seconds)</label>
          <input
            type="number"
            min={30}
            value={draft.timeoutSecs}
            onChange={(e) =>
              setDraft({
                ...draft,
                timeoutSecs: Number(e.target.value) || 1800,
              })
            }
          />
        </div>

        {dataDir && (
          <p className="status-line">Local history: {dataDir}</p>
        )}
        {status && (
          <div className={`status-line ${statusKind ?? ""}`}>{status}</div>
        )}

        <div className="modal-actions" style={{ justifyContent: "flex-start" }}>
          <button
            className="btn btn-primary"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void api
                .saveSettings(draft)
                .then(async (s) => {
                  onSettings(s);
                  setStatus("Settings saved");
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
            Save settings
          </button>
        </div>
      </div>
    </>
  );
}
