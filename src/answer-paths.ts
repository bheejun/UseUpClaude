const FILE_URL = "project-file:";

type MdNode = {
  type: string;
  value?: string;
  url?: string;
  children?: MdNode[];
};

export function projectFileUrl(path: string): string {
  return FILE_URL + encodeURIComponent(path);
}

export function projectFilePath(url: string): string | null {
  if (!url.startsWith(FILE_URL)) return null;
  try {
    const path = decodeURIComponent(url.slice(FILE_URL.length)).trim();
    return path || null;
  } catch {
    return null;
  }
}

export function cleanPathToken(raw: string): string {
  let text = raw.trim();
  if (
    (text.startsWith('"') && text.endsWith('"') && text.length >= 2) ||
    (text.startsWith("'") && text.endsWith("'") && text.length >= 2)
  ) {
    text = text.slice(1, -1).trim();
  }
  return text.replace(/[.,:;!?)]+$/u, "");
}

export function looksLikePath(token: string): boolean {
  if (!token || token.length > 4096) return false;
  if (/[\u0000-\u001f]/.test(token)) return false;
  if (token.includes("://") || token.includes("*") || token.includes("?")) return false;
  if (!token.includes("/") && !token.includes("\\")) return false;
  const last = token.split(/[\\/]/).pop() ?? "";
  return last.length > 0 && last !== "." && last !== "..";
}

const PROSE_PATH =
  /(?:[A-Za-z]:[\\/][^\s"'<>|]+|(?:(?:\.\.?[\\/])?(?:[A-Za-z0-9._@()-]+[\\/])+)[A-Za-z0-9._@()-]+)/g;

function inUrl(text: string, index: number): boolean {
  const from = Math.max(0, index - 16);
  return text.slice(from, index + 1).includes("://");
}

/** Path-shaped tokens in an answer. Not every word, and not fenced code. */
export function pathCandidates(markdown: string): string[] {
  const withoutFences = markdown.replace(/```[\s\S]*?```/g, " ");
  const found: string[] = [];
  const seen = new Set<string>();
  const add = (raw: string) => {
    const token = cleanPathToken(raw);
    if (!looksLikePath(token) || seen.has(token)) return;
    seen.add(token);
    found.push(token);
  };
  const withoutInline = withoutFences.replace(/`([^`\n]+)`/g, (_all, inner: string) => {
    add(inner);
    return " ";
  });
  for (const match of withoutInline.matchAll(PROSE_PATH)) {
    const index = match.index ?? 0;
    if (inUrl(withoutInline, index)) continue;
    add(match[0]);
    if (found.length >= 48) break;
  }
  return found.slice(0, 48);
}

function boundary(value: string, start: number, len: number): boolean {
  const prev = start > 0 ? value.charAt(start - 1) : "";
  const next = value.charAt(start + len);
  if (prev && /[0-9A-Za-z_\\/]/.test(prev)) return false;
  if (next && /[0-9A-Za-z_\\/]/.test(next)) return false;
  return true;
}

function splitText(value: string, openable: readonly string[]): MdNode[] {
  const needles = openable.filter((item) => item.length > 0).sort((a, b) => b.length - a.length);
  if (needles.length === 0) return [{ type: "text", value }];
  const nodes: MdNode[] = [];
  let index = 0;
  while (index < value.length) {
    let hitAt = -1;
    let hitToken = "";
    for (const token of needles) {
      const at = value.indexOf(token, index);
      if (at < 0) continue;
      if (hitAt < 0 || at < hitAt || (at === hitAt && token.length > hitToken.length)) {
        hitAt = at;
        hitToken = token;
      }
    }
    if (hitAt < 0) {
      nodes.push({ type: "text", value: value.slice(index) });
      break;
    }
    if (!boundary(value, hitAt, hitToken.length)) {
      const skipTo = hitAt + 1;
      nodes.push({ type: "text", value: value.slice(index, skipTo) });
      index = skipTo;
      continue;
    }
    if (hitAt > index) nodes.push({ type: "text", value: value.slice(index, hitAt) });
    nodes.push({
      type: "link",
      url: projectFileUrl(hitToken),
      children: [{ type: "text", value: hitToken }],
    });
    index = hitAt + hitToken.length;
  }
  return nodes;
}

function rewrite(node: MdNode, openable: ReadonlySet<string>, list: readonly string[]) {
  if (!node.children || node.type === "link" || node.type === "linkReference" || node.type === "code") {
    return;
  }
  const next: MdNode[] = [];
  for (const child of node.children) {
    if (child.type === "inlineCode" && child.value) {
      const token = cleanPathToken(child.value);
      const path = openable.has(child.value) ? child.value : openable.has(token) ? token : null;
      if (path) {
        next.push({
          type: "link",
          url: projectFileUrl(path),
          children: [{ type: "inlineCode", value: child.value }],
        });
        continue;
      }
    }
    if (child.type === "text" && child.value) {
      next.push(...splitText(child.value, list));
      continue;
    }
    rewrite(child, openable, list);
    next.push(child);
  }
  node.children = next;
}

/** Remark plugin: turn openable path strings into project-file links. */
export function remarkOpenablePaths(openable: ReadonlySet<string>): () => (tree: unknown) => void {
  const list = [...openable];
  return () => (tree: unknown) => {
    rewrite(tree as MdNode, openable, list);
  };
}
