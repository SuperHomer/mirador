import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  DiffFile,
  DiffResult,
  ResolvedColors,
  closePane,
  focusPane,
  loadDiff,
  setDiffSpec,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { TreeNode, buildTree } from "./fileTree";

/** Row height, in lockstep with `--diff-row-h` in styles.css. */
const ROW_H = 18;
/**
 * Rows below which the whole diff renders up front. Lazy mounting exists
 * for the 60k-line lockfile, not for an ordinary review — and anything
 * that mounts DOM mid-scroll is a chance to flicker, so an ordinary diff
 * should never do it.
 */
const EAGER_ROW_BUDGET = 4000;

interface Props {
  paneId: string;
  repo: string;
  spec: string;
  focused: boolean;
}

/**
 * Whether a hunk shows its `@@ … @@` row. The synthesized whole-file hunk
 * of an untracked file has neither a range worth printing nor a section
 * header, and a lone "@@ -0 +1 @@" above a new file is just noise.
 */
const showsHeaderRow = (hunk: { header: string; oldStart: number }) =>
  Boolean(hunk.header) || hunk.oldStart > 0;

const STATUS_GLYPH: Record<string, string> = {
  added: "A",
  untracked: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  modified: "M",
};

/**
 * A reviewable diff: file tree on the left, hunks on the right. Unlike the
 * browser pane this is ordinary DOM in the host webview, so the keymap,
 * focus ring and theme all apply without special handling.
 */
export function DiffPane({ paneId, repo, spec, focused }: Props) {
  const [result, setResult] = useState<DiffResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [collapsedFiles, setCollapsedFiles] = useState<Set<string>>(new Set());
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string | null>(null);
  // A diff pane is usually a split, not a window: the tree has to be able
  // to get out of the way.
  const [treeOpen, setTreeOpen] = useState(true);
  const colors = useConfigStore((s) => s.config?.colors);
  const scrollRef = useRef<HTMLDivElement>(null);
  const sections = useRef(new Map<string, HTMLDivElement>());

  const refresh = useCallback(() => {
    setLoading(true);
    let stale = false;
    void loadDiff(paneId)
      .then((r) => {
        if (stale) return;
        setResult(r);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!stale) setError(String(e));
      })
      .finally(() => {
        if (!stale) setLoading(false);
      });
    return () => {
      stale = true;
    };
  }, [paneId]);

  useEffect(() => refresh(), [refresh, spec]);

  // A working-tree diff goes stale the moment anything writes a file, and
  // the usual writer is the agent in the pane next door. Coming back to
  // the pane is the natural "show me where we are now".
  const live = spec === "worktree" || spec === "staged";
  const wasFocused = useRef(focused);
  useEffect(() => {
    if (focused && !wasFocused.current && live) refresh();
    wasFocused.current = focused;
  }, [focused, live, refresh]);

  const tree = useMemo(() => buildTree(result?.files ?? []), [result]);
  const eager = useMemo(
    () =>
      (result?.files ?? []).reduce(
        (n, f) => n + f.hunks.reduce((m, h) => m + h.lines.length, 0),
        0,
      ) <= EAGER_ROW_BUDGET,
    [result],
  );
  const totals = useMemo(() => {
    const files = result?.files ?? [];
    return {
      files: files.length,
      additions: files.reduce((n, f) => n + f.additions, 0),
      deletions: files.reduce((n, f) => n + f.deletions, 0),
    };
  }, [result]);

  const reveal = (path: string) => {
    setSelected(path);
    setCollapsedFiles((prev) => {
      if (!prev.has(path)) return prev;
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
    sections.current.get(path)?.scrollIntoView({ block: "start" });
  };

  const toggleFile = useCallback(
    (path: string) =>
      setCollapsedFiles((prev) => {
        const next = new Set(prev);
        if (!next.delete(path)) next.add(path);
        return next;
      }),
    [],
  );

  const register = useCallback((path: string, el: HTMLDivElement | null) => {
    if (el) sections.current.set(path, el);
    else sections.current.delete(path);
  }, []);

  const toggleDir = (path: string) =>
    setCollapsedDirs((prev) => {
      const next = new Set(prev);
      if (!next.delete(path)) next.add(path);
      return next;
    });

  return (
    <div
      className={`pane diff-pane${focused ? " focused" : ""}`}
      style={diffTheme(colors)}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="diff-chrome">
        <button
          className="diff-tree-toggle"
          title={treeOpen ? "Hide file tree" : "Show file tree"}
          onClick={() => setTreeOpen((open) => !open)}
        >
          ☰
        </button>
        <span className="diff-label" title={repo}>
          {result?.label ?? spec}
        </span>
        <span className="diff-totals">
          {totals.files} {totals.files === 1 ? "file" : "files"}
          <span className="diff-add-text"> +{totals.additions}</span>
          <span className="diff-del-text"> −{totals.deletions}</span>
        </span>
        <div className="diff-specs">
          {(
            [
              ["worktree", "Working tree"],
              ["staged", "Staged"],
            ] as const
          ).map(([id, title]) => (
            <button
              key={id}
              className={spec === id ? "active" : ""}
              onClick={() => void setDiffSpec(paneId, id)}
            >
              {title}
            </button>
          ))}
          {!live && <span className="active">{spec}</span>}
        </div>
        <button
          className="diff-refresh"
          title="Refresh"
          disabled={loading}
          onClick={() => refresh()}
        >
          ⟳
        </button>
        <button
          className="diff-close"
          title="Close diff pane"
          onClick={() => void closePane(paneId)}
        >
          ×
        </button>
      </div>

      {error ? (
        <div className="diff-empty diff-error">{error}</div>
      ) : (
        <div className="diff-body">
          {treeOpen && (
            <div className="diff-tree">
              {tree.map((node) => (
                <TreeRow
                  key={node.path}
                  node={node}
                  depth={0}
                  collapsedDirs={collapsedDirs}
                  selected={selected}
                  onToggleDir={toggleDir}
                  onPick={reveal}
                />
              ))}
            </div>
          )}
          <div className="diff-files" ref={scrollRef} tabIndex={0}>
            {result?.files.map((file) => (
              <FileSection
                key={file.path}
                file={file}
                collapsed={collapsedFiles.has(file.path)}
                selected={selected === file.path}
                scrollRef={scrollRef}
                eager={eager}
                onToggle={toggleFile}
                register={register}
              />
            ))}
            {result && result.files.length === 0 && (
              <div className="diff-empty">
                {loading ? "Loading…" : "No changes"}
              </div>
            )}
            {!result && <div className="diff-empty">Loading…</div>}
          </div>
        </div>
      )}
    </div>
  );
}

function TreeRow({
  node,
  depth,
  collapsedDirs,
  selected,
  onToggleDir,
  onPick,
}: {
  node: TreeNode;
  depth: number;
  collapsedDirs: Set<string>;
  selected: string | null;
  onToggleDir: (path: string) => void;
  onPick: (path: string) => void;
}) {
  const indent = { paddingLeft: `${6 + depth * 12}px` };
  if (node.file) {
    const f = node.file;
    return (
      <button
        className={`diff-tree-file${selected === f.path ? " selected" : ""}`}
        style={indent}
        title={f.path}
        onClick={() => onPick(f.path)}
      >
        <span className={`diff-status s-${f.status}`}>
          {STATUS_GLYPH[f.status] ?? "M"}
        </span>
        <span className="diff-tree-name">{node.name}</span>
        <span className="diff-tree-stat">
          <span className="diff-add-text">+{f.additions}</span>{" "}
          <span className="diff-del-text">−{f.deletions}</span>
        </span>
      </button>
    );
  }
  const open = !collapsedDirs.has(node.path);
  return (
    <>
      <button
        className="diff-tree-dir"
        style={indent}
        onClick={() => onToggleDir(node.path)}
      >
        <span className="diff-caret">{open ? "▾" : "▸"}</span>
        {node.name}
      </button>
      {open &&
        node.children.map((child) => (
          <TreeRow
            key={child.path}
            node={child}
            depth={depth + 1}
            collapsedDirs={collapsedDirs}
            selected={selected}
            onToggleDir={onToggleDir}
            onPick={onPick}
          />
        ))}
    </>
  );
}

/**
 * Memoized: the pane re-renders on every workspace snapshot (a couple of
 * times a second while an agent works), and without this each one would
 * reconcile every mounted diff row — thousands of divs — which shows up
 * as a stutter in whichever file you are scrolling through.
 */
const FileSection = memo(function FileSection({
  file,
  collapsed,
  selected,
  scrollRef,
  eager,
  onToggle,
  register,
}: {
  file: DiffFile;
  collapsed: boolean;
  selected: boolean;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  eager: boolean;
  onToggle: (path: string) => void;
  register: (path: string, el: HTMLDivElement | null) => void;
}) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const [mounted, setMounted] = useState(false);

  // Rows are fixed-height, so an unmounted body can reserve its exact
  // height: scrolling past a 3000-line file costs one div, and the
  // scrollbar never jumps when the real rows arrive.
  const rows = useMemo(
    () =>
      file.hunks.reduce(
        (n, h) => n + h.lines.length + (showsHeaderRow(h) ? 1 : 0),
        0,
      ),
    [file],
  );

  // Only the oversized diffs get here, and the latch is one-way: a body
  // that has mounted stays mounted, so nothing ever shrinks back to the
  // reserved height behind the reader.
  useEffect(() => {
    if (eager || mounted) return;
    const el = bodyRef.current;
    const root = scrollRef.current;
    if (!el || !root) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) setMounted(true);
      },
      { root, rootMargin: "600px 0px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [eager, mounted, scrollRef]);

  return (
    <div
      className={`diff-file${selected ? " selected" : ""}`}
      ref={(el) => register(file.path, el)}
    >
      <div className="diff-file-head" onClick={() => onToggle(file.path)}>
        <span className="diff-caret">{collapsed ? "▸" : "▾"}</span>
        <span className={`diff-status s-${file.status}`}>
          {STATUS_GLYPH[file.status] ?? "M"}
        </span>
        <span className="diff-file-path">
          {file.oldPath && file.oldPath !== file.path && (
            <span className="diff-old-path">{file.oldPath} → </span>
          )}
          {file.path}
        </span>
        <span className="diff-file-stat">
          <span className="diff-add-text">+{file.additions}</span>{" "}
          <span className="diff-del-text">−{file.deletions}</span>
        </span>
      </div>
      <div
        className="diff-file-body"
        ref={bodyRef}
        style={collapsed ? { display: "none" } : { minHeight: rows * ROW_H }}
      >
        {file.binary ? (
          <div className="diff-note">Binary file not shown</div>
        ) : file.hunks.length === 0 ? (
          <div className="diff-note">
            {file.truncated ? "File too large to show" : "No content changes"}
          </div>
        ) : (
          !collapsed &&
          (eager || mounted) &&
          file.hunks.map((hunk, i) => (
            <div className="diff-hunk" key={i}>
              {showsHeaderRow(hunk) && (
                <div className="diff-row hunk">
                  <span className="diff-num" />
                  <span className="diff-num" />
                  <span className="diff-code">
                    @@ -{hunk.oldStart} +{hunk.newStart} @@ {hunk.header}
                  </span>
                </div>
              )}
              {hunk.lines.map((line, j) => (
                <div className={`diff-row ${line.kind}`} key={j}>
                  <span className="diff-num">{line.oldLine ?? ""}</span>
                  <span className="diff-num">{line.newLine ?? ""}</span>
                  <span className="diff-code">
                    {line.kind === "add"
                      ? "+"
                      : line.kind === "del"
                        ? "−"
                        : " "}
                    {line.content}
                  </span>
                </div>
              ))}
            </div>
          ))
        )}
        {file.truncated && file.hunks.length > 0 && (
          <div className="diff-note">
            Rest of this file not shown — it is too large to review here.
          </div>
        )}
      </div>
    </div>
  );
});

/**
 * Add/delete colors come from the resolved terminal palette, so a diff
 * pane matches whichever Ghostty/wezterm theme the user imported instead
 * of importing GitHub's green and red.
 */
function diffTheme(colors?: ResolvedColors): React.CSSProperties {
  const palette = colors?.palette ?? [];
  const add = palette[10] ?? palette[2] ?? "#3fb950";
  const del = palette[9] ?? palette[1] ?? "#f85149";
  return {
    "--diff-add": add,
    "--diff-del": del,
    "--diff-add-bg": tint(add, 0.13),
    "--diff-del-bg": tint(del, 0.13),
    "--diff-add-num-bg": tint(add, 0.22),
    "--diff-del-num-bg": tint(del, 0.22),
  } as React.CSSProperties;
}

function tint(hex: string, alpha: number): string {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return "transparent";
  const n = parseInt(match[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}
