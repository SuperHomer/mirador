import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AgentInfo,
  Harness,
  HARNESS_LABEL,
  agentLabel,
  TabSnapshot,
  ToolKind,
  WallTool,
  focusPane,
  newTab,
  zoomPane,
  closeTab,
  setActiveTab,
  renameTab,
  setView,
  setWallView,
  tabAgents,
  toggleAgentWall,
  waitingAgents,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useUiStore } from "../state/uiStore";
import { useConfigStore } from "../state/configStore";
import { STATUS_LABEL, StatusMark, StatusPips } from "../agents/StatusMark";
import { elapsed, useNow } from "../agents/time";
import { AskingLine } from "../agents/AskingLine";
import { showOnWall } from "../agents/showOnWall";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { UpdateBanner } from "../update/UpdateBanner";
import { ClaudeBanner } from "../claude/ClaudeBanner";

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const currentTab = useWorkspaceStore((s) => s.currentTab);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  const accels = acceleratorsByAction(
    useConfigStore((s) => s.config?.keybindings),
  );
  if (!snapshot || !visible) return null;
  const accelOf = (action: string) => {
    const accel = accels.get(action);
    return accel ? formatAccel(accel) : undefined;
  };
  const tab = snapshot.tabs.find((t) => t.id === currentTab);
  const waiting = waitingAgents(snapshot).length;

  return (
    <div className="sidebar">
      {tab && <ViewSwitch view={snapshot.view} tab={tab} accelOf={accelOf} />}
      <div className="sidebar-tabs">
        {/* Tabs, numbered for the tab shortcuts. The current tab's agents
            and tools hang under its row; any other tab shows a pip per
            agent. */}
        {snapshot.tabs.map((tab, i) => {
          const agents = tabAgents(snapshot, tab.id);
          const current = tab.id === currentTab;
          return (
            <div key={tab.id} className="sidebar-tab">
              <TabRow
                tab={tab}
                index={i}
                active={current && !tab.zoomedPane}
                pips={current ? [] : agents}
              />
              {current && (agents.length > 0 || tab.wallTools.length > 0) && (
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
          + New tab
        </button>
        <button
          className={`sidebar-button${waiting > 0 ? " waiting" : ""}`}
          title={
            waiting > 0
              ? `${waiting} ${waiting === 1 ? "agent needs" : "agents need"} you`
              : "Notifications"
          }
          onClick={() => useUiStore.getState().toggleNotifications()}
        >
          Inbox {waiting}
        </button>
        <button
          className="sidebar-button"
          onClick={() => useUiStore.getState().togglePalette()}
        >
          Commands
          {accelOf("command_palette") && (
            <span className="sidebar-accel">{accelOf("command_palette")}</span>
          )}
        </button>
      </div>
      <ClaudeBanner />
      <UpdateBanner />
    </div>
  );
}

type Shown = "splits" | "wall" | "tasks" | "board";

/**
 * What the current tab shows, and the way to each other view of it: its
 * splits, its wall's agents, its wall's task board, or every tab on the
 * board. Every button but Board also leaves the board.
 */
function ViewSwitch({
  view,
  tab,
  accelOf,
}: {
  view: "tabs" | "board";
  tab: TabSnapshot;
  accelOf: (action: string) => string | undefined;
}) {
  const shown: Shown =
    view === "board"
      ? "board"
      : !tab.wall
        ? "splits"
        : tab.wallView === "tasks"
          ? "tasks"
          : "wall";
  const pane = tab.focusedPane;
  const show = async (next: Shown) => {
    if (next === "board") return setView("board");
    if (view === "board") await setView("tabs");
    if (next === "splits") {
      if (tab.wall) await toggleAgentWall(pane);
    } else {
      await setWallView(pane, next === "wall" ? "agents" : "tasks");
    }
  };
  const hint = (title: string, action?: string) => {
    const accel = action && accelOf(action);
    return accel ? `${title} (${accel})` : title;
  };
  const buttons: [Shown, string, string][] = [
    ["splits", "Splits", hint("This tab's splits", "agent_wall")],
    ["wall", "Wall", hint("This tab's agents, as the wall", "agent_wall")],
    ["tasks", "Tasks", hint("This tab's task board", "task_board")],
    ["board", "Board", hint("Every tab on one board", "toggle_board")],
  ];
  return (
    <div className="view-switch">
      {buttons.map(([id, label, title]) => (
        <button
          key={id}
          className={shown === id ? "selected" : undefined}
          title={title}
          onClick={() => shown !== id && void show(id)}
        >
          {label}
        </button>
      ))}
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

/** What a tool is, in the same neutral line as the harness marks. */
function ToolMark({ kind }: { kind: ToolKind }) {
  const props = {
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  switch (kind) {
    case "terminal":
      return (
        <svg {...props}>
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
          <path d="M4.5 6l2 2-2 2M8.5 10.5h3" />
        </svg>
      );
    case "browser":
      return (
        <svg {...props}>
          <circle cx="8" cy="8" r="6.5" />
          <path d="M1.5 8h13M8 1.5c-2.2 2.2-2.2 10.8 0 13M8 1.5c2.2 2.2 2.2 10.8 0 13" />
        </svg>
      );
    case "diff":
      return (
        <svg {...props}>
          <path d="M5 2v6M2 5h6M2 12.5h6M10.5 3.5h3.5v9" />
        </svg>
      );
    case "graph":
      return (
        <svg {...props}>
          <circle cx="4" cy="3.5" r="1.5" />
          <circle cx="4" cy="12.5" r="1.5" />
          <circle cx="12" cy="6" r="1.5" />
          <path d="M4 5v6M12 7.5c0 2.5-3 3-8 3.5" />
        </svg>
      );
  }
}

/** The current tab's agents, then its tools, under its row. Mounted only
 *  while there are some to show, and the clock ticks only for agents. */
function AgentTree({
  tab,
  agents,
  unread,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  unread: string[];
}) {
  const now = useNow(agents.length > 0);
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
      {tab.wallTools.map((tool) => (
        <ToolRow key={tool.paneId} tool={tool} />
      ))}
    </div>
  );
}

/** One tool: what it is and its title, with no status — a tool is not
 *  waiting on anyone. Clicking focuses its pane. */
function ToolRow({ tool }: { tool: WallTool }) {
  return (
    <div
      className="agent-row tool-row"
      onClick={() => void focusPane(tool.paneId)}
      title={tool.title}
    >
      <div className="agent-line">
        <span className="agent-mark tool-mark">
          <ToolMark kind={tool.kind} />
        </span>
        <span className="agent-name">{tool.title}</span>
      </div>
    </div>
  );
}

/**
 * One agent: its status, its name and how long it has been so. Clicking
 * shows it on its tab's wall, large and pinned, or — with `agentClick`
 * set to "pane" — its pane alone, filling its tab.
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
    showOnWall(agent.tabId, agent.paneId);
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
      title={agent.cwd ?? undefined}
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
      {status === "needsYou" && <AskingLine agent={agent} className="agent-prompt" />}
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
      <StatusPips agents={pips} />
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
