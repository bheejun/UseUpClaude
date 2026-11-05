import { listen } from "@tauri-apps/api/event";
import { ChevronDown, ChevronRight, File, Folder, Plus, Sparkles, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/api";
import { MarkdownMessage } from "@/components/markdown-message";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useI18n, type Locale } from "@/i18n";
import { cn } from "@/lib/utils";
import { folderKey, projectRelativePath, sameProjectFile } from "@/produced-files";
import type { ProjectDirEntry, ProjectFileRef, ProjectFileView, RecentProjectFile } from "@/types";

type Listing = {
  status: "loading" | "ready" | "error";
  entries: ProjectDirEntry[];
  truncated: boolean;
};

type Page = "project" | "file";
type Mode = "preview" | "source";

const INTL: Record<Locale, string> = {
  ko: "ko",
  en: "en",
  ja: "ja",
  zh: "zh-CN",
  es: "es",
};

function fileName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

function parentDir(path: string, cwd: string): string {
  const rel = projectRelativePath(path, cwd).replace(/\\/g, "/");
  const parts = rel.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

function isMarkdownName(name: string): boolean {
  const lower = name.toLowerCase();
  return lower.endsWith(".md") || lower.endsWith(".markdown") || lower.endsWith(".mdx") || lower.endsWith(".mdown");
}

function FileGlyph({ name, className }: { name: string; className?: string }) {
  return isMarkdownName(name) ? <Sparkles className={className} /> : <File className={className} />;
}

function formatModified(ms: number, locale: Locale, t: ReturnType<typeof useI18n>["t"]): string {
  if (!ms) return "";
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  const time = new Intl.DateTimeFormat(INTL[locale], { hour: "numeric", minute: "2-digit" }).format(date);
  const startToday = new Date();
  startToday.setHours(0, 0, 0, 0);
  const startThat = new Date(date);
  startThat.setHours(0, 0, 0, 0);
  const days = Math.round((startToday.getTime() - startThat.getTime()) / 86_400_000);
  if (days === 0) return t("modifiedToday", { time });
  if (days === 1) return t("modifiedYesterday", { time });
  return new Intl.DateTimeFormat(INTL[locale], {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

export function LibraryView({
  sessionId,
  cwd,
  openedPath,
  onActivePath,
}: {
  sessionId: string;
  cwd: string;
  openedPath: string | null;
  onActivePath: (path: string | null) => void;
}) {
  const { t, locale } = useI18n();
  const [tabs, setTabs] = useState<string[]>([]);
  const [page, setPage] = useState<Page>("project");
  const [current, setCurrent] = useState<string | null>(null);
  const [picker, setPicker] = useState(false);
  const [recents, setRecents] = useState<RecentProjectFile[]>([]);
  const [recentsStatus, setRecentsStatus] = useState<"loading" | "ready" | "error">("loading");
  const [listings, setListings] = useState<Record<string, Listing>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([""]));
  const requested = useRef(new Set<string>());
  const [view, setView] = useState<ProjectFileView | null>(null);
  const [loading, setLoading] = useState(false);
  const [missing, setMissing] = useState(false);
  const [mode, setMode] = useState<Mode>("preview");
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const modes = useRef(new Map<string, Mode>());
  const drafts = useRef(new Map<string, string>());
  const activePath = page === "file" ? current : null;

  const loadDir = useCallback(
    (rel: string, force = false) => {
      if (!force && requested.current.has(rel)) return;
      requested.current.add(rel);
      setListings((existing) => ({
        ...existing,
        [rel]: { status: "loading", entries: existing[rel]?.entries ?? [], truncated: false },
      }));
      void api
        .listProjectDir(sessionId, rel)
        .then((listing) => {
          setListings((existing) => ({
            ...existing,
            [rel]: { status: "ready", entries: listing.entries, truncated: listing.truncated },
          }));
        })
        .catch(() => {
          setListings((existing) => ({
            ...existing,
            [rel]: { status: "error", entries: [], truncated: false },
          }));
        });
    },
    [sessionId],
  );

  const loadRecents = useCallback(() => {
    setRecentsStatus("loading");
    void api
      .recentProjectFiles(sessionId)
      .then((rows) => {
        // Cursor Recents are content preview cards — markdown documents only.
        setRecents(rows.filter((file) => file.kind === "markdown"));
        setRecentsStatus("ready");
      })
      .catch(() => {
        setRecentsStatus("error");
      });
  }, [sessionId]);

  const refresh = useCallback(() => {
    requested.current.delete("");
    loadDir("", true);
    loadRecents();
  }, [loadDir, loadRecents]);

  useEffect(() => {
    requested.current.clear();
    setListings({});
    setExpanded(new Set([""]));
    loadDir("");
    loadRecents();
  }, [loadDir, loadRecents]);

  useEffect(() => {
    const key = folderKey(cwd);
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<{ cwd: string }>("project-workspace", (event) => {
      if (folderKey(event.payload.cwd) !== key) return;
      refresh();
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [cwd, refresh]);

  const openFile = useCallback(
    (path: string) => {
      setTabs((existing) =>
        existing.some((item) => sameProjectFile(item, path, cwd)) ? existing : [...existing, path],
      );
      setCurrent(path);
      setPage("file");
      onActivePath(path);
    },
    [cwd, onActivePath],
  );

  const showProject = () => {
    setPage("project");
    onActivePath(null);
  };

  const closeTab = (path: string) => {
    const next = tabs.filter((item) => !sameProjectFile(item, path, cwd));
    setTabs(next);
    if (!current || !sameProjectFile(current, path, cwd)) return;
    const fallback = next[next.length - 1] ?? null;
    setCurrent(fallback);
    if (page !== "file") return;
    if (fallback) onActivePath(fallback);
    else {
      setPage("project");
      onActivePath(null);
    }
  };

  useEffect(() => {
    if (!openedPath) return;
    setTabs((existing) =>
      existing.some((item) => sameProjectFile(item, openedPath, cwd)) ? existing : [...existing, openedPath],
    );
    setCurrent(openedPath);
    setPage("file");
  }, [openedPath, cwd]);

  useEffect(() => {
    if (!current) return;
    const rel = projectRelativePath(current, cwd).replace(/\\/g, "/");
    if (!rel || rel.includes(":") || rel.startsWith("/") || rel.split("/").includes("..")) return;
    const parts = rel.split("/").filter(Boolean);
    const parents = [""];
    for (let index = 0; index < parts.length - 1; index += 1) {
      parents.push(parts.slice(0, index + 1).join("/"));
    }
    setExpanded((existing) => {
      const next = new Set(existing);
      for (const parent of parents) next.add(parent);
      return next;
    });
    for (const parent of parents) loadDir(parent);
  }, [current, cwd, loadDir]);

  useEffect(() => {
    if (!activePath) {
      setView(null);
      setMissing(false);
      setLoading(false);
      setSaveError(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setMissing(false);
    setSaveError(false);
    void api
      .readProjectFile(sessionId, activePath)
      .then((next) => {
        if (cancelled) return;
        setView(next);
        const edited = drafts.current.get(activePath);
        const text = edited ?? next.content ?? "";
        setDraft(text);
        setDirty(edited != null && edited !== (next.content ?? ""));
        const remembered = modes.current.get(activePath);
        setMode(remembered ?? (next.kind === "markdown" ? "preview" : "source"));
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

  const chooseMode = (next: Mode) => {
    setMode(next);
    if (activePath) modes.current.set(activePath, next);
  };

  const editable = Boolean(view && !view.truncated && view.content != null && view.kind !== "unsupported");
  const markdown = Boolean(view && view.kind === "markdown" && view.content != null);

  const save = () => {
    if (!activePath || !editable || saving) return;
    setSaving(true);
    setSaveError(false);
    const path = activePath;
    void api
      .writeProjectFile(sessionId, path, draft)
      .then(() => {
        drafts.current.delete(path);
        setView((existing) => (existing ? { ...existing, content: draft, truncated: false } : existing));
        setDirty(false);
        loadDir(parentDir(path, cwd), true);
        loadRecents();
      })
      .catch(() => setSaveError(true))
      .finally(() => setSaving(false));
  };

  const onDraft = (value: string) => {
    setDraft(value);
    if (activePath) drafts.current.set(activePath, value);
    setDirty(value !== (view?.content ?? ""));
  };

  const toggleDir = (path: string) => {
    setExpanded((existing) => {
      const next = new Set(existing);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    loadDir(path);
  };

  const rootOpen = expanded.has("");
  const rootName = fileName(cwd) || cwd;
  const formatTime = (ms: number) => formatModified(ms, locale, t);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        role="tablist"
        aria-label={t("projectLibrary")}
        className="flex min-w-0 items-center gap-0.5 overflow-x-auto border-b border-border px-1.5"
      >
        <button
          type="button"
          role="tab"
          aria-selected={page === "project"}
          className={cn(
            "inline-flex h-8 max-w-[9rem] shrink-0 items-center gap-1.5 border-b-2 px-2 text-[12px]",
            page === "project"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground",
          )}
          onClick={showProject}
        >
          <Folder className="size-3.5 shrink-0" />
          <span className="truncate">{t("projectTab")}</span>
        </button>
        {tabs.map((path) => {
          const name = fileName(path);
          const selected = page === "file" && current != null && sameProjectFile(path, current, cwd);
          return (
            <div
              key={path}
              className={cn(
                "flex h-8 min-w-0 max-w-[12rem] shrink-0 items-center border-b-2",
                selected ? "border-primary" : "border-transparent",
              )}
            >
              <button
                type="button"
                role="tab"
                aria-selected={selected}
                title={path}
                className={cn(
                  "inline-flex min-w-0 items-center gap-1.5 px-2 text-[12px]",
                  selected ? "text-foreground" : "text-muted-foreground hover:text-foreground",
                )}
                onClick={() => openFile(path)}
              >
                <FileGlyph name={name} className="size-3.5 shrink-0" />
                <span className="truncate">{name}</span>
              </button>
              <button
                type="button"
                className="mr-1 inline-flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
                aria-label={t("closeFileTab")}
                onClick={() => closeTab(path)}
              >
                <X className="size-3" />
              </button>
            </div>
          );
        })}
        <button
          type="button"
          className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          aria-label={t("openAnotherFile")}
          onClick={() => setPicker(true)}
        >
          <Plus className="size-3.5" />
        </button>
      </div>

      {page === "project" ? (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <section className="px-3 pt-3" aria-label={t("recents")}>
            <h4 className="text-[13px] text-muted-foreground">{t("recents")}</h4>
            {recentsStatus === "error" && (
              <p className="mt-2 text-[12px] text-muted-foreground">{t("folderReadFailed")}</p>
            )}
            {recentsStatus !== "error" && recents.length === 0 && (
              <p className="mt-2 text-[12px] text-muted-foreground">
                {recentsStatus === "loading" ? t("readingFolder") : t("noRecentFiles")}
              </p>
            )}
            {recents.length > 0 && (
              <div className="mt-2 flex gap-2 overflow-x-auto pb-2">
                {recents.map((file) => (
                  <button
                    key={file.path}
                    type="button"
                    title={file.path}
                    className="flex w-44 shrink-0 flex-col overflow-hidden rounded-md border border-border bg-[#0c0e11] text-left hover:border-primary/40"
                    onClick={() => openFile(file.path)}
                  >
                    <div className="pointer-events-none h-36 overflow-hidden px-2 py-1.5">
                      <MarkdownMessage text={file.preview} />
                    </div>
                    <span className="truncate border-t border-border px-2 py-1.5 text-[11px] text-muted-foreground">
                      {file.name}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </section>
          {tabs.length > 0 && (
            <div className="flex gap-3 overflow-x-auto border-y border-border px-3 py-1.5">
              {tabs.map((path) => {
                const name = fileName(path);
                const underlined = current != null && sameProjectFile(path, current, cwd);
                return (
                  <button
                    key={path}
                    type="button"
                    title={path}
                    className={cn(
                      "max-w-[10rem] shrink-0 truncate text-[12px]",
                      underlined
                        ? "text-foreground underline underline-offset-4"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                    onClick={() => openFile(path)}
                  >
                    {name}
                  </button>
                );
              })}
            </div>
          )}
          <section className="pb-3" aria-label={t("allFiles")}>
            <h4 className="px-3 pb-1 pt-3 text-[13px] text-muted-foreground">{t("allFiles")}</h4>
            <div role="tree" aria-label={t("allFiles")}>
              <div role="treeitem" aria-expanded={rootOpen} aria-level={1}>
                <button
                  type="button"
                  className="flex w-full items-center gap-1.5 px-2 py-1 text-left text-[13px] hover:bg-accent"
                  title={cwd}
                  aria-label={`${rootOpen ? t("collapseFolder") : t("expandFolder")}: ${rootName}`}
                  onClick={() => toggleDir("")}
                >
                  {rootOpen ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />}
                  <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate">{rootName}</span>
                </button>
                {rootOpen && (
                  <div role="group">
                    <TreeLevel
                      parent=""
                      depth={1}
                      listings={listings}
                      expanded={expanded}
                      current={current}
                      cwd={cwd}
                      formatTime={formatTime}
                      onToggle={toggleDir}
                      onOpenFile={openFile}
                    />
                  </div>
                )}
              </div>
            </div>
          </section>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <header className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
            <h4 className="min-w-0 truncate font-mono text-[12px]" title={current ?? undefined}>
              {current ? fileName(current) : ""}
            </h4>
            <div className="flex shrink-0 items-center gap-1.5">
              {markdown && (
                <div className="flex rounded-md border border-border p-0.5" role="group" aria-label={t("markdownPreview")}>
                  <button
                    type="button"
                    className={cn(
                      "rounded px-2 py-0.5 text-[12px]",
                      mode === "preview" ? "bg-secondary text-foreground" : "text-muted-foreground hover:text-foreground",
                    )}
                    aria-pressed={mode === "preview"}
                    onClick={() => chooseMode("preview")}
                  >
                    {t("markdownPreview")}
                  </button>
                  <button
                    type="button"
                    className={cn(
                      "rounded px-2 py-0.5 text-[12px]",
                      mode === "source" ? "bg-secondary text-foreground" : "text-muted-foreground hover:text-foreground",
                    )}
                    aria-pressed={mode === "source"}
                    onClick={() => chooseMode("source")}
                  >
                    {t("markdownSource")}
                  </button>
                </div>
              )}
              {editable && (
                <Button type="button" size="sm" disabled={saving || !dirty} onClick={save}>
                  {saving ? t("savingFile") : t("saveFile")}
                </Button>
              )}
            </div>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            {loading && <p className="text-[13px] text-muted-foreground">{t("filePreviewLoading")}</p>}
            {missing && <p className="text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewMissing")}</p>}
            {view?.kind === "unsupported" && (
              <p className="text-[13px] leading-relaxed text-muted-foreground">{t("filePreviewUnsupported")}</p>
            )}
            {saveError && <p className="mb-2 text-[12px] text-destructive">{t("fileSaveFailed")}</p>}
            {view && view.kind !== "unsupported" && view.content != null && (
              <>
                {view.truncated && (
                  <p className="mb-2 text-[12px] text-muted-foreground">{t("filePreviewTruncated")}</p>
                )}
                {markdown && mode === "preview" ? (
                  <MarkdownMessage
                    text={dirty ? draft : view.content}
                    cwd={cwd}
                    sessionId={sessionId}
                    linkBase={activePath ?? undefined}
                    onOpenFile={openFile}
                  />
                ) : view.truncated || !editable ? (
                  <pre className="m-0 whitespace-pre-wrap break-words font-mono text-[12px] leading-relaxed text-foreground">
                    {view.content}
                  </pre>
                ) : (
                  <textarea
                    className="min-h-64 w-full resize-y rounded-md border border-border bg-[#0c0e11] p-2 font-mono text-[12px] leading-relaxed text-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    value={draft}
                    spellCheck={false}
                    aria-label={markdown ? t("markdownSource") : t("editFile")}
                    onChange={(event) => onDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if ((event.metaKey || event.ctrlKey) && event.key === "s") {
                        event.preventDefault();
                        save();
                      }
                    }}
                  />
                )}
              </>
            )}
          </div>
        </div>
      )}
      <OpenFileDialog
        open={picker}
        sessionId={sessionId}
        cwd={cwd}
        onOpenChange={setPicker}
        onOpen={openFile}
      />
    </div>
  );
}

function OpenFileDialog({
  open,
  sessionId,
  cwd,
  onOpenChange,
  onOpen,
}: {
  open: boolean;
  sessionId: string;
  cwd: string;
  onOpenChange: (open: boolean) => void;
  onOpen: (path: string) => void;
}) {
  const { t, locale } = useI18n();
  const [files, setFiles] = useState<ProjectFileRef[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [failed, setFailed] = useState(false);
  const [loadingList, setLoadingList] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setFiles([]);
    setFailed(false);
    setLoadingList(true);
    let cancelled = false;
    void api
      .listProjectFiles(sessionId)
      .then((index) => {
        if (cancelled) return;
        setFiles(index.files);
        setTruncated(index.truncated);
      })
      .catch(() => {
        if (cancelled) return;
        setFiles([]);
        setFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoadingList(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, sessionId]);

  const needle = query.trim().toLowerCase();
  const shown = files.filter((file) => !needle || file.path.toLowerCase().includes(needle)).slice(0, 80);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("openAnotherFile")}</DialogTitle>
          <DialogDescription className="truncate" title={cwd}>
            {cwd}
          </DialogDescription>
        </DialogHeader>
        <Input
          value={query}
          aria-label={t("openAnotherFile")}
          onChange={(event) => setQuery(event.target.value)}
        />
        {failed && <p className="mt-2 text-[13px] text-destructive">{t("filePickerFailed")}</p>}
        {loadingList && <p className="mt-2 text-[13px] text-muted-foreground">{t("readingFolder")}</p>}
        {!loadingList && !failed && shown.length === 0 && (
          <p className="mt-2 text-[13px] text-muted-foreground">{t("filePickerEmpty")}</p>
        )}
        <ul className="mt-2 max-h-80 overflow-y-auto">
          {shown.map((file) => (
            <li key={file.path}>
              <button
                type="button"
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent"
                onClick={() => {
                  onOpen(file.path);
                  onOpenChange(false);
                }}
              >
                <FileGlyph name={file.name} className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate font-mono text-[12px]">{file.path}</span>
                <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                  {formatModified(file.modifiedMs, locale, t)}
                </span>
              </button>
            </li>
          ))}
        </ul>
        {truncated && <p className="mt-2 text-[12px] text-muted-foreground">{t("filePickerTruncated")}</p>}
      </DialogContent>
    </Dialog>
  );
}

function TreeLevel({
  parent,
  depth,
  listings,
  expanded,
  current,
  cwd,
  formatTime,
  onToggle,
  onOpenFile,
}: {
  parent: string;
  depth: number;
  listings: Record<string, Listing>;
  expanded: Set<string>;
  current: string | null;
  cwd: string;
  formatTime: (ms: number) => string;
  onToggle: (path: string) => void;
  onOpenFile: (path: string) => void;
}) {
  const { t } = useI18n();
  const listing = listings[parent];
  const pad = { paddingLeft: 8 + depth * 14 };
  if (!listing || (listing.status === "loading" && listing.entries.length === 0)) {
    return (
      <p className="px-2 py-1 text-[12px] text-muted-foreground" style={pad}>
        {t("readingFolder")}
      </p>
    );
  }
  if (listing.status === "error" && listing.entries.length === 0) {
    return (
      <p className="px-2 py-1 text-[12px] leading-relaxed text-muted-foreground" style={pad}>
        {t("folderReadFailed")}
      </p>
    );
  }
  if (listing.entries.length === 0) {
    return (
      <p className="px-2 py-1 text-[12px] text-muted-foreground" style={pad}>
        {t("emptyFolder")}
      </p>
    );
  }
  return (
    <>
      {listing.entries.map((entry) => {
        const when = formatTime(entry.modifiedMs);
        if (entry.isDir) {
          const open = expanded.has(entry.path);
          return (
            <div key={entry.path} role="treeitem" aria-expanded={open} aria-level={depth + 1}>
              <button
                type="button"
                className="flex w-full items-center gap-1.5 py-1 pr-3 text-left text-[13px] hover:bg-accent"
                style={pad}
                title={entry.path}
                aria-label={`${open ? t("collapseFolder") : t("expandFolder")}: ${entry.name}`}
                onClick={() => onToggle(entry.path)}
              >
                {open ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />}
                <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                {when && (
                  <time className="shrink-0 pl-3 font-mono text-[11px] text-muted-foreground" dateTime={new Date(entry.modifiedMs).toISOString()}>
                    {when}
                  </time>
                )}
              </button>
              {open && (
                <div role="group">
                  <TreeLevel
                    parent={entry.path}
                    depth={depth + 1}
                    listings={listings}
                    expanded={expanded}
                    current={current}
                    cwd={cwd}
                    formatTime={formatTime}
                    onToggle={onToggle}
                    onOpenFile={onOpenFile}
                  />
                </div>
              )}
            </div>
          );
        }
        const selected = current != null && sameProjectFile(entry.path, current, cwd);
        return (
          <div key={entry.path} role="treeitem" aria-level={depth + 1} aria-selected={selected}>
            <button
              type="button"
              className={cn(
                "flex w-full items-center gap-1.5 py-1 pr-3 text-left text-[13px] hover:bg-accent",
                selected ? "bg-accent text-primary" : "text-foreground",
              )}
              style={{ paddingLeft: 8 + depth * 14 + 18 }}
              title={entry.path}
              aria-current={selected ? "true" : undefined}
              onClick={() => onOpenFile(entry.path)}
            >
              <FileGlyph name={entry.name} className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              {when && (
                <time className="shrink-0 pl-3 font-mono text-[11px] text-muted-foreground" dateTime={new Date(entry.modifiedMs).toISOString()}>
                  {when}
                </time>
              )}
            </button>
          </div>
        );
      })}
      {listing.truncated && (
        <p className="px-2 py-1 text-[12px] leading-relaxed text-muted-foreground" style={pad}>
          {t("folderListingTruncated")}
        </p>
      )}
    </>
  );
}
