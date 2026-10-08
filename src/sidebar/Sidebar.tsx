import { Fragment, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  TabSnapshot,
  newTab,
  closeTab,
  setActiveTab,
  renameTab,
} from "../bindings";
import {
  orderedTabs,
  projectName,
  useWorkspaceStore,
} from "../state/workspaceStore";
import { UpdateBanner } from "../update/UpdateBanner";
import { ClaudeBanner } from "../claude/ClaudeBanner";

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const project = useWorkspaceStore((s) => s.project);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  if (!snapshot || !visible) return null;

  // Agents of projects not on screen are not listed; their project's tabs
  // say how many there are, and light up when one wants you.
  const elsewhere = (tab: TabSnapshot): Elsewhere | undefined => {
    if (tab.agent || !tab.project || tab.project === project) return undefined;
    const agents = snapshot.agents.filter((a) => a.project === tab.project);
    if (agents.length === 0) return undefined;
    return {
      count: agents.length,
      attention: agents.some(
        (a) =>
          a.status === "needsYou" || snapshot.unreadPanes.includes(a.paneId),
      ),
    };
  };

  return (
    <div className="sidebar">
      <div className="sidebar-tabs">
        {/* The project's agents below the rest, under a heading of their
            own; numbered in that order, which the tab shortcuts use. */}
        {orderedTabs(snapshot, project).map((tab, i, tabs) => (
          <Fragment key={tab.id}>
            {tab.agent && !tabs[i - 1]?.agent && (
              <div className="sidebar-section" title={tab.project ?? undefined}>
                Agents
                {tab.project && (
                  <span className="sidebar-section-project">
                    {" · "}
                    {projectName(tab.project)}
                  </span>
                )}
              </div>
            )}
            <TabRow
              tab={tab}
              index={i}
              active={tab.id === snapshot.activeTab}
              elsewhere={elsewhere(tab)}
            />
          </Fragment>
        ))}
      </div>
      <button className="sidebar-new-tab" onClick={() => void newTab()}>
        + New Tab
      </button>
      <ClaudeBanner />
      <UpdateBanner />
    </div>
  );
}

/** Agents of a tab's project that the sidebar is not listing. */
interface Elsewhere {
  count: number;
  /** One of them needs you, or said something unread. */
  attention: boolean;
}

function TabRow({
  tab,
  index,
  active,
  elsewhere,
}: {
  tab: TabSnapshot;
  index: number;
  active: boolean;
  elsewhere?: Elsewhere;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(tab.title);

  const commit = () => {
    setEditing(false);
    if (draft !== tab.title) void renameTab(tab.id, draft);
  };

  // An agent's tab is one line: the harness and the role. Its directory,
  // branch and last message are what the agent wall is for.
  if (tab.agent) {
    return (
      <div
        className={`tab-row agent${active ? " active" : ""}`}
        onClick={() => void setActiveTab(tab.id)}
        title={tab.title}
      >
        <span className="tab-index">{index + 1}</span>
        <span className="tab-harness" aria-label="Claude Code">
          ✳
        </span>
        <span className={`tab-title${tab.unread > 0 ? " has-unread" : ""}`}>
          {tab.agent}
        </span>
        {tab.unread > 0 && <span className="tab-badge">{tab.unread}</span>}
        <CloseTab tabId={tab.id} />
      </div>
    );
  }

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
      {elsewhere && (
        <span
          className={`tab-agents${elsewhere.attention ? " attention" : ""}`}
          title="Agents in this project — switch to the tab to list them"
        >
          ✳ {elsewhere.count}
        </span>
      )}
      {tab.unread > 0 && <span className="tab-badge">{tab.unread}</span>}
      <CloseTab tabId={tab.id} />
    </div>
  );
}

function CloseTab({ tabId }: { tabId: string }) {
  return (
    <button
      className="tab-close"
      title="Close tab"
      onClick={(e) => {
        e.stopPropagation();
        void closeTab(tabId);
      }}
    >
      ×
    </button>
  );
}
