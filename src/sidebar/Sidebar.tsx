import { Fragment, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AgentInfo,
  TabSnapshot,
  newTab,
  openAgentWall,
  zoomPane,
  closeTab,
  setActiveTab,
  renameTab,
} from "../bindings";
import {
  isCurrentEntry,
  sidebarEntries,
  useWorkspaceStore,
} from "../state/workspaceStore";
import { UpdateBanner } from "../update/UpdateBanner";
import { ClaudeBanner } from "../claude/ClaudeBanner";

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const currentTab = useWorkspaceStore((s) => s.currentTab);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  if (!snapshot || !visible) return null;

  // Every tab says how many agents it holds, and lights up when one wants
  // you — the current tab too, so the count stays put when you switch.
  const agentCount = (tab: TabSnapshot): AgentCount | undefined => {
    const agents = snapshot.agents.filter((a) => a.tabId === tab.id);
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
        {/* Tabs, whole; then the current tab's agents, each shown alone
            when clicked. Numbered in that order, which the tab
            shortcuts use. */}
        {sidebarEntries(snapshot, currentTab).map((entry, i, entries) =>
          entry.kind === "tab" ? (
            <TabRow
              key={entry.tab.id}
              tab={entry.tab}
              index={i}
              active={isCurrentEntry(snapshot, entry)}
              agents={agentCount(entry.tab)}
            />
          ) : (
            <Fragment key={entry.agent.paneId}>
              {entries[i - 1]?.kind === "tab" && (
                <div className="sidebar-section">
                  <span>Agents</span>
                  <button
                    className="sidebar-wall"
                    title="Agent wall for this tab (⌘⇧A / Ctrl+Shift+A)"
                    onClick={() => void openAgentWall(entry.agent.paneId)}
                  >
                    ▦ Wall
                  </button>
                </div>
              )}
              <AgentRow
                agent={entry.agent}
                index={i}
                active={isCurrentEntry(snapshot, entry)}
                unread={snapshot.unreadPanes.includes(entry.agent.paneId)}
              />
            </Fragment>
          ),
        )}
      </div>
      <button className="sidebar-new-tab" onClick={() => void newTab()}>
        + New Tab
      </button>
      <ClaudeBanner />
      <UpdateBanner />
    </div>
  );
}

/**
 * One agent: the harness and its role, on one line. Clicking shows its
 * pane alone, filling its tab; the tab's own row shows the whole tab.
 */
function AgentRow({
  agent,
  index,
  active,
  unread,
}: {
  agent: AgentInfo;
  index: number;
  active: boolean;
  unread: boolean;
}) {
  return (
    <div
      className={`tab-row agent${active ? " active" : ""}`}
      onClick={() => void zoomPane(agent.paneId, true)}
      title={agent.cwd ?? undefined}
    >
      <span className="tab-index">{index + 1}</span>
      <span className="tab-harness" aria-label="Claude Code">
        ✳
      </span>
      <span className={`tab-title${unread ? " has-unread" : ""}`}>
        {agent.role ?? "Claude"}
      </span>
      {agent.status === "needsYou" && (
        <span className="tab-agent-needs" title={agent.message ?? "Needs you"}>
          needs you
        </span>
      )}
      {unread && <span className="tab-badge">•</span>}
    </div>
  );
}

/** The agents in a tab. */
interface AgentCount {
  count: number;
  /** One of them needs you, or said something unread. */
  attention: boolean;
}

function TabRow({
  tab,
  index,
  active,
  agents,
}: {
  tab: TabSnapshot;
  index: number;
  active: boolean;
  agents?: AgentCount;
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
      {agents && (
        <span
          className={`tab-agents${agents.attention ? " attention" : ""}`}
          title={`${agents.count} ${agents.count === 1 ? "agent" : "agents"} in this tab`}
        >
          ✳ {agents.count}
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
