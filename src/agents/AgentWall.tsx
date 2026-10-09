import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AgentInfo,
  AgentStatus,
  Node,
  closePane,
  markPaneRead,
  openAgent,
  zoomPane,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import {
  fitTerminal,
  isLent,
  isLentTo,
  lendTerminal,
  watchTerminal,
} from "../terminal/registry";

type Props = {
  paneId: string;
  /** The tab whose agents it shows: the one it was opened from. */
  tabId: string | null;
  /** The wall's tab is the one on screen: only then does it borrow. */
  visible: boolean;
};

function paneIds(node: Node): string[] {
  return node.type === "leaf" ? [node.paneId] : node.children.flatMap(paneIds);
}

const STATUS_LABEL: Record<AgentStatus, string> = {
  working: "working",
  needsYou: "needs you",
  idle: "idle",
};

/** "42s", "3m", "1h" — how long it has been in its status. */
function elapsed(sinceMs: number, now: number): string {
  const s = Math.max(0, Math.floor((now - sinceMs) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h`;
}

/** Columns for `n` tiles: as square as it gets, never taller than wide. */
function columns(n: number): number {
  return Math.max(1, Math.ceil(Math.sqrt(n)));
}

/**
 * Every Claude Code agent's terminal in one grid. The terminals are the
 * panes' own, lent for as long as the wall is on screen (see
 * `lendTerminal`), so a tile is live and takes input — answering a
 * permission prompt from here is the same as in the agent's tab.
 *
 * Agents in the wall's own tab are left out: they are already on screen,
 * and a terminal can only be drawn in one place.
 */
export const AgentWall = memo(function AgentWall({
  paneId,
  tabId,
  visible,
}: Props) {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  // The tab it shows; gone if that tab has closed since.
  const shown = snapshot?.tabs.find((t) => t.id === tabId);
  const roles = useConfigStore((s) => s.config?.agentRoles ?? []);
  const [menuOpen, setMenuOpen] = useState(false);
  const now = useNow(visible);

  const { tiles, tabTitle } = useMemo(() => {
    const tabOf = new Map<string, { id: string; title: string }>();
    for (const t of snapshot?.tabs ?? []) {
      for (const p of paneIds(t.root)) tabOf.set(p, { id: t.id, title: t.title });
    }
    const ownTab = tabOf.get(paneId)?.id;
    const tiles = (snapshot?.agents ?? []).filter(
      (a) => a.tabId === tabId && tabOf.get(a.paneId)?.id !== ownTab,
    );
    return {
      tiles,
      tabTitle: (pane: string) => tabOf.get(pane)?.title ?? "",
    };
  }, [snapshot, paneId, tabId]);

  const unread = snapshot?.unreadPanes ?? [];
  const count = (s: AgentStatus) => tiles.filter((a) => a.status === s).length;
  const cols = columns(tiles.length);

  const start = (role: string | null) => {
    setMenuOpen(false);
    // Like everywhere else, the agent joins a tab rather than getting its
    // own: it splits the focused pane of the tab the wall shows (the
    // wall's own tab is no place to work). That tab is behind the wall, so
    // the wall stays on screen and gains a tile. With that tab gone, a tab
    // of its own, also behind the wall.
    if (shown) void openAgent(role, null, shown.focusedPane, false);
    else void openAgent(role, null, paneId, true, true);
  };

  return (
    <div className="pane agent-wall">
      <div className="wall-chrome">
        <span className="wall-badge">Agents</span>
        {shown && (
          <span className="wall-tab" title="The tab this wall shows the agents of">
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
                <button key={r.name} onClick={() => start(r.name)}>
                  <span>{r.name}</span>
                  <span className="wall-menu-model">
                    {r.model ?? "default model"}
                  </span>
                </button>
              ))}
              {roles.length === 0 && (
                <p className="wall-menu-hint">
                  Add <code>agentRoles</code> to mirador.json for roles with
                  their own model and prompt.
                </p>
              )}
            </div>
          )}
        </div>
        <button
          className="wall-close"
          title="Close agent wall"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>

      {tiles.length === 0 ? (
        <div className="wall-empty">
          <p>
            {shown
              ? `No Claude Code agents in ${shown.title}.`
              : "The tab this wall showed has closed."}
          </p>
          <p className="wall-empty-hint">
            Start one with <em>+ New agent</em>, or run <code>claude</code> in
            a pane of that tab. For another tab's agents, go to the tab and
            open the wall from there.
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
              where={tabTitle(a.paneId)}
              unread={unread.includes(a.paneId)}
              visible={visible}
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
  where,
  unread,
  visible,
  now,
}: {
  agent: AgentInfo;
  where: string;
  unread: boolean;
  visible: boolean;
  now: number;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  // Assumed until a lend fails, so a tile does not flash the placeholder.
  const [shown, setShown] = useState(true);
  const pane = agent.paneId;

  // Give the terminal back only when the tile goes away: while the wall is
  // merely hidden it stays here, until its own tab is shown and takes it
  // (see `lendTerminal` for why every move counts).
  const releaseRef = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      releaseRef.current?.();
      releaseRef.current = null;
    },
    [pane],
  );

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

  const status = agent.status;
  return (
    <div
      className={`wall-tile${status ? ` ${status}` : ""}${unread ? " unread" : ""}`}
    >
      <div
        className="wall-tile-head"
        title="Show this agent in its tab"
        onClick={() => void zoomPane(pane, true)}
      >
        <span className="wall-dot" />
        <span className="wall-role">{agent.role ?? "claude"}</span>
        {agent.model && <span className="wall-model">{agent.model}</span>}
        <span className="wall-where">
          {where}
          {agent.branch && ` · ${agent.branch}`}
        </span>
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
      <div
        className="wall-term"
        onMouseDown={() => {
          if (unread) void markPaneRead(pane);
        }}
      >
        {/* The terminal's element is appended here by `lendTerminal`, so
            React must own nothing inside it. */}
        <div className="wall-term-host" ref={hostRef} />
        {!shown && <span className="wall-term-missing">shown elsewhere</span>}
      </div>
    </div>
  );
}

/** The current time, ticking each second while `active`. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}
