import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AgentRole,
  agentRoles,
  roleHint,
  AgentInfo,
  agentLabel,
  openAgent,
  setAgentsPerTab,
  setWallMode,
  WallMode,
  TabSnapshot,
  WallView,
} from "../bindings";
import { useNow } from "./time";
import { TaskBoard } from "./TaskBoard";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useUiStore, WallFilter } from "../state/uiStore";
import { useConfigStore } from "../state/configStore";
import { getTerminal } from "../terminal/registry";
import { FocusLayout } from "./FocusLayout";
import { GridLayout } from "./GridLayout";

type Props = {
  /** The tab whose agents it shows, and in place of whose splits. */
  tabId: string;
  /** It is on screen: only then does it borrow. On the board, every
   *  tab's wall is. */
  visible: boolean;
  /** Its tab is the active one, where the keyboard goes. */
  current: boolean;
};

const FILTERS: [WallFilter, string][] = [
  ["all", "All"],
  ["needsYou", "Needs you"],
  ["working", "Working"],
  ["idle", "Idle"],
];

/**
 * The wall's one row of controls: which agents it shows, by status; then,
 * in Focus, who it shows large and whether that follows whoever needs
 * you; in Grid, how many a page holds; then Focus | Grid, and a new agent.
 */
function WallToolbar({
  tab,
  agents,
  filter,
  onMode,
  menu,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  filter: WallFilter;
  onMode: (mode: WallMode) => void;
  menu: React.ReactNode;
}) {
  const pinnedPane = useUiStore((s) => s.pinned[tab.id]);
  const perTab = useConfigStore((s) => s.config?.agentsPerTab ?? 4);
  const pinned = agents.find((a) => a.paneId === pinnedPane);
  // Who Focus shows: the pinned agent while it is here, else the most
  // urgent.
  const focus = pinned ?? agents.find((a) => a.paneId === tab.wallUrgency[0]);
  const count = (f: WallFilter) =>
    f === "all" ? agents.length : agents.filter((a) => a.status === f).length;
  return (
    <div className="wall-toolbar">
      {FILTERS.map(([f, label]) => (
        <button
          key={f}
          className={`wall-filter ${f}${filter === f ? " selected" : ""}`}
          onClick={() => useUiStore.getState().setWallFilter(tab.id, f)}
        >
          {label} {count(f)}
        </button>
      ))}
      <span className="wall-toolbar-spacer" />
      {tab.wallMode === "focus" ? (
        <>
          <span className="wall-toolbar-note">Focus follows needs you</span>
          {focus && (
            <button
              className={`wall-pill${pinned ? " selected" : ""}`}
              title={pinned ? "Let Focus follow whoever needs you" : "Keep this agent large"}
              onClick={() =>
                useUiStore.getState().pinAgent(tab.id, pinned ? null : focus.paneId)
              }
            >
              {pinned ? "Unpin" : `Pin ${agentLabel(focus)}`}
            </button>
          )}
        </>
      ) : (
        <>
          <span className="wall-toolbar-label">Agents per tab</span>
          <span className="wall-segments">
            {[4, 6, 9].map((n) => (
              <button
                key={n}
                className={perTab === n ? "selected" : undefined}
                onClick={() => perTab !== n && void setAgentsPerTab(n)}
              >
                {n}
              </button>
            ))}
          </span>
        </>
      )}
      <span className="wall-segments">
        {(["focus", "grid"] as WallMode[]).map((mode) => (
          <button
            key={mode}
            className={tab.wallMode === mode ? "selected" : undefined}
            // A click must not take the keyboard from the terminal first:
            // switching asks which agent holds it, to keep it there.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onMode(mode)}
          >
            {mode === "focus" ? "Focus" : "Grid"}
          </button>
        ))}
      </span>
      {menu}
    </div>
  );
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

  // The tab's agents in team order, which never moves with status.
  const agents = useMemo(() => {
    const byPane = new Map(
      (snapshot?.agents ?? [])
        .filter((a) => a.tabId === tabId)
        .map((a) => [a.paneId, a]),
    );
    return (shown?.wallPages ?? [])
      .flat()
      .flatMap((pane) => byPane.get(pane) ?? []);
  }, [snapshot, shown, tabId]);
  const filter = useUiStore((s) => s.wallFilter[tabId] ?? "all");

  // Its tiles, or the tab's task board in their place. On the Tasks side
  // no tile is mounted, so each lent terminal has gone back to its own
  // pane under the wall, and the Agents side borrows them again.
  const side: WallView = shown?.wallView ?? "agents";
  const tilesShown = visible && side === "agents";

  // The tab's focused pane is under the wall, so the keyboard would go
  // nowhere: each layout hands it to one of its tiles as it comes on
  // screen in the active tab. Switching layouts keeps it on the agent that
  // had it: Grid pins that agent for Focus to show large, and Focus tells
  // Grid which page and tile to give it to.
  const wallRef = useRef<HTMLDivElement>(null);
  const keyboard = tilesShown && current;
  const mode = shown?.wallMode ?? "focus";
  const handover = useRef<string | null>(null);
  const switchMode = (next: WallMode) => {
    if (!shown || next === mode) return;
    const held = agents.find((a) => {
      const el = getTerminal(a.paneId)?.element;
      return el && wallRef.current?.contains(el) && el.contains(document.activeElement);
    });
    if (held && next === "focus") useUiStore.getState().pinAgent(tabId, held.paneId);
    if (held && next === "grid") {
      handover.current = held.paneId;
      const page = shown.wallPages.findIndex((p) => p.includes(held.paneId));
      if (page >= 0) useUiStore.getState().setGridPage(tabId, page);
    }
    void setWallMode(shown.focusedPane, next);
  };

  const unread = snapshot?.unreadPanes ?? [];

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
      {/* The Tasks side has no toolbar here: the task board has its own,
          and the sidebar's view switch leads back. */}
      {side === "agents" && shown && (
        <WallToolbar
          tab={shown}
          agents={agents}
          filter={filter}
          onMode={switchMode}
          menu={
            <div className="wall-new">
              <button
                className={`wall-new-button${menuOpen ? " open" : ""}`}
                title="Start a Claude Code agent"
                onClick={() => setMenuOpen((o) => !o)}
              >
                + Agent ▾
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
          }
        />
      )}

      {side === "tasks" ? (
        <TaskBoard tabId={tabId} visible={visible} />
      ) : agents.length === 0 ? (
        <div className="wall-empty">
          <p>No agents in {shown?.title ?? "this tab"}.</p>
          <p className="wall-empty-hint">
            Start one with <em>+ Agent</em>, or run <code>claude</code>,{" "}
            <code>codex</code>, <code>opencode</code> or another agent in a pane
            of the tab: <em>Splits</em> shows them.
          </p>
        </div>
      ) : mode === "focus" && shown ? (
        <FocusLayout
          tab={shown}
          agents={agents}
          filter={filter}
          unread={unread}
          visible={visible}
          current={current}
          keyboard={keyboard}
          now={now}
        />
      ) : (
        shown && (
          <GridLayout
            tab={shown}
            agents={agents}
            filter={filter}
            unread={unread}
            visible={visible}
            current={current}
            keyboard={keyboard}
            handover={handover}
            now={now}
          />
        )
      )}
    </div>
  );
});
