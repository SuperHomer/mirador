import { Fragment, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AgentInfo,
  Harness,
  HARNESS_LABEL,
  agentLabel,
  TabSnapshot,
  newTab,
  openAgentWall,
  zoomPane,
  closeTab,
  setActiveTab,
  renameTab,
  setView,
} from "../bindings";
import {
  isCurrentEntry,
  sidebarEntries,
  useWorkspaceStore,
} from "../state/workspaceStore";
import { useUiStore } from "../state/uiStore";
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
      <div className="sidebar-footer">
        <button className="sidebar-new-tab" onClick={() => void newTab()}>
          + New Tab
        </button>
        <div className="view-switch" title="Tabs or board (⌘⇧O / Ctrl+Shift+O)">
          {(["tabs", "board"] as const).map((view) => (
            <button
              key={view}
              className={snapshot.view === view ? "selected" : undefined}
              onClick={() => snapshot.view !== view && void setView(view)}
            >
              {view === "tabs" ? "Tabs" : "Board"}
            </button>
          ))}
        </div>
      </div>
      <ClaudeBanner />
      <UpdateBanner />
    </div>
  );
}

/**
 * The mark each harness draws for itself, in the row's color: OpenCode's
 * and Pi's logos (from their favicons), Codex's `>_` prompt — it has no
 * logo of its own apart from OpenAI's. Claude Code keeps its spark; the
 * rest share one.
 */
function HarnessMark({ harness }: { harness: Harness }) {
  switch (harness) {
    case "claude":
      return <>✳</>;
    case "codex":
      return (
        <svg
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M3 4l4 4-4 4M9 12h4" />
        </svg>
      );
    case "openCode":
      return (
        <svg viewBox="96 96 320 320" fill="currentColor">
          <path d="M320 224V352H192V224H320Z" opacity="0.45" />
          <path
            fillRule="evenodd"
            d="M384 416H128V96H384V416ZM320 160H192V352H320V160Z"
          />
        </svg>
      );
    case "pi":
      return (
        <svg viewBox="0 0 560 560" fill="currentColor">
          <path d="M420 280H280V140H0V0H420V280Z" />
          <path d="M560 560H420V280H560V560Z" />
          <path d="M140 560H0V140H140V280H280V420H140V560Z" />
        </svg>
      );
    default:
      return <>◆</>;
  }
}

/**
 * One agent: the harness and its role, on one line. Clicking shows its
 * pane alone, filling its tab; the tab's own row shows the whole tab.
 * Right-clicking opens its activity: task progress and subagents.
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
      onContextMenu={(e) => {
        e.preventDefault();
        if (agent.hasActivity) useUiStore.getState().openActivity(agent.paneId);
      }}
      title={
        [agent.cwd, agent.hasActivity ? "Right-click for its subagents" : null]
          .filter(Boolean)
          .join(" — ") || undefined
      }
    >
      <span className="tab-index">{index + 1}</span>
      <span
        className={`tab-harness ${agent.harness}`}
        aria-label={HARNESS_LABEL[agent.harness]}
      >
        <HarnessMark harness={agent.harness} />
      </span>
      <span className={`tab-title${unread ? " has-unread" : ""}`}>
        {agentLabel(agent)}
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
            {tab.team && (
              <span
                className="tab-team"
                title={`Team ${tab.team.name}: ${tab.team.members.map((m) => m.name).join(", ")}`}
              >
                ⚑
              </span>
            )}
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
