// Single action registry: feeds both the keymap dispatcher and the command
// palette, so anything bindable is palette-searchable and vice versa.
import {
  newTab,
  closePane,
  closeTab,
  splitPane,
  focusDirection,
  setActiveTab,
  openBrowser,
  writePty,
} from "../bindings";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";
import { useUiStore } from "../state/uiStore";
import { getTerminal } from "../terminal/registry";

export interface ActionDef {
  id: string;
  title: string;
  run: () => void;
}

function focusedPane(): string | undefined {
  const { snapshot } = useWorkspaceStore.getState();
  return activeTab(snapshot)?.focusedPane;
}

function cycleTab(offset: number) {
  const { snapshot } = useWorkspaceStore.getState();
  if (!snapshot || snapshot.tabs.length < 2) return;
  const idx = snapshot.tabs.findIndex((t) => t.id === snapshot.activeTab);
  const next =
    snapshot.tabs[(idx + offset + snapshot.tabs.length) % snapshot.tabs.length];
  void setActiveTab(next.id);
}

export const actions: ActionDef[] = [
  { id: "new_tab", title: "New Tab", run: () => void newTab() },
  {
    id: "close_pane",
    title: "Close Pane",
    run: () => {
      const pane = focusedPane();
      if (pane) void closePane(pane);
    },
  },
  {
    id: "close_tab",
    title: "Close Tab",
    run: () => {
      const { snapshot } = useWorkspaceStore.getState();
      if (snapshot) void closeTab(snapshot.activeTab);
    },
  },
  {
    id: "split_right",
    title: "Split Right",
    run: () => {
      const pane = focusedPane();
      if (pane) void splitPane(pane, "row");
    },
  },
  {
    id: "split_down",
    title: "Split Down",
    run: () => {
      const pane = focusedPane();
      if (pane) void splitPane(pane, "column");
    },
  },
  {
    id: "focus_left",
    title: "Focus Pane Left",
    run: () => void focusDirection("left"),
  },
  {
    id: "focus_right",
    title: "Focus Pane Right",
    run: () => void focusDirection("right"),
  },
  {
    id: "focus_up",
    title: "Focus Pane Up",
    run: () => void focusDirection("up"),
  },
  {
    id: "focus_down",
    title: "Focus Pane Down",
    run: () => void focusDirection("down"),
  },
  { id: "next_tab", title: "Next Tab", run: () => cycleTab(1) },
  { id: "prev_tab", title: "Previous Tab", run: () => cycleTab(-1) },
  {
    id: "toggle_sidebar",
    title: "Toggle Sidebar",
    run: () => useWorkspaceStore.getState().toggleSidebar(),
  },
  {
    id: "command_palette",
    title: "Command Palette",
    run: () => useUiStore.getState().togglePalette(),
  },
  {
    id: "notifications",
    title: "Notifications Panel",
    run: () => useUiStore.getState().toggleNotifications(),
  },
  // Windows/Linux have no app menu supplying Edit roles, so the terminal
  // conventions (Ctrl+Shift+C/V) are wired here. Plain Ctrl+C must stay
  // with the program in the pane.
  {
    id: "copy",
    title: "Copy Selection",
    run: () => {
      const pane = focusedPane();
      const selection = pane ? getTerminal(pane)?.getSelection() : undefined;
      if (selection) void navigator.clipboard.writeText(selection);
    },
  },
  {
    id: "paste",
    title: "Paste",
    run: () => {
      const pane = focusedPane();
      if (!pane) return;
      void navigator.clipboard.readText().then((text) => {
        if (text) void writePty(pane, text);
      });
    },
  },
  {
    id: "new_browser_pane",
    title: "New Browser Pane",
    run: () => {
      const pane = focusedPane();
      if (pane) void openBrowser(pane, false, "about:blank");
    },
  },
];

export function runAction(id: string): boolean {
  const def = actions.find((a) => a.id === id);
  if (def) {
    def.run();
    return true;
  }
  // tab_1 .. tab_9
  const tabJump = id.match(/^tab_([1-9])$/);
  if (tabJump) {
    const { snapshot } = useWorkspaceStore.getState();
    const target = snapshot?.tabs[Number(tabJump[1]) - 1];
    if (target) void setActiveTab(target.id);
    return true;
  }
  return false;
}
