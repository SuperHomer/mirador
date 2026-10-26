import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AgentInfo,
  Harness,
  HARNESS_LABEL,
  agentLabel,
  TabSnapshot,
  newTab,
  zoomPane,
  closeTab,
  setActiveTab,
  renameTab,
  setView,
  setWallMode,
  setWallView,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useUiStore } from "../state/uiStore";
import { useConfigStore } from "../state/configStore";
import { STATUS_LABEL, StatusMark, StatusPip } from "../agents/StatusMark";
import { elapsed, useNow } from "../agents/time";
import { UpdateBanner } from "../update/UpdateBanner";
import { ClaudeBanner } from "../claude/ClaudeBanner";

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const currentTab = useWorkspaceStore((s) => s.currentTab);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  if (!snapshot || !visible) return null;

  return (
    <div className="sidebar">
      <div className="sidebar-tabs">
        {/* Tabs, numbered for the tab shortcuts. The current tab's agents
            hang under its row; any other tab shows a pip per agent. */}
        {snapshot.tabs.map((tab, i) => {
          const agents = snapshot.agents.filter((a) => a.tabId === tab.id);
          const current = tab.id === currentTab;
          return (
            <div key={tab.id} className="sidebar-tab">
              <TabRow
                tab={tab}
                index={i}
                active={current && !tab.zoomedPane}
                pips={current ? [] : agents}
              />
              {current && agents.length > 0 && (
                <AgentTree
                  tab={tab}
                  agents={agents}
                  unread={snapshot.unreadPanes}
                />
              )}
            </div>
          );
        })}
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
 * The mark each harness draws for itself, in the row's neutral color — so
 * no harness's colour competes with a status's: OpenCode's
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

/** The current tab's agents, under its row. Mounted only while there are
 *  some to show, so the clock ticks only then. */
function AgentTree({
  tab,
  agents,
  unread,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  unread: string[];
}) {
  const now = useNow(true);
  const click = useConfigStore((s) => s.config?.agentClick ?? "wall");
  const pinned = useUiStore((s) => s.pinned[tab.id]);
  return (
    <div className="agent-tree">
      {agents.map((agent) => (
        <AgentRow
          key={agent.paneId}
          agent={agent}
          now={now}
          click={click}
          shown={
            click === "wall"
              ? tab.wall && pinned === agent.paneId
              : tab.zoomedPane === agent.paneId
          }
          unread={unread.includes(agent.paneId)}
        />
      ))}
    </div>
  );
}

/**
 * One agent: its status, its name and how long it has been so. Clicking
 * shows it on its tab's wall, large and pinned, or — with `agentClick`
 * set to "pane" — its pane alone, filling its tab. Right-clicking opens
 * its activity: task progress and subagents.
 */
function AgentRow({
  agent,
  now,
  click,
  shown,
  unread,
}: {
  agent: AgentInfo;
  now: number;
  click: "wall" | "pane";
  /** It is what the tab shows: pinned on the wall, or the zoomed pane. */
  shown: boolean;
  unread: boolean;
}) {
  const open = () => {
    if (click === "pane") {
      void zoomPane(agent.paneId, true);
      return;
    }
    useUiStore.getState().pinAgent(agent.tabId, agent.paneId);
    void (async () => {
      await setWallMode(agent.paneId, "focus");
      await setWallView(agent.paneId, "agents");
    })();
  };
  const status = agent.status;
  const classes = [
    "agent-row",
    status === "needsYou" && "needs-you",
    status === "idle" && "idle",
    shown && (click === "wall" ? "pinned" : "active"),
  ];
  return (
    <div
      className={classes.filter(Boolean).join(" ")}
      onClick={open}
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
      <div className="agent-line">
        <span className="agent-mark">
          <StatusMark status={status} small />
        </span>
        <span className={`agent-name${unread ? " has-unread" : ""}`}>
          {agentLabel(agent)}
        </span>
        <span
          className="tab-harness"
          aria-label={HARNESS_LABEL[agent.harness]}
          title={HARNESS_LABEL[agent.harness]}
        >
          <HarnessMark harness={agent.harness} />
        </span>
        {unread && <span className="tab-badge">•</span>}
        {status && (
          <span className={`agent-status ${status}`}>
            {STATUS_LABEL[status]}
            {agent.sinceMs ? ` · ${elapsed(agent.sinceMs, now)}` : ""}
          </span>
        )}
      </div>
      {status === "needsYou" && agent.message && (
        <div className="agent-prompt">{agent.message}</div>
      )}
    </div>
  );
}

function TabRow({
  tab,
  index,
  active,
  pips,
}: {
  tab: TabSnapshot;
  index: number;
  active: boolean;
  /** Agents to show a status pip for: none for the current tab, whose
   *  agents are listed under it. */
  pips: AgentInfo[];
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
          <span className="tab-head">
            <span className={`tab-title${tab.unread > 0 ? " has-unread" : ""}`}>
              {tab.title}
            </span>
            {tab.team && (
              <span
                className="tab-team"
                title={`Team ${tab.team.name}: ${tab.team.members.map((m) => m.name).join(", ")}`}
              >
                team · {tab.team.name}
                {tab.team.members.length > 4 && ` · ${tab.team.members.length}`}
              </span>
            )}
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
      {pips.length > 0 && (
        <span className="tab-pips">
          {pips.map((a) => (
            <StatusPip key={a.paneId} status={a.status} />
          ))}
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
