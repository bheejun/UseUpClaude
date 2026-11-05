import { useEffect, useMemo, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "@/api";
import { isExternalWebLink, pathCandidates, remarkOpenablePaths, resolveFileHref } from "@/answer-paths";
import { sameProjectFile } from "@/produced-files";

export function MarkdownMessage({
  text,
  cwd,
  sessionId,
  producedPaths,
  linkBase,
  onOpenFile,
}: {
  text: string;
  cwd?: string;
  sessionId?: string;
  producedPaths?: string[];
  /** Open file, so links inside it resolve next to that file. */
  linkBase?: string;
  onOpenFile?: (path: string) => void;
}) {
  const discover = producedPaths != null && Boolean(onOpenFile);
  const candidates = useMemo(() => (discover ? pathCandidates(text) : []), [discover, text]);
  const producedKey = (producedPaths ?? []).join("\0");
  const producedHits = useMemo(
    () =>
      candidates.filter((candidate) =>
        (producedPaths ?? []).some((path) => sameProjectFile(candidate, path, cwd ?? "")),
      ),
    [candidates, cwd, producedKey, producedPaths],
  );
  const candidateKey = candidates.join("\0");
  const [resolved, setResolved] = useState<string[]>([]);

  useEffect(() => {
    if (!onOpenFile || !sessionId || candidates.length === 0) {
      setResolved([]);
      return;
    }
    let cancelled = false;
    void api
      .locateProjectFiles(sessionId, candidates)
      .then((paths) => {
        if (!cancelled) setResolved(paths);
      })
      .catch(() => {
        if (!cancelled) setResolved([]);
      });
    return () => {
      cancelled = true;
    };
  }, [onOpenFile, sessionId, candidateKey, candidates]);

  const openable = useMemo(() => new Set([...producedHits, ...resolved]), [producedHits, resolved]);
  const plugins = useMemo(
    () => (discover ? [remarkGfm, remarkOpenablePaths(openable)] : [remarkGfm]),
    [discover, openable],
  );
  const components = useMemo<Components>(
    () => ({
      a: ({ href, children }) => {
        if (href && isExternalWebLink(href)) {
          return (
            <a href={href} target="_blank" rel="noreferrer noopener">
              {children}
            </a>
          );
        }
        const path = href ? resolveFileHref(href, linkBase) : null;
        if (path && onOpenFile) {
          return (
            <button type="button" className="file-link" onClick={() => onOpenFile(path)}>
              {children}
            </button>
          );
        }
        return <span>{children}</span>;
      },
    }),
    [linkBase, onOpenFile],
  );

  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={plugins} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
