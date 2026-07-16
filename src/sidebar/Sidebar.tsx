import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
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
        {(tab.branch || tab.pr || tab.ports.length > 0) && (
          <span className="tab-intel">
            {tab.branch && <span className="tab-branch">⎇ {tab.branch}</span>}
            {tab.pr && (
              <button
                className={`tab-pr ${tab.pr.checks} ${tab.pr.state.toLowerCase()}`}
                title={`PR #${tab.pr.number} — ${tab.pr.state}, checks: ${tab.pr.checks}`}
                onClick={(e) => {
                  e.stopPropagation();
                  if (tab.pr) void openUrl(tab.pr.url);
                }}
              >
                #{tab.pr.number}
                {tab.pr.checks === "pass" && " ✓"}
                {tab.pr.checks === "fail" && " ✗"}
                {tab.pr.checks === "pending" && " ●"}
              </button>
            )}
            {tab.ports.map((port) => (
              <button
                key={port}
                className="tab-port"
                title={`open http://localhost:${port}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void openUrl(`http://localhost:${port}`);
                }}
              >
                :{port}
              </button>
            ))}
          </span>
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
