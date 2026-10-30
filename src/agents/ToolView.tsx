import { WallTool } from "../bindings";
import { DiffPane } from "../diff/DiffPane";
import { GraphPane } from "../graph/GraphPane";
import { useWorkspaceStore } from "../state/workspaceStore";

/**
 * A diff or graph tool drawn on the wall: the pane's own component, with
 * what its split is given. Unlike a terminal it is not lent but drawn a
 * second time; the split's instance under the wall is told it is not
 * focused (SplitLayer), so only this one refreshes when the pane is.
 */
export function ToolView({ tool, focused }: { tool: WallTool; focused: boolean }) {
  const diff = useWorkspaceStore((s) => s.snapshot?.diffPanes.find((d) => d.paneId === tool.paneId));
  const graph = useWorkspaceStore((s) => s.snapshot?.graphPanes.find((g) => g.paneId === tool.paneId));
  if (diff) return <DiffPane paneId={diff.paneId} repo={diff.repo} spec={diff.spec} focused={focused} />;
  if (graph) return <GraphPane paneId={graph.paneId} repo={graph.repo} focused={focused} />;
  return null;
}
