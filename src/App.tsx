import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { workspaceSnapshot, WorkspaceSnapshot } from "./bindings";
import { useWorkspaceStore } from "./state/workspaceStore";
import { useKeymap } from "./keymap/useKeymap";
import { Sidebar } from "./sidebar/Sidebar";
import { SplitLayer } from "./layout/SplitLayer";

export default function App() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const setSnapshot = useWorkspaceStore((s) => s.setSnapshot);
  useKeymap();

  useEffect(() => {
    let disposed = false;
    void workspaceSnapshot().then((s) => {
      if (!disposed) setSnapshot(s);
    });
    const unlisten = listen<WorkspaceSnapshot>("workspace-changed", (e) =>
      setSnapshot(e.payload),
    );
    return () => {
      disposed = true;
      void unlisten.then((fn) => fn());
    };
  }, [setSnapshot]);

  if (!snapshot) return <div className="app" />;

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
            <SplitLayer tab={tab} />
          </div>
        ))}
      </div>
    </div>
  );
}
