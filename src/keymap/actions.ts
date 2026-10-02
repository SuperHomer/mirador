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
  openDiff,
  checkUpdate,
  installUpdate,
} from "../bindings";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";
import { useUpdateStore } from "../state/updateStore";
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
        // Through xterm, not straight to the PTY: it applies bracketed
        // paste (a multi-line paste must not run line by line) and newline
        // normalization, and its input path keeps the paste in order with
        // typing.
        if (text) getTerminal(pane)?.paste(text);
      });
    },
  },
  {
    id: "check_for_updates",
    title: "Check for Updates",
    run: () => {
      void checkUpdate().then((u) => {
        useUpdateStore.getState().setUpdate(u);
      });
    },
  },
  {
    id: "install_update",
    title: "Install Update and Restart",
    run: () => {
      // Only offered when a check already found something; the banner is the
      // discoverable route and this is the keyboard one.
      if (!useUpdateStore.getState().update) return;
      useUpdateStore.getState().setInstalling(true);
      void installUpdate().catch((e: unknown) =>
        useUpdateStore.getState().setError(String(e)),
      );
    },
  },
  {
    id: "new_diff_pane",
    title: "New Diff Pane (uncommitted changes)",
    run: () => {
      const pane = focusedPane();
      if (pane) void openDiff(pane, false, null);
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
