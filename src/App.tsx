import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  workspaceSnapshot,
  getConfig,
  WorkspaceSnapshot,
  ResolvedConfig,
} from "./bindings";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useConfigStore } from "./state/configStore";
import { useNotificationStore } from "./state/notificationStore";
import { useKeymap } from "./keymap/useKeymap";
import { Sidebar } from "./sidebar/Sidebar";
import { SplitLayer } from "./layout/SplitLayer";
import { BoardHost, TabLayer } from "./layout/BoardHost";
import { CommandPalette } from "./palette/CommandPalette";
import { ActivityPanel } from "./agents/ActivityPanel";
import { NotificationPanel } from "./notifications/NotificationPanel";
import {
  NotificationDto,
  Node,
  listNotifications,
  setBrowserVisible,
} from "./bindings";
import { useUiStore } from "./state/uiStore";
import {
  getTerminal,
  readScreenText,
  saveAllScrollbacks,
} from "./terminal/registry";
import { invoke } from "@tauri-apps/api/core";

function collectPaneIds(node: Node): string[] {
  if (node.type === "leaf") return [node.paneId];
  return node.children.flatMap(collectPaneIds);
}

const isWindows = navigator.platform.toUpperCase().startsWith("WIN");

/**
 * A dropped path, quoted to stay in one piece. Elsewhere than Windows, POSIX
 * single quotes, each `'` closed and escaped: one literal word. On Windows,
 * double quotes (a name cannot contain `"`) only keep a path with spaces
 * together; the shell still expands what is inside — `$` and backticks in
 * PowerShell and Git Bash, `%…%` (and `!…!` with delayed expansion) in
 * cmd. Those are rare in paths, and quoting each shell its own way would
 * mean knowing which shell each pane runs.
 */
function quotePath(path: string): string {
  if (isWindows) return `"${path}"`;
  return `'${path.replaceAll("'", `'\\''`)}'`;
}

const resolveScreenRead = (requestId: number, text: string) =>
  invoke<void>("resolve_screen_read", { requestId, text });

export default function App() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const setSnapshot = useWorkspaceStore((s) => s.setSnapshot);
  const config = useConfigStore((s) => s.config);
  const setConfig = useConfigStore((s) => s.setConfig);
  const paletteOpen = useUiStore((s) => s.paletteOpen);
  const notificationsOpen = useUiStore((s) => s.notificationsOpen);
  useKeymap();

  // Native child webviews float above the host webview: hide the ones on
  // inactive tabs, and all of them while an overlay is open. On the board
  // each browser card decides for itself (`BrowserPane`).
  useEffect(() => {
    if (!snapshot || snapshot.view === "board") return;
    const activeTab = snapshot.tabs.find((t) => t.id === snapshot.activeTab);
    // A zoomed tab shows one pane; a browser pane zoomed out of view is
    // hidden like one on another tab.
    const activePanes = activeTab
      ? activeTab.zoomedPane
        ? [activeTab.zoomedPane]
        : collectPaneIds(activeTab.root)
      : [];
    const overlaysOpen = paletteOpen || notificationsOpen;
    for (const b of snapshot.browserPanes) {
      const visible = !overlaysOpen && activePanes.includes(b.paneId);
      void setBrowserVisible(b.paneId, visible);
    }
  }, [snapshot, paletteOpen, notificationsOpen]);

  useEffect(() => {
    let disposed = false;
    void workspaceSnapshot().then((s) => {
      if (!disposed) setSnapshot(s);
    });
    void getConfig().then((c) => {
      if (!disposed) setConfig(c);
    });
    const unlistenWs = listen<WorkspaceSnapshot>("workspace-changed", (e) =>
      setSnapshot(e.payload),
    );
    const unlistenCfg = listen<ResolvedConfig>("config-changed", (e) =>
      setConfig(e.payload),
    );
    void listNotifications().then((list) => {
      if (!disposed) useNotificationStore.getState().setAll(list);
    });
    const unlistenNotif = listen<NotificationDto>("notification", (e) =>
      useNotificationStore.getState().append(e.payload),
    );
    // Automation socket read-screen round-trip.
    const unlistenRead = listen<{
      requestId: number;
      paneId: string;
      lines: number | null;
    }>("read-screen-request", (e) => {
      void resolveScreenRead(
        e.payload.requestId,
        readScreenText(e.payload.paneId, e.payload.lines),
      );
    });
    // Scrollback persistence: cadence comes from Rust (webview timers are
    // suspended in occluded windows); blur catches "about to quit".
    const unlistenScrollback = listen("scrollback-save-request", () =>
      saveAllScrollbacks(),
    );
    // Files dropped on a pane are typed into it as paths, as a terminal
    // does. A drop on no pane, or on a pane with no terminal, does nothing.
    const unlistenDrop = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type !== "drop" || e.payload.paths.length === 0) return;
      const { x, y } = e.payload.position.toLogical(window.devicePixelRatio);
      const pane = document
        .elementFromPoint(x, y)
        ?.closest<HTMLElement>("[data-pane]")?.dataset.pane;
      if (!pane) return;
      getTerminal(pane)?.paste(
        e.payload.paths.map(quotePath).join(" ") + " ",
      );
    });
    const onBlur = () => saveAllScrollbacks();
    window.addEventListener("blur", onBlur);
    return () => {
      void unlistenScrollback.then((fn) => fn());
      void unlistenDrop.then((fn) => fn());
      window.removeEventListener("blur", onBlur);
      void unlistenRead.then((fn) => fn());
      disposed = true;
      void unlistenWs.then((fn) => fn());
      void unlistenCfg.then((fn) => fn());
      void unlistenNotif.then((fn) => fn());
    };
  }, [setSnapshot, setConfig]);

  if (!snapshot || !config) return <div className="app" />;
  const board = snapshot.view === "board";

  return (
    <div
      className="app"
      // The resolved terminal background, exposed to CSS so a pane's
      // sub-row remainder can match the terminal rather than the chrome.
      style={{ "--term-bg": config.colors.background } as React.CSSProperties}
    >
      <Sidebar />
      <BoardHost board={board}>
        {snapshot.tabs.map((tab) => (
          <TabLayer
            key={tab.id}
            tab={tab}
            board={board}
            current={tab.id === snapshot.activeTab}
          >
            <SplitLayer
              tab={tab}
              board={board}
              active={board || tab.id === snapshot.activeTab}
              current={tab.id === snapshot.activeTab}
              agents={snapshot.agents}
              unreadPanes={snapshot.unreadPanes}
              agentPanes={snapshot.agentPanes}
              browserPanes={snapshot.browserPanes}
              remotePanes={snapshot.remotePanes}
              diffPanes={snapshot.diffPanes}
              whatsNewPanes={snapshot.whatsNewPanes}
              graphPanes={snapshot.graphPanes}
              agentEditorPanes={snapshot.agentEditorPanes ?? []}
            />
          </TabLayer>
        ))}
      </BoardHost>
      <CommandPalette />
      <NotificationPanel />
      <ActivityPanel />
    </div>
  );
}
