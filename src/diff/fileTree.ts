import { DiffFile } from "../bindings";

export interface TreeNode {
  /** Label shown in the tree: a basename, or "src/diff" for a collapsed chain. */
  name: string;
  /** Full path from the repo root — also the React key and the dir toggle id. */
  path: string;
  children: TreeNode[];
  /** Set on leaves only. */
  file?: DiffFile;
}

/**
 * Groups changed files into a directory tree, collapsing chains of
 * single-child directories into one row ("src/diff/…") the way GitHub
 * does — a deep tree of one-child levels is all indentation and no
 * information.
 */
export function buildTree(files: DiffFile[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", children: [] };
  for (const file of files) {
    const parts = file.path.split("/");
    let node = root;
    parts.forEach((part, i) => {
      const path = parts.slice(0, i + 1).join("/");
      if (i === parts.length - 1) {
        node.children.push({ name: part, path, children: [], file });
        return;
      }
      let next = node.children.find((c) => !c.file && c.path === path);
      if (!next) {
        next = { name: part, path, children: [] };
        node.children.push(next);
      }
      node = next;
    });
  }
  return collapse(root).children;
}

function collapse(node: TreeNode): TreeNode {
  node.children = node.children.map(collapse);
  // Only directories collapse, and only into a lone child directory.
  if (!node.file && node.path && node.children.length === 1) {
    const only = node.children[0];
    if (!only.file) {
      return { ...only, name: `${node.name}/${only.name}` };
    }
  }
  return node;
}

/** Every directory path in the tree — what "expand all" starts from. */
export function dirPaths(nodes: TreeNode[]): string[] {
  return nodes.flatMap((n) =>
    n.file ? [] : [n.path, ...dirPaths(n.children)],
  );
}
