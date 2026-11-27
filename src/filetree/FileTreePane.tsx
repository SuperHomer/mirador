import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  FileTree,
  FileTreeNode,
  closePane,
  focusPane,
  loadFileTree,
  openTreeFileDiff,
  openTreeFileInEditor,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { reportFailure } from "../state/uiStore";
import { diffTheme } from "../diff/DiffPane";

interface Props {
  paneId: string;
  /** Absolute directory it lists. */
  root: string;
  focused: boolean;
  /** Under its tab's wall, which has no tile for it: it loads nothing
   *  until the wall goes off, and then loads afresh. */
  walled?: boolean;
}

/** The diff pane's letters, but an untracked file is U here: in a tree it
 *  is not an addition to anything shown. */
const STATUS_GLYPH: Record<string, string> = {
  added: "A",
  untracked: "U",
  deleted: "D",
  renamed: "R",
  copied: "C",
  modified: "M",
};

/**
 * What a file tree pane shows, as text: `read-screen`'s answer for one,
 * which has no terminal to read. Null when `paneId` is no file tree pane.
 * Read from the page, so it says what was drawn — the rows unfolded, each
 * name and its status letter — not what Rust listed.
 */
export function renderedFileTree(paneId: string): string | null {
  const body = document.querySelector<HTMLElement>(`[data-file-tree-pane="${CSS.escape(paneId)}"]`);
  return body ? body.innerText.trim() : null;
}

const allDirs = (nodes: FileTreeNode[], into: Set<string> = new Set()) => {
  for (const n of nodes) {
    if (n.dir) {
      into.add(n.path);
      allDirs(n.children, into);
    }
  }
  return into;
};

/**
 * The files of a pane's repository, or of its directory outside one, each
 * marked with what git says about it. Listed and ordered in Rust
 * (cmux-core/src/file_tree.rs); this only draws it. Every directory starts
 * collapsed, the top level showing.
 */
export function FileTreePane({ paneId, root, focused, walled = false }: Props) {
  const [tree, setTree] = useState<FileTree | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const colors = useConfigStore((s) => s.config?.colors);
  const name = root.split(/[\\/]/).filter(Boolean).pop() || root;

  const refresh = useCallback(() => {
    setLoading(true);
    let stale = false;
    void loadFileTree(paneId)
      .then((t) => {
        if (stale) return;
        setTree(t);
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

  useEffect(() => (walled ? undefined : refresh()), [refresh, root, walled]);

  // The tree goes stale the moment anything writes a file, usually the
  // agent in the pane next door; coming back to the pane shows it afresh,
  // as a working-tree diff does. Not as the wall goes off: the load above
  // has just run for that.
  const wasFocused = useRef(focused);
  const wasWalled = useRef(walled);
  useEffect(() => {
    if (focused && !wasFocused.current && !wasWalled.current) refresh();
    wasFocused.current = focused;
    wasWalled.current = walled;
  }, [focused, refresh, walled]);

  const toggle = useCallback(
    (path: string) =>
      setOpen((prev) => {
        const next = new Set(prev);
        if (!next.delete(path)) next.add(path);
        return next;
      }),
    [],
  );

  const openFile = useCallback(
    (path: string) =>
      void openTreeFileInEditor(paneId, path).catch((e) => reportFailure("Open in editor", e)),
    [paneId],
  );

  const openDiff = useCallback(
    (path: string) =>
      void openTreeFileDiff(paneId, path).catch((e) => reportFailure("Open diff", e)),
    [paneId],
  );

  return (
    <div
      className={`pane diff-pane file-tree-pane${focused ? " focused" : ""}`}
      style={diffTheme(colors)}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="diff-chrome">
        <span className="diff-label" title={root}>
          {name}
        </span>
        {tree && !tree.repo && <span className="diff-totals">not a repository</span>}
        <button title="Expand all" disabled={!tree} onClick={() => tree && setOpen(allDirs(tree.nodes))}>
          ⊞
        </button>
        <button title="Collapse all" onClick={() => setOpen(new Set())}>
          ⊟
        </button>
        <button title="Refresh" disabled={loading} onClick={() => refresh()}>
          ⟳
        </button>
        <button title="Close file tree" onClick={() => void closePane(paneId)}>
          ×
        </button>
      </div>

      <div className="file-tree-body" data-file-tree-pane={paneId}>
        {error ? (
          <div className="diff-empty diff-error">{error}</div>
        ) : !tree ? (
          <div className="diff-empty">Loading…</div>
        ) : tree.nodes.length === 0 ? (
          <div className="diff-empty">No files</div>
        ) : (
          <>
            {tree.nodes.map((node) => (
              <Row
                key={node.path}
                node={node}
                depth={0}
                open={open}
                onToggle={toggle}
                onOpenFile={openFile}
                onOpenDiff={openDiff}
              />
            ))}
            {tree.truncated && (
              <div className="diff-note">
                More files than a tree shows: only the first {countNodes(tree.nodes)} are listed.
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

const countNodes = (nodes: FileTreeNode[]): number =>
  nodes.reduce((n, c) => n + 1 + countNodes(c.children), 0);

function Marker({ node }: { node: FileTreeNode }) {
  if (!node.status) return <span className="diff-status" />;
  // A directory's "modified" means something under it changed: a dot, so
  // it does not read as the directory itself being edited.
  const glyph = node.dir && node.status === "modified" ? "•" : (STATUS_GLYPH[node.status] ?? "M");
  return (
    <span className={`diff-status s-${node.status}`} title={node.status}>
      {glyph}
    </span>
  );
}

function Row({
  node,
  depth,
  open,
  onToggle,
  onOpenFile,
  onOpenDiff,
}: {
  node: FileTreeNode;
  depth: number;
  open: Set<string>;
  onToggle: (path: string) => void;
  onOpenFile: (path: string) => void;
  onOpenDiff: (path: string) => void;
}) {
  const indent = { paddingLeft: `${6 + depth * 12}px` };
  if (!node.dir) {
    // A deleted file has nothing on disk to open, but still a diff.
    const gone = node.status === "deleted";
    // A div, not a button: the diff action is a button inside it, and a
    // button may not hold another.
    return (
      <div
        className={`diff-tree-file file-tree-file${gone ? " file-tree-gone" : ""}`}
        style={indent}
        title={gone ? `${node.path} (deleted)` : `Open ${node.path} in your editor`}
        role="button"
        tabIndex={0}
        aria-disabled={gone}
        onClick={() => !gone && onOpenFile(node.path)}
        onKeyDown={(e) => {
          // A key on the diff button inside bubbles here: that one is its own.
          if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
          e.preventDefault();
          if (!gone) onOpenFile(node.path);
        }}
      >
        <span className="diff-caret" />
        <span className="diff-tree-name">{node.name}</span>
        {node.status && (
          <button
            className="file-tree-diff"
            title={`Show the changes to ${node.path}`}
            onClick={(e) => {
              // Not the row's click too: that would open the editor.
              e.stopPropagation();
              onOpenDiff(node.path);
            }}
          >
            diff
          </button>
        )}
        <Marker node={node} />
      </div>
    );
  }
  const expanded = open.has(node.path);
  return (
    <>
      <button className="diff-tree-dir" style={indent} title={node.path} onClick={() => onToggle(node.path)}>
        <span className="diff-caret">{expanded ? "▾" : "▸"}</span>
        <span className="diff-tree-name">{node.name}</span>
        <Marker node={node} />
      </button>
      {expanded &&
        node.children.map((child) => (
          <Row
            key={child.path}
            node={child}
            depth={depth + 1}
            open={open}
            onToggle={onToggle}
            onOpenFile={onOpenFile}
            onOpenDiff={onOpenDiff}
          />
        ))}
    </>
  );
}

export default memo(FileTreePane);
