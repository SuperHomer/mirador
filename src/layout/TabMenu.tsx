import { useEffect } from "react";
import {
  TAB_COLORS,
  TabColor,
  closeTab,
  equalizePanes,
  newTab,
  setTabColor,
  setWallView,
  toggleAgentWall,
} from "../bindings";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { useConfigStore } from "../state/configStore";
import { reportFailure, useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { ContextMenu, MenuEntry, SEPARATOR } from "./ContextMenu";

/**
 * A tab's right-click menu — on its sidebar row, its tab bar's label, its
 * frame's title bar on the board: the palette's tab actions, done to the
 * tab clicked rather than the active one, each with its shortcut.
 */
export function TabMenu({
  tabId,
  x,
  y,
  onClose,
}: {
  tabId: string;
  x: number;
  y: number;
  onClose: () => void;
}) {
  const tab = useWorkspaceStore((s) => s.snapshot?.tabs.find((t) => t.id === tabId));
  const gone = !tab;
  useEffect(() => {
    if (gone) onClose();
  }, [gone, onClose]);

  const accels = acceleratorsByAction(useConfigStore((s) => s.config?.keybindings));
  const key = (action: string) => {
    const accel = accels.get(action);
    return accel ? formatAccel(accel) : undefined;
  };
  if (!tab) return null;
  const tried = (what: string, call: () => Promise<unknown>) => () =>
    void call().catch((e) => reportFailure(what, e));

  const pick = (color: TabColor | null) => () => {
    onClose();
    void setTabColor(tab.id, color).catch((e) => reportFailure("Tab color", e));
  };

  const items: MenuEntry[] = [
    {
      label: "Rename…",
      // The sidebar row is where a tab is renamed: shown first if hidden.
      run: () => {
        const ws = useWorkspaceStore.getState();
        if (!ws.sidebarVisible) ws.toggleSidebar();
        useUiStore.getState().setRenamingTab(tab.id);
      },
    },
    { label: "New Tab", hint: key("new_tab"), run: tried("New tab", () => newTab()) },
    SEPARATOR,
    <div className="pop-menu-swatches">
      <button
        className={`swatch none${tab.color === null ? " current" : ""}`}
        title="No color"
        aria-label="No color"
        onClick={pick(null)}
      >
        <span />
      </button>
      {TAB_COLORS.map((c) => (
        <button
          key={c}
          className={`swatch${tab.color === c ? " current" : ""}`}
          title={c[0].toUpperCase() + c.slice(1)}
          aria-label={c}
          data-color={c}
          onClick={pick(c)}
        >
          <span />
        </button>
      ))}
    </div>,
    SEPARATOR,
    {
      label: tab.wall ? "Show Splits" : "Show Agent Wall",
      hint: key("agent_wall"),
      run: tried("Agent wall", () => toggleAgentWall(tab.focusedPane)),
    },
    {
      label: "Task Board",
      hint: key("task_board"),
      run: tried("Task board", () => setWallView(tab.focusedPane, "tasks")),
    },
    {
      label: "Layout: Even Sizes",
      hint: key("layout_even"),
      run: tried("Even sizes", () => equalizePanes(tab.id)),
    },
    SEPARATOR,
    { label: "Copy Tab ID", run: () => void navigator.clipboard.writeText(tab.id) },
    SEPARATOR,
    { label: "Close Tab", hint: key("close_tab"), run: tried("Close tab", () => closeTab(tab.id)) },
  ];

  return <ContextMenu x={x} y={y} items={items} onClose={onClose} />;
}
