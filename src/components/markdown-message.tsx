import { useEffect, useMemo, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "@/api";
import { pathCandidates, projectFilePath, remarkOpenablePaths } from "@/answer-paths";
import { sameProjectFile } from "@/produced-files";

export function MarkdownMessage({
  text,
  cwd,
  sessionId,
  producedPaths,
  onOpenFile,
}: {
  text: string;
  cwd?: string;
  sessionId?: string;
  producedPaths?: string[];
  onOpenFile?: (path: string) => void;
}) {
  const candidates = useMemo(() => (onOpenFile ? pathCandidates(text) : []), [onOpenFile, text]);
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
    () => (onOpenFile ? [remarkGfm, remarkOpenablePaths(openable)] : [remarkGfm]),
    [onOpenFile, openable],
  );
  const components = useMemo<Components>(
    () => ({
      a: ({ href, children }) => {
        const path = href ? projectFilePath(href) : null;
        if (path && onOpenFile) {
          return (
            <button type="button" className="file-link" onClick={() => onOpenFile(path)}>
              {children}
            </button>
          );
        }
        return (
          <a href={href} target="_blank" rel="noreferrer noopener">
            {children}
          </a>
        );
      },
    }),
    [onOpenFile],
  );

  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={plugins} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
