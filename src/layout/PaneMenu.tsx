import { useEffect } from "react";
import {
  closePane,
  openAgent,
  openBrowser,
  openDiff,
  openGraph,
  splitPane,
  zoomPane,
} from "../bindings";
import { holds } from "../agents/NewMenu";
import { startHere } from "../agents/startHere";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { opensInTab, useConfigStore } from "../state/configStore";
import { reportFailure } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { getTerminal, isLent } from "../terminal/registry";
import { pasteFromClipboard } from "../terminal/pasteImage";
import { ContextMenu, MenuEntry, SEPARATOR } from "./ContextMenu";

/**
 * A pane's right-click menu: what the palette's pane actions do, done to
 * the pane clicked rather than the focused one. Each item shows its
 * shortcut, so the menu is also where the keys are learnt.
 */
export function PaneMenu({
  pane,
  x,
  y,
  onClose,
}: {
  pane: string;
  x: number;
  y: number;
  onClose: () => void;
}) {
  // What it offers is for this pane, in the tab on screen: it closes when
  // the pane goes, or another tab comes on screen. Its own tab coming on
  // is fine — a right-click on a card of another frame focuses it.
  useEffect(
    () =>
      useWorkspaceStore.subscribe((s, prev) => {
        const tabs = s.snapshot?.tabs ?? [];
        const tab = tabs.find((t) => holds(t.root, pane));
        const switched = s.snapshot?.activeTab !== prev.snapshot?.activeTab;
        if (!tab || (switched && s.snapshot?.activeTab !== tab.id)) onClose();
      }),
    [pane, onClose],
  );

  const accels = acceleratorsByAction(useConfigStore((s) => s.config?.keybindings));
  const key = (action: string) => {
    const accel = accels.get(action);
    return accel ? formatAccel(accel) : undefined;
  };
  const tried = (what: string, call: () => Promise<unknown>) => () =>
    void call().catch((e) => reportFailure(what, e));

  // A terminal on loan to the wall is drawn there, not in this pane.
  const term = isLent(pane) ? undefined : getTerminal(pane);
  // Read now: on macOS xterm has just selected the word right-clicked.
  const selection = term?.getSelection() ?? "";

  const items: MenuEntry[] = [
    { label: "Split Right", hint: key("split_right"), run: tried("Split right", () => splitPane(pane, "row")) },
    { label: "Split Down", hint: key("split_down"), run: tried("Split down", () => splitPane(pane, "column")) },
    { label: "Toggle Zoom", hint: key("toggle_zoom"), run: tried("Zoom", () => zoomPane(pane)) },
    SEPARATOR,
    {
      label: "New Agent",
      hint: key("new_agent"),
      run: () =>
        startHere(pane, { harness: "claude" }, "Start Claude Code", (remote) =>
          openAgent(null, null, pane, false, null, remote !== null),
        ),
    },
    { label: "Diff", hint: key("new_diff_pane"), run: tried("Diff", () => openDiff(pane, opensInTab("diff"), null)) },
    { label: "Commit Graph", hint: key("commit_graph"), run: tried("Commit graph", () => openGraph(pane, opensInTab("graph"))) },
    {
      label: "New Browser Pane",
      hint: key("new_browser_pane"),
      run: tried("Browser", () => openBrowser(pane, false, "about:blank")),
    },
    SEPARATOR,
    ...(term
      ? [
          {
            label: "Copy",
            hint: key("copy"),
            disabled: !selection,
            run: () => void navigator.clipboard.writeText(selection),
          },
          { label: "Paste", hint: key("paste"), run: () => void pasteFromClipboard(term, pane) },
        ]
      : []),
    { label: "Copy Pane ID", run: () => void navigator.clipboard.writeText(pane) },
    SEPARATOR,
    { label: "Close Pane", hint: key("close_pane"), run: tried("Close", () => closePane(pane)) },
  ];

  return <ContextMenu x={x} y={y} items={items} onClose={onClose} />;
}
