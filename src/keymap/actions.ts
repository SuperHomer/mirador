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
  openGraph,
  openAgent,
  toggleAgentWall,
  setWallView,
  openAgentEditor,
  openWhatsNew,
  zoomPane,
  setView,
  installUpdate,
  quitEndingSessions,
} from "../bindings";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";
import { useUpdateStore } from "../state/updateStore";
import { useUiStore } from "../state/uiStore";
import { useClaudeStore } from "../state/claudeStore";
import { getTerminal, terminalWithFocus } from "../terminal/registry";
import { arrangeBoard, fitBoard } from "../state/boardStore";

/** The palette's groups: what an action is about. */
export type ActionGroup = "agents" | "git" | "pane" | "view" | "app";

export interface ActionDef {
  id: string;
  /** "Area: Verb", as the palette lists it. */
  title: string;
  group: ActionGroup;
  /** What it acts on, shown after the title: "uncommitted changes". */
  hint?: string;
  run: () => void;
}

function focusedPane(): string | undefined {
  const { snapshot } = useWorkspaceStore.getState();
  return activeTab(snapshot)?.focusedPane;
}

/** Moves `offset` tabs along the sidebar's order, wrapping around. */
function cycleTab(offset: number) {
  const { snapshot } = useWorkspaceStore.getState();
  if (!snapshot || snapshot.tabs.length < 2) return;
  const tabs = snapshot.tabs;
  const idx = tabs.findIndex((t) => t.id === snapshot.activeTab);
  void setActiveTab(tabs[(idx + offset + tabs.length) % tabs.length].id);
}

export const actions: ActionDef[] = [
  { id: "new_tab", title: "Tab: New", group: "pane", run: () => void newTab() },
  {
    id: "close_pane",
    title: "Pane: Close",
    group: "pane",
    run: () => {
      const pane = focusedPane();
      if (pane) void closePane(pane);
    },
  },
  {
    id: "close_tab",
    title: "Tab: Close",
    group: "pane",
    run: () => {
      const { snapshot } = useWorkspaceStore.getState();
      if (snapshot) void closeTab(snapshot.activeTab);
    },
  },
  {
    id: "split_right",
    title: "Pane: Split Right…",
    group: "pane",
    run: () => {
      const pane = focusedPane();
      if (pane) void splitPane(pane, "row");
    },
  },
  {
    id: "split_down",
    title: "Pane: Split Down…",
    group: "pane",
    run: () => {
      const pane = focusedPane();
      if (pane) void splitPane(pane, "column");
    },
  },
  {
    id: "focus_left",
    title: "Pane: Focus Left",
    group: "pane",
    run: () => void focusDirection("left"),
  },
  {
    id: "focus_right",
    title: "Pane: Focus Right",
    group: "pane",
    run: () => void focusDirection("right"),
  },
  {
    id: "focus_up",
    title: "Pane: Focus Up",
    group: "pane",
    run: () => void focusDirection("up"),
  },
  {
    id: "focus_down",
    title: "Pane: Focus Down",
    group: "pane",
    run: () => void focusDirection("down"),
  },
  { id: "next_tab", title: "Tab: Next", group: "pane", run: () => cycleTab(1) },
  { id: "prev_tab", title: "Tab: Previous", group: "pane", run: () => cycleTab(-1) },
  {
    id: "toggle_sidebar",
    title: "View: Toggle Sidebar",
    group: "view",
    run: () => useWorkspaceStore.getState().toggleSidebar(),
  },
  {
    id: "command_palette",
    title: "App: Command Palette",
    group: "app",
    run: () => useUiStore.getState().togglePalette(),
  },
  {
    id: "notifications",
    title: "Agents: Open Inbox",
    group: "agents",
    run: () => useUiStore.getState().toggleNotifications(),
  },
  // Windows/Linux have no app menu supplying Edit roles, so the terminal
  // conventions (Ctrl+Shift+C/V) are wired here. Plain Ctrl+C must stay
  // with the program in the pane.
  {
    id: "copy",
    title: "Pane: Copy Selection",
    group: "pane",
    run: () => {
      // The terminal with keyboard focus first: in the agent wall that is
      // a tile's, which need not be the workspace's focused pane.
      const pane = focusedPane();
      const term =
        terminalWithFocus() ?? (pane ? getTerminal(pane) : undefined);
      const selection = term?.getSelection();
      if (selection) void navigator.clipboard.writeText(selection);
    },
  },
  {
    id: "paste",
    title: "Pane: Paste",
    group: "pane",
    run: () => {
      const pane = focusedPane();
      const term =
        terminalWithFocus() ?? (pane ? getTerminal(pane) : undefined);
      if (!term) return;
      void navigator.clipboard.readText().then((text) => {
        // Through xterm, not straight to the PTY: it applies bracketed
        // paste (a multi-line paste must not run line by line) and newline
        // normalization, and its input path keeps the paste in order with
        // typing.
        if (text) term.paste(text);
      });
    },
  },
  {
    id: "commit_graph",
    title: "Git: Commit Graph",
    group: "git",
    run: () => {
      const { snapshot } = useWorkspaceStore.getState();
      const pane = activeTab(snapshot)?.focusedPane;
      if (pane) void openGraph(pane, false);
    },
  },
  {
    id: "agent_wall",
    title: "Agents: Show Wall",
    group: "agents",
    hint: "again for the splits",
    // The tab's agents' terminals in one grid in place of its splits,
    // or its splits again.
    run: () => void toggleAgentWall(focusedPane() ?? null),
  },
  {
    id: "task_board",
    title: "Agents: Task Board",
    group: "agents",
    // The cards of the tasks the focused tab's agents were given: its
    // wall, on the Tasks side.
    run: () => void setWallView(focusedPane() ?? null, "tasks"),
  },
  {
    id: "agent_editor",
    title: "Agents: Edit Roles, Ways & Teams",
    group: "agents",
    // The role, way-of-working and team files the focused pane's
    // directory sees, beside it.
    run: () => void openAgentEditor(focusedPane() ?? null, false),
  },
  {
    id: "agent_activity",
    title: "Agents: Show Activity",
    group: "agents",
    hint: "the focused agent",
    // The focused agent's task progress and its subagents, in a drawer:
    // the whole of what its status bar sums up.
    run: () => {
      const pane = focusedPane();
      const agent = useWorkspaceStore
        .getState()
        .snapshot?.agents.find((a) => a.paneId === pane);
      if (pane && agent?.hasActivity) useUiStore.getState().openActivity(pane);
    },
  },
  {
    id: "toggle_board",
    title: "View: Board",
    group: "view",
    hint: "again for the tabs",
    // Every pane of every tab as a card on one board, or back to tabs.
    run: () => {
      const view = useWorkspaceStore.getState().snapshot?.view;
      void setView(view === "board" ? "tabs" : "board");
    },
  },
  {
    id: "board_fit",
    title: "View: Show Everything",
    group: "view",
    hint: "on the board",
    run: () => {
      if (useWorkspaceStore.getState().snapshot?.view === "board") fitBoard();
    },
  },
  {
    id: "board_arrange",
    title: "View: Arrange Frames",
    group: "view",
    hint: "on the board",
    run: () => {
      if (useWorkspaceStore.getState().snapshot?.view === "board") arrangeBoard();
    },
  },
  {
    id: "toggle_zoom",
    title: "Pane: Toggle Zoom",
    group: "pane",
    // The focused pane alone, filling its tab — or the whole tab again.
    run: () => {
      const pane = focusedPane();
      if (pane) void zoomPane(pane);
    },
  },
  {
    id: "new_agent",
    title: "Agents: New Agent",
    group: "agents",
    hint: "Claude Code",
    // Beside the focused pane, in the tab you are working in.
    run: () => void openAgent(null, null, focusedPane() ?? null, false),
  },
  {
    id: "setup_claude_integration",
    title: "App: Set Up Claude Code Integration",
    group: "app",
    // Hooks that light up tabs, /mira-diff, and mira on PATH. The answer
    // shows at the foot of the sidebar, where the first-launch offer was.
    run: () => void useClaudeStore.getState().run(),
  },
  {
    id: "quit_ending_sessions",
    title: "App: Quit and End All Sessions",
    group: "app",
    // With persistSessions on, Cmd+Q leaves every terminal running for the
    // next launch; this ends them first. Without it, it is a plain quit.
    run: () => void quitEndingSessions(),
  },
  {
    id: "whats_new",
    title: "App: What's New",
    group: "app",
    // Opens the release notes for the running version — the same pane the
    // app shows itself once after an update, for reading again later.
    run: () => void openWhatsNew(),
  },
  {
    id: "check_for_updates",
    title: "App: Check for Updates…",
    group: "app",
    run: () => {
      // A background check is allowed to find nothing in silence. This one
      // was asked for, so it answers either way — "up to date" is a result,
      // and so is a failure. Without this the command looked broken when it
      // had in fact worked.
      const store = useUpdateStore.getState();
      store.setError(null);
      store.setCheck("checking");
      void checkUpdate()
        .then((u) => {
          store.setUpdate(u);
          store.setCheck(u ? null : "uptodate");
        })
        .catch((e: unknown) => {
          store.setError(String(e));
          store.setCheck("error");
        });
    },
  },
  {
    id: "install_update",
    title: "App: Install Update and Restart",
    group: "app",
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
    title: "Git: New Diff Pane",
    group: "git",
    hint: "uncommitted changes",
    run: () => {
      const pane = focusedPane();
      if (pane) void openDiff(pane, false, null);
    },
  },
  {
    id: "new_browser_pane",
    title: "Pane: New Browser Pane",
    group: "pane",
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
  // tab_1 .. tab_9: tabs only, by the number beside each in the sidebar.
  const tabJump = id.match(/^tab_([1-9])$/);
  if (tabJump) {
    const { snapshot } = useWorkspaceStore.getState();
    const target = snapshot?.tabs[Number(tabJump[1]) - 1];
    if (target) void setActiveTab(target.id);
    return true;
  }
  return false;
}
