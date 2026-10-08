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
  openAgentWall,
  openWhatsNew,
  zoomPane,
  installUpdate,
  quitEndingSessions,
} from "../bindings";
import {
  useWorkspaceStore,
  activeTab,
  sidebarEntries,
  isCurrentEntry,
  SidebarEntry,
} from "../state/workspaceStore";
import { useUpdateStore } from "../state/updateStore";
import { useUiStore } from "../state/uiStore";
import { useClaudeStore } from "../state/claudeStore";
import { getTerminal, terminalWithFocus } from "../terminal/registry";

export interface ActionDef {
  id: string;
  title: string;
  run: () => void;
}

function focusedPane(): string | undefined {
  const { snapshot } = useWorkspaceStore.getState();
  return activeTab(snapshot)?.focusedPane;
}

/** Shows a sidebar row: a whole tab, or an agent's pane alone. */
export function openEntry(entry: SidebarEntry) {
  if (entry.kind === "tab") void setActiveTab(entry.tab.id);
  else void zoomPane(entry.agent.paneId, true);
}

function cycleTab(offset: number) {
  const { snapshot, project } = useWorkspaceStore.getState();
  const entries = sidebarEntries(snapshot, project);
  if (!snapshot || entries.length < 2) return;
  const idx = entries.findIndex((e) => isCurrentEntry(snapshot, e));
  openEntry(entries[(idx + offset + entries.length) % entries.length]);
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
      // The terminal with keyboard focus first: in the agent wall that is
      // an agent's, while the workspace's focused pane is the wall.
      const pane = focusedPane();
      const term = terminalWithFocus() ?? (pane ? getTerminal(pane) : undefined);
      const selection = term?.getSelection();
      if (selection) void navigator.clipboard.writeText(selection);
    },
  },
  {
    id: "paste",
    title: "Paste",
    run: () => {
      const pane = focusedPane();
      const term = terminalWithFocus() ?? (pane ? getTerminal(pane) : undefined);
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
    run: () => {
      const { snapshot } = useWorkspaceStore.getState();
      const pane = activeTab(snapshot)?.focusedPane;
      if (pane) void openGraph(pane, false);
    },
  },
  {
    id: "agent_wall",
    title: "Agent Wall",
    // Every Claude Code agent's terminal in one grid; goes to the wall's
    // tab when there already is one.
    run: () => void openAgentWall(focusedPane() ?? null, true),
  },
  {
    id: "toggle_zoom",
    title: "Toggle Pane Zoom",
    // The focused pane alone, filling its tab — or the whole tab again.
    run: () => {
      const pane = focusedPane();
      if (pane) void zoomPane(pane);
    },
  },
  {
    id: "new_agent",
    title: "New Agent (Claude Code)",
    run: () => void openAgent(null, null, focusedPane() ?? null, true),
  },
  {
    id: "setup_claude_integration",
    title: "Set Up Claude Code Integration",
    // Hooks that light up tabs, /mira-diff, and mira on PATH. The answer
    // shows at the foot of the sidebar, where the first-launch offer was.
    run: () => void useClaudeStore.getState().run(),
  },
  {
    id: "quit_ending_sessions",
    title: "Quit and End All Sessions",
    // With persistSessions on, Cmd+Q leaves every terminal running for the
    // next launch; this ends them first. Without it, it is a plain quit.
    run: () => void quitEndingSessions(),
  },
  {
    id: "whats_new",
    title: "What's New",
    // Opens the release notes for the running version — the same pane the
    // app shows itself once after an update, for reading again later.
    run: () => void openWhatsNew(),
  },
  {
    id: "check_for_updates",
    title: "Check for Updates",
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
    title: "Git: New Diff Pane (uncommitted changes)",
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
    const { snapshot, project } = useWorkspaceStore.getState();
    const target = sidebarEntries(snapshot, project)[Number(tabJump[1]) - 1];
    if (target) openEntry(target);
    return true;
  }
  return false;
}
