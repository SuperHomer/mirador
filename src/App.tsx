import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
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
import { CommandPalette } from "./palette/CommandPalette";
import { NotificationPanel } from "./notifications/NotificationPanel";
import {
  NotificationDto,
  Node,
  listNotifications,
  setBrowserVisible,
} from "./bindings";
import { useUiStore } from "./state/uiStore";
import { readScreenText, saveAllScrollbacks } from "./terminal/registry";
import { invoke } from "@tauri-apps/api/core";

function collectPaneIds(node: Node): string[] {
  if (node.type === "leaf") return [node.paneId];
  return node.children.flatMap(collectPaneIds);
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
  // inactive tabs, and all of them while an overlay is open.
  useEffect(() => {
    if (!snapshot) return;
    const activeTab = snapshot.tabs.find((t) => t.id === snapshot.activeTab);
    const activePanes = activeTab ? collectPaneIds(activeTab.root) : [];
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
    const onBlur = () => saveAllScrollbacks();
    window.addEventListener("blur", onBlur);
    return () => {
      void unlistenScrollback.then((fn) => fn());
      window.removeEventListener("blur", onBlur);
      void unlistenRead.then((fn) => fn());
      disposed = true;
      void unlistenWs.then((fn) => fn());
      void unlistenCfg.then((fn) => fn());
      void unlistenNotif.then((fn) => fn());
    };
  }, [setSnapshot, setConfig]);

  if (!snapshot || !config) return <div className="app" />;

  return (
    <div className="app">
      <Sidebar />
      <div className="tabs-host">
        {snapshot.tabs.map((tab) => (
          <div
            key={tab.id}
            className="tab-layer"
            style={{
              visibility: tab.id === snapshot.activeTab ? "visible" : "hidden",
            }}
          >
            <SplitLayer
              tab={tab}
              unreadPanes={snapshot.unreadPanes}
              agentPanes={snapshot.agentPanes}
              browserPanes={snapshot.browserPanes}
            />
          </div>
        ))}
      </div>
      <CommandPalette />
      <NotificationPanel />
    </div>
  );
}
