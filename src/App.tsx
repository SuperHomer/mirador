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
import { NotificationDto, listNotifications } from "./bindings";
import { readScreenText } from "./terminal/registry";
import { invoke } from "@tauri-apps/api/core";

const resolveScreenRead = (requestId: number, text: string) =>
  invoke<void>("resolve_screen_read", { requestId, text });

export default function App() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const setSnapshot = useWorkspaceStore((s) => s.setSnapshot);
  const config = useConfigStore((s) => s.config);
  const setConfig = useConfigStore((s) => s.setConfig);
  useKeymap();

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
    return () => {
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
            />
          </div>
        ))}
      </div>
      <CommandPalette />
      <NotificationPanel />
    </div>
  );
}
