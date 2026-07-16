import { useState } from "react";
import {
  TabSnapshot,
  newTab,
  closeTab,
  setActiveTab,
  renameTab,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  if (!snapshot || !visible) return null;

  return (
    <div className="sidebar">
      <div className="sidebar-tabs">
        {snapshot.tabs.map((tab, i) => (
          <TabRow
            key={tab.id}
            tab={tab}
            index={i}
            active={tab.id === snapshot.activeTab}
          />
        ))}
      </div>
      <button className="sidebar-new-tab" onClick={() => void newTab()}>
        + New Tab
      </button>
    </div>
  );
}

function TabRow({
  tab,
  index,
  active,
}: {
  tab: TabSnapshot;
  index: number;
  active: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(tab.title);

  const commit = () => {
    setEditing(false);
    if (draft !== tab.title) void renameTab(tab.id, draft);
  };

  return (
    <div
      className={`tab-row${active ? " active" : ""}`}
      onClick={() => void setActiveTab(tab.id)}
      onDoubleClick={() => {
        setDraft(tab.title);
        setEditing(true);
      }}
    >
      <span className="tab-index">{index + 1}</span>
      <div className="tab-info">
        {editing ? (
          <input
            className="tab-rename"
            value={draft}
            autoFocus
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span className={`tab-title${tab.unread > 0 ? " has-unread" : ""}`}>
            {tab.title}
          </span>
        )}
        {tab.unread > 0 && tab.lastNotification ? (
          <span className="tab-notif">{tab.lastNotification}</span>
        ) : (
          tab.cwd && <span className="tab-cwd">{tab.cwd}</span>
        )}
      </div>
      {tab.unread > 0 && <span className="tab-badge">{tab.unread}</span>}
      <button
        className="tab-close"
        title="Close tab"
        onClick={(e) => {
          e.stopPropagation();
          void closeTab(tab.id);
        }}
      >
        ×
      </button>
    </div>
  );
}
