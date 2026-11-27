import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  workspaceSnapshot,
  getConfig,
  WorkspaceSnapshot,
  ResolvedConfig,
  sendFiles,
  setMenuActions,
} from "./bindings";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useConfigStore } from "./state/configStore";
import { useNotificationStore } from "./state/notificationStore";
import { useKeymap } from "./keymap/useKeymap";
import { actions, runAction } from "./keymap/actions";
import { useZoomWheel } from "./keymap/useZoomWheel";
import { Sidebar } from "./sidebar/Sidebar";
import { SplitLayer } from "./layout/SplitLayer";
import { BoardHost, TabLayer } from "./layout/BoardHost";
import { ErrorBar } from "./layout/ErrorBar";
import { CommandPalette } from "./palette/CommandPalette";
import { ActivityPanel } from "./agents/ActivityPanel";
import { NotificationPanel } from "./notifications/NotificationPanel";
import {
  NotificationDto,
  Node,
  listNotifications,
  setBrowserVisible,
} from "./bindings";
import { reportFailure, useUiStore } from "./state/uiStore";
import {
  getTerminal,
  readScreenText,
  saveAllScrollbacks,
} from "./terminal/registry";
import { quotePath } from "./terminal/pasteImage";
import { invoke } from "@tauri-apps/api/core";

function collectPaneIds(node: Node): string[] {
  if (node.type === "leaf") return [node.paneId];
  return node.children.flatMap(collectPaneIds);
}

const resolveScreenRead = (
  requestId: number,
  read: { text: string; cursor: [number, number] | null; afterCursor: string },
) => invoke<void>("resolve_screen_read", { requestId, ...read });

export default function App() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const setSnapshot = useWorkspaceStore((s) => s.setSnapshot);
  const config = useConfigStore((s) => s.config);
  const setConfig = useConfigStore((s) => s.setConfig);
  const paletteOpen = useUiStore((s) => s.paletteOpen);
  const notificationsOpen = useUiStore((s) => s.notificationsOpen);
  const wallBrowsers = useUiStore((s) => s.wallBrowsers);
  const newMenuOpen = useUiStore((s) => s.newMenuOpen);
  useKeymap();
  useZoomWheel();

  // Native child webviews float above the host webview: hide the ones on
  // inactive tabs, and all of them while an overlay is open. A tab showing
  // its wall shows only the pages its tiles draw, over those tiles. On the
  // board each browser card decides for itself (`BrowserPane`).
  useEffect(() => {
    if (!snapshot || snapshot.view === "board") return;
    const activeTab = snapshot.tabs.find((t) => t.id === snapshot.activeTab);
    // A zoomed tab shows one pane; a browser pane zoomed out of view is
    // hidden like one on another tab.
    const activePanes = activeTab
      ? activeTab.wall
        ? wallBrowsers
        : activeTab.zoomedPane
          ? [activeTab.zoomedPane]
          : collectPaneIds(activeTab.root)
      : [];
    const overlaysOpen = paletteOpen || notificationsOpen || newMenuOpen;
    for (const b of snapshot.browserPanes) {
      const visible = !overlaysOpen && activePanes.includes(b.paneId);
      void setBrowserVisible(b.paneId, visible);
    }
  }, [snapshot, paletteOpen, notificationsOpen, wallBrowsers, newMenuOpen]);

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
      // Rust answers what to type: the paths themselves in a local pane,
      // the copies it sent there in an SSH pane.
      const paths = e.payload.paths;
      void sendFiles(pane, paths)
        .then((typed) => getTerminal(pane)?.paste(typed.map(quotePath).join(" ") + " "))
        .catch((err) => reportFailure("Drop files", err));
    });
    // `mira send-file`: typed by the terminal, as a drop is, so xterm
    // brackets it only for a program that asked for bracketed paste.
    const unlistenPasteInto = listen<{ paneId: string; text: string }>("paste-into-pane", (e) =>
      getTerminal(e.payload.paneId)?.paste(e.payload.text),
    );
    // The menu bar lists the palette's actions and runs them here.
    void setMenuActions(actions.map(({ id, title, group }) => ({ id, title, group }))).catch(
      (err) => reportFailure("Menu bar", err),
    );
    const unlistenMenu = listen<string>("menu-action", (e) => runAction(e.payload));
    const onBlur = () => saveAllScrollbacks();
    window.addEventListener("blur", onBlur);
    return () => {
      void unlistenScrollback.then((fn) => fn());
      void unlistenDrop.then((fn) => fn());
      void unlistenPasteInto.then((fn) => fn());
      void unlistenMenu.then((fn) => fn());
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
      {/* The error bar takes a row of its own above the panes rather than
          floating over them, where a browser page, a native view, would
          cover it. */}
      <div className="app-main">
        <ErrorBar />
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
                settingsPanes={snapshot.settingsPanes ?? []}
                usagePanes={snapshot.usagePanes ?? []}
                computersPanes={snapshot.computersPanes ?? []}
              />
            </TabLayer>
          ))}
        </BoardHost>
      </div>
      <CommandPalette />
      <NotificationPanel />
      <ActivityPanel />
    </div>
  );
}
