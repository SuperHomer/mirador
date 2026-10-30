import { useEffect } from "react";
import { setBrowserVisible, WallTool } from "../bindings";
import { BrowserPane } from "../browser/BrowserPane";
import { DiffPane } from "../diff/DiffPane";
import { GraphPane } from "../graph/GraphPane";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";

/**
 * A browser, diff or graph tool drawn on the wall: the pane's own
 * component, with what its split is given. Unlike a terminal it is not
 * lent but drawn a second time; the split's instance under the wall is
 * told so (`walled`, from SplitLayer), and leaves the loading — and a
 * browser's page — to this one.
 */
export function ToolView({
  tool,
  focused,
  visible,
}: {
  tool: WallTool;
  focused: boolean;
  /** The wall is on screen. */
  visible: boolean;
}) {
  const board = useWorkspaceStore((s) => s.snapshot?.view === "board");
  const browser = useWorkspaceStore((s) => s.snapshot?.browserPanes.find((b) => b.paneId === tool.paneId));
  const diff = useWorkspaceStore((s) => s.snapshot?.diffPanes.find((d) => d.paneId === tool.paneId));
  const graph = useWorkspaceStore((s) => s.snapshot?.graphPanes.find((g) => g.paneId === tool.paneId));
  // A page is one native webview, shown in one place: here, for as long
  // as this tile is on screen (`App` shows it).
  const shown = !!browser && visible;
  useEffect(() => {
    if (!shown) return;
    useUiStore.getState().setWallBrowser(tool.paneId, true);
    return () => useUiStore.getState().setWallBrowser(tool.paneId, false);
  }, [shown, tool.paneId]);
  // On the board the tile's BrowserPane shows the page itself, and the
  // split's under the wall abstains: as the tile goes — filtered out,
  // another page — it takes the page down. When the wall goes off this
  // runs before the split's BrowserPane looks again, so that one wins.
  const boardPage = !!browser && board;
  useEffect(() => {
    if (!boardPage) return;
    return () => void setBrowserVisible(tool.paneId, false);
  }, [boardPage, tool.paneId]);
  if (browser) return <BrowserPane paneId={browser.paneId} url={browser.url} focused={focused} board={board} />;
  if (diff) return <DiffPane paneId={diff.paneId} repo={diff.repo} spec={diff.spec} focused={focused} />;
  if (graph) return <GraphPane paneId={graph.paneId} repo={graph.repo} focused={focused} />;
  return null;
}
