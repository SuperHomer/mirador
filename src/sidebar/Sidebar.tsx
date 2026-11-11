import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AccountUsage,
  AgentInfo,
  Harness,
  HARNESS_LABEL,
  agentLabel,
  TabSnapshot,
  WallTool,
  focusPane,
  zoomPane,
  closeTab,
  setActiveTab,
  renameTab,
  setView,
  setWallView,
  tabAgents,
  toggleAgentWall,
  waitingAgents,
  accountUsage,
  openUsage,
  TokenTotals,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useUiStore } from "../state/uiStore";
import { useClaudeStore } from "../state/claudeStore";
import { opensInTab, useConfigStore } from "../state/configStore";
import { STATUS_LABEL, StatusMark, StatusPips } from "../agents/StatusMark";
import { elapsed, useNow } from "../agents/time";
import { AskingLine } from "../agents/AskingLine";
import { showOnWall } from "../agents/showOnWall";
import { ToolMark } from "../agents/ToolMark";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { UpdateBanner } from "../update/UpdateBanner";
import { fullness } from "../agents/AgentStatusBar";
import { tokens } from "../agents/ActivityPanel";
import { clock } from "../usage/UsagePane";
import { ClaudeBanner } from "../claude/ClaudeBanner";
import { SIDEBAR_WIDTH, useDragWidth } from "../layout/useDragWidth";

/** What the sidebar's edge leaves the rest of the window at least. */
const MAIN_MIN_WIDTH = 320;

export function Sidebar() {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const currentTab = useWorkspaceStore((s) => s.currentTab);
  const visible = useWorkspaceStore((s) => s.sidebarVisible);
  const accels = acceleratorsByAction(
    useConfigStore((s) => s.config?.keybindings),
  );
  const resize = useDragWidth(
    "sidebarWidth",
    useConfigStore((s) => s.config?.sidebarWidth ?? null),
    SIDEBAR_WIDTH,
    "right",
  );
  if (!snapshot || !visible) return null;
  const accelOf = (action: string) => {
    const accel = accels.get(action);
    return accel ? formatAccel(accel) : undefined;
  };
  const tab = snapshot.tabs.find((t) => t.id === currentTab);
  const waiting = waitingAgents(snapshot).length;

  return (
    // Until the config is read the width is styles.css's, the default.
    <div className="sidebar" style={resize.width === null ? undefined : { width: resize.width }}>
      <div
        className={`width-handle${resize.dragging ? " dragging" : ""}`}
        title="Drag to resize · double-click for the default width"
        onPointerDown={(e) =>
          resize.onPointerDown(
            e,
            e.currentTarget.parentElement?.getBoundingClientRect().width ?? SIDEBAR_WIDTH.min,
            window.innerWidth - MAIN_MIN_WIDTH,
          )
        }
        onDoubleClick={resize.onDoubleClick}
      />
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
      <UsageSection pane={tab?.focusedPane ?? null} />
      <div className="sidebar-footer">
        <button
          className={`sidebar-button${waiting > 0 ? " waiting" : ""}`}
          title={
            waiting > 0
              ? `${waiting} ${waiting === 1 ? "agent needs" : "agents need"} you`
              : "Notifications"
          }
          onClick={() => useUiStore.getState().toggleNotifications()}
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3">
            <path d="M2 9.5 3.8 3h8.4L14 9.5V13H2z" />
            <path d="M2 9.5h3.5l1 1.5h3l1-1.5H14" />
          </svg>
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

const LOGIN_LABEL = {
  subscription: "Subscription",
  apiKey: "API key",
  planOrApiKey: "Plan or API key",
  none: "Not signed in",
};

const total = (t: TokenTotals) => t.input + t.cacheWrite + t.cacheRead + t.output;

/**
 * Each harness's account, always in view: read on mount and every minute
 * (through the app's usage cache, so a read costs only what was appended).
 * Nothing shows until the first answer; a refresh keeps the last one up
 * until the next arrives. An account not in use (`inUse`, decided in
 * cmux-core: no login and no tokens, a harness never used here) is left
 * out; the Usage pane still lists it.
 */
function UsageSection({ pane }: { pane: string | null }) {
  const [accounts, setAccounts] = useState<AccountUsage[] | null>(null);
  const [open, setOpen] = useState<Harness[]>([]);
  useEffect(() => {
    // StrictMode mounts twice in dev: the first mount's cleanup clears its
    // interval, and its late answer is dropped.
    let live = true;
    const read = () =>
      void accountUsage()
        .then((a) => live && setAccounts(a))
        .catch(() => {});
    read();
    const timer = setInterval(read, 60_000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);
  const shown = (accounts ?? []).filter((a) => a.inUse);
  if (shown.length === 0) return null;
  const toggle = (h: Harness) =>
    setOpen((o) => (o.includes(h) ? o.filter((x) => x !== h) : [...o, h]));
  return (
    <div className="usage-section">
      <div className="usage-section-head">Usage</div>
      {shown.map((a) => (
        <UsageRow
          key={a.harness}
          account={a}
          open={open.includes(a.harness)}
          onToggle={() => toggle(a.harness)}
          onOpenPane={() => void openUsage(pane, opensInTab("editor"))}
          onSetUpLimits={() =>
            void useClaudeStore
              .getState()
              .run()
              .then(() => accountUsage())
              .then(setAccounts)
              .catch(() => {})
          }
        />
      ))}
    </div>
  );
}

/** One account: its icon and name, then its most used window, or for an
 *  account with none its tokens in the last five hours. Opens to the rest. */
function UsageRow({
  account: a,
  open,
  onToggle,
  onOpenPane,
  onSetUpLimits,
}: {
  account: AccountUsage;
  open: boolean;
  onToggle: () => void;
  onOpenPane: () => void;
  onSetUpLimits: () => void;
}) {
  const settingUp = useClaudeStore((s) => s.state === "running");
  const worst = a.limits[0];
  const cost = (t: TokenTotals) => (t.cost !== null ? ` · $${t.cost.toFixed(2)}` : "");
  return (
    <div className={`usage-row${open ? " open" : ""}`}>
      <button className="usage-row-line" onClick={onToggle} aria-expanded={open}>
        <span className="tab-harness" aria-hidden="true">
          <HarnessMark harness={a.harness} />
        </span>
        <span className="usage-row-name">{HARNESS_LABEL[a.harness]}</span>
        {a.login === "none" ? (
          <span className="usage-row-value muted">not signed in</span>
        ) : worst ? (
          <>
            <span className={`agent-bar-meter usage-row-meter ${fullness(worst.usedPercent)}`}>
              <span style={{ "--fill": `${Math.min(100, worst.usedPercent)}%` } as React.CSSProperties} />
            </span>
            <span className={`usage-row-value pct ${fullness(worst.usedPercent)}`}>
              {Math.round(worst.usedPercent)}%
            </span>
          </>
        ) : (
          <span className="usage-row-value">
            {tokens(total(a.fiveHours))}
            <span className="muted"> · 5h{cost(a.fiveHours)}</span>
          </span>
        )}
      </button>
      {open && (
        <div className="usage-row-detail">
          <div className="usage-kv">
            <span>{LOGIN_LABEL[a.login]}</span>
            <span>{a.plan ?? ""}</span>
          </div>
          {a.provider && <div className="usage-note">via {a.provider}</div>}
          {a.note && <div className="usage-note">{a.note}</div>}
          {a.limits.map((w) => (
            <div key={w.windowMinutes} className="usage-kv">
              <span>
                {w.name} · {w.limitReached ? "limit reached" : `${Math.round(w.usedPercent)}%`}
              </span>
              {w.resetsAt !== null && (
                <span>
                  resets{" "}
                  {new Date(w.resetsAt * 1000).toLocaleString(undefined, {
                    day: "numeric",
                    month: "short",
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </span>
              )}
            </div>
          ))}
          {a.asOf !== null && a.limits.length > 0 && <div className="usage-note">as of {clock(a.asOf)}</div>}
          {a.limitsNote && <div className="usage-note">{a.limitsNote}</div>}
          {a.limitsSetup && (
            <button className="usage-open" disabled={settingUp} onClick={onSetUpLimits}>
              Show limits
            </button>
          )}
          <div className="usage-kv">
            <span>Last 5h</span>
            <span>
              {tokens(total(a.fiveHours))}
              {cost(a.fiveHours)}
            </span>
          </div>
          <div className="usage-kv">
            <span>Last 7d</span>
            <span>
              {tokens(total(a.sevenDays))}
              {cost(a.sevenDays)}
            </span>
          </div>
          <button className="usage-open" onClick={onOpenPane}>
            Open Usage pane
          </button>
        </div>
      )}
    </div>
  );
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
  const onWall = tab.wallPages.flat();
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
        <ToolRow
          key={tool.paneId}
          tabId={tab.id}
          tool={tool}
          click={onWall.includes(tool.paneId) ? click : null}
          shown={
            click === "wall"
              ? tab.wall && pinned === tool.paneId
              : tab.zoomedPane === tool.paneId
          }
        />
      ))}
    </div>
  );
}

/** One tool: what it is and its title, with no status — a tool is not
 *  waiting on anyone. Clicking one the wall draws does what clicking an
 *  agent does; any other, it focuses its pane. */
function ToolRow({
  tabId,
  tool,
  click,
  shown,
}: {
  tabId: string;
  tool: WallTool;
  /** What a click does, as for an agent; null when it is not on the wall. */
  click: "wall" | "pane" | null;
  /** It is what the tab shows: pinned on the wall, or the zoomed pane. */
  shown: boolean;
}) {
  const open = () => {
    if (click === "wall") showOnWall(tabId, tool.paneId);
    else if (click === "pane") void zoomPane(tool.paneId, true);
    else void focusPane(tool.paneId);
  };
  const classes = ["agent-row", "tool-row", click && shown && (click === "wall" ? "pinned" : "active")];
  return (
    <div className={classes.filter(Boolean).join(" ")} onClick={open} title={tool.title}>
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
                {tab.team.remote && ` · on ${tab.team.remote.host}`}
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
