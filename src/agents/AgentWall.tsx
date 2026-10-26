import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AgentRole,
  agentRoles,
  roleHint,
  AgentInfo,
  agentLabel,
  AgentStatus,
  focusPane,
  markPaneRead,
  openAgent,
  setWallView,
  toggleAgentWall,
  WallView,
  zoomPane,
} from "../bindings";
import { SubagentList, percent, useAgentActivity } from "./ActivityPanel";
import { STATUS_LABEL } from "./StatusMark";
import { elapsed, useNow } from "./time";
import { TaskBoard } from "./TaskBoard";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import {
  fitTerminal,
  getTerminal,
  isLent,
  isLentTo,
  lendTerminal,
  watchTerminal,
} from "../terminal/registry";

type Props = {
  /** The tab whose agents it shows, and in place of whose splits. */
  tabId: string;
  /** It is on screen: only then does it borrow. On the board, every
   *  tab's wall is. */
  visible: boolean;
  /** Its tab is the active one, where the keyboard goes. */
  current: boolean;
};

/** Columns for `n` tiles: as square as it gets, never taller than wide. */
function columns(n: number): number {
  return Math.max(1, Math.ceil(Math.sqrt(n)));
}

/**
 * A tab's agents' terminals in one grid — Claude Code, Codex, OpenCode, …,
 * drawn in place of the tab's splits. The terminals are the panes' own,
 * lent for as long as the wall is on screen (see `lendTerminal`), so a
 * tile is live and takes input — answering a permission prompt from here
 * is the same as in the agent's pane.
 */
export const AgentWall = memo(function AgentWall({
  tabId,
  visible,
  current,
}: Props) {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const shown = snapshot?.tabs.find((t) => t.id === tabId);
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [menuOpen, setMenuOpen] = useState(false);
  const now = useNow(visible);

  const tiles = useMemo(
    () => (snapshot?.agents ?? []).filter((a) => a.tabId === tabId),
    [snapshot, tabId],
  );

  // Its tiles, or the tab's task board in their place. On the Tasks side
  // no tile is mounted, so each lent terminal has gone back to its own
  // pane under the wall, and the Agents side borrows them again.
  const side: WallView = shown?.wallView ?? "agents";
  const tilesShown = visible && side === "agents";

  // The tab's focused pane is under the wall, so the keyboard would go
  // nowhere: as the tiles come on screen in the active tab — on the board,
  // as their tab becomes it — the wall gives it to that pane's tile, or
  // the first. Only then — not on every snapshot, or a click in another
  // tile would be undone. The tiles' own effects ran first, so their
  // terminals are here by now — unless another wall holds one, and then
  // it is not this wall's to focus.
  const wallRef = useRef<HTMLDivElement>(null);
  const keyboard = tilesShown && current;
  useEffect(() => {
    if (!keyboard) return;
    const tile = tiles.find((a) => a.paneId === shown?.focusedPane) ?? tiles[0];
    const term = tile && getTerminal(tile.paneId);
    if (term?.element && wallRef.current?.contains(term.element)) term.focus();
  }, [keyboard]);

  const unread = snapshot?.unreadPanes ?? [];
  const count = (s: AgentStatus) => tiles.filter((a) => a.status === s).length;
  const cols = columns(tiles.length);

  // The roles of the directory a new agent would work in: the shown tab's
  // focused pane's, read each time the menu opens.
  const rolesPane = shown?.focusedPane;
  useEffect(() => {
    if (!menuOpen || !rolesPane) return;
    let live = true;
    agentRoles(rolesPane)
      .then((c) => live && setRoles(c.roles))
      .catch(() => live && setRoles([]));
    return () => {
      live = false;
    };
  }, [menuOpen, rolesPane]);

  const start = (role: string | null) => {
    setMenuOpen(false);
    if (!shown) return;
    // Like everywhere else, the agent joins a tab rather than getting its
    // own: it splits the tab's focused pane, under the wall, so the wall
    // stays on screen and gains a tile.
    openAgent(role, null, shown.focusedPane, false).catch((e) =>
      console.error(`mirador: role ${role}: ${e}`),
    );
  };

  return (
    <div className="pane agent-wall" ref={wallRef}>
      <div className="wall-chrome">
        <span className="wall-badge">Agents</span>
        {shown && (
          <span className="wall-tab" title="The tab whose agents these are">
            {shown.title}
          </span>
        )}
        <span className="wall-counts">
          {tiles.length === 0 ? (
            "none running"
          ) : (
            <>
              {count("needsYou") > 0 && (
                <span className="wall-count needs-you">
                  {count("needsYou")} need you
                </span>
              )}
              <span className="wall-count working">
                {count("working")} working
              </span>
              <span className="wall-count idle">{count("idle")} idle</span>
            </>
          )}
        </span>
        {shown && (
          // Splits leaves the wall, so it is never the side shown here.
          <div className="tb-switch" role="group">
            <button
              title="Show this tab's splits (⌘⇧A / Ctrl+Shift+A)"
              onClick={() => void toggleAgentWall(shown.focusedPane)}
            >
              Splits
            </button>
            <button
              className={side === "agents" ? "on" : ""}
              title="This tab's agents' terminals"
              onClick={() => void setWallView(shown.focusedPane, "agents")}
            >
              Agents
            </button>
            <button
              className={side === "tasks" ? "on" : ""}
              title="The cards of the tasks this tab's agents were given"
              onClick={() => void setWallView(shown.focusedPane, "tasks")}
            >
              Tasks
            </button>
          </div>
        )}
        <div className="wall-new">
          <button
            className="wall-new-button"
            title="Start a Claude Code agent"
            onClick={() => setMenuOpen((o) => !o)}
          >
            + New agent
          </button>
          {menuOpen && (
            <div className="wall-menu" onMouseLeave={() => setMenuOpen(false)}>
              <button onClick={() => start(null)}>
                <span>Claude</span>
                <span className="wall-menu-model">default model</span>
              </button>
              {roles.map((r) => (
                <button
                  key={r.name}
                  disabled={!!r.problem}
                  title={r.problem ?? r.description ?? undefined}
                  onClick={() => start(r.name)}
                >
                  <span>{r.name}</span>
                  <span className="wall-menu-model">{roleHint(r)}</span>
                </button>
              ))}
              {roles.length === 0 && (
                <p className="wall-menu-hint">
                  Add <code>.mirador/roles/&lt;name&gt;.md</code> to the
                  repository for roles with their own model, prompt and way of
                  working.
                </p>
              )}
            </div>
          )}
        </div>
      </div>

      {side === "tasks" ? (
        <TaskBoard tabId={tabId} visible={visible} />
      ) : tiles.length === 0 ? (
        <div className="wall-empty">
          <p>No agents in {shown?.title ?? "this tab"}.</p>
          <p className="wall-empty-hint">
            Start one with <em>+ New agent</em>, or run <code>claude</code>,{" "}
            <code>codex</code>, <code>opencode</code> or another agent in a pane
            of the tab: <em>Splits</em> shows them.
          </p>
        </div>
      ) : (
        <div
          className="wall-grid"
          style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
        >
          {tiles.map((a) => (
            <WallTile
              key={a.paneId}
              agent={a}
              unread={unread.includes(a.paneId)}
              visible={visible}
              current={current}
              now={now}
            />
          ))}
        </div>
      )}
    </div>
  );
});

function WallTile({
  agent,
  unread,
  visible,
  current,
  now,
}: {
  agent: AgentInfo;
  unread: boolean;
  visible: boolean;
  current: boolean;
  now: number;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  // Assumed until a lend fails, so a tile does not flash the placeholder.
  const [shown, setShown] = useState(true);
  const pane = agent.paneId;

  const releaseRef = useRef<(() => void) | null>(null);

  // Borrow the pane's terminal whenever the wall is on screen. The pane may
  // register after the wall mounts (tabs mount in order), remount, or have
  // been taken back while the wall was hidden; a terminal lent nowhere is
  // picked up whenever it appears.
  useEffect(() => {
    const host = hostRef.current;
    if (!visible || !host) return;
    const borrow = () => {
      if (isLentTo(pane, host)) {
        setShown(true);
        return;
      }
      if (isLent(pane)) {
        setShown(false);
        return;
      }
      const release = lendTerminal(pane, host);
      if (release) releaseRef.current = release;
      setShown(!!release);
    };
    borrow();
    // The tile may have changed size while the wall was hidden.
    if (isLentTo(pane, host)) fitTerminal(pane);
    const unwatch = watchTerminal(pane, borrow);
    const observer = new ResizeObserver(() => {
      if (isLentTo(pane, host)) fitTerminal(pane);
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      unwatch();
    };
  }, [pane, visible]);

  // Give the terminal back only when the tile goes away: while the wall is
  // merely hidden it stays here, until its own tab is shown and takes it
  // (see `lendTerminal` for why every move counts). Declared after the
  // borrow so that React, which runs cleanups in order, unwatches first:
  // the release announces the terminal free, and a tile still listening
  // would borrow it again into a host leaving the page.
  useEffect(
    () => () => {
      releaseRef.current?.();
      releaseRef.current = null;
    },
    [pane],
  );

  // Its subagents, read while the wall is on screen, for an agent that
  // has them to read (`hasActivity`, decided in Rust).
  const { activity } = useAgentActivity(pane, visible && agent.hasActivity);
  const tasks = activity?.session.tasks ?? null;

  const status = agent.status;
  return (
    <div
      className={`wall-tile${status ? ` ${status}` : ""}${unread ? " unread" : ""}`}
    >
      <div
        className="wall-tile-head"
        title={
          "Show this agent alone in its tab" +
          (agent.hasActivity ? " — right-click for its subagents" : "")
        }
        onClick={() => void zoomPane(pane, true)}
        onContextMenu={(e) => {
          e.preventDefault();
          if (agent.hasActivity) useUiStore.getState().openActivity(pane);
        }}
      >
        <span className="wall-dot" />
        <span className="wall-role">{agentLabel(agent)}</span>
        {agent.role && agent.name && agent.role !== agent.name && (
          <span className="wall-model">{agent.role}</span>
        )}
        {agent.model && <span className="wall-model">{agent.model}</span>}
        <span className="wall-where">
          {[agent.topic, agent.branch].filter(Boolean).join(" · ")}
        </span>
        {tasks && (
          <span className="wall-tasks" title={tasks.current ?? "Its task list"}>
            {tasks.done}/{tasks.total} · {percent(tasks)}%
          </span>
        )}
        <span className="wall-status">
          {status ? STATUS_LABEL[status] : "—"}
          {status && agent.sinceMs ? ` ${elapsed(agent.sinceMs, now)}` : ""}
        </span>
      </div>
      {status === "needsYou" && agent.message && (
        <div className="wall-message" title={agent.message}>
          {agent.message}
        </div>
      )}
      <div className="wall-body">
        <div
          className="wall-term"
          data-pane={pane}
          onMouseDown={() => {
            if (unread) void markPaneRead(pane);
            // On the board, a wall in another tab's frame: its tab becomes
            // the active one, with this agent focused, so the keyboard
            // stays in the tile pressed.
            if (!current) void focusPane(pane);
          }}
        >
          {/* The terminal's element is appended here by `lendTerminal`, so
              React must own nothing inside it. */}
          <div className="wall-term-host" ref={hostRef} />
          {!shown && <span className="wall-term-missing">shown elsewhere</span>}
        </div>
        {activity && (activity.subagents.length > 0 || tasks) && (
          <aside className="wall-side">
            <div className="wall-side-head">
              <span>Subagents · {activity.subagents.length}</span>
              <button
                title="The full activity panel"
                onClick={() => useUiStore.getState().openActivity(pane)}
              >
                ⤢
              </button>
            </div>
            <SubagentList activity={activity} now={now} compact />
          </aside>
        )}
      </div>
    </div>
  );
}
