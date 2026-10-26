import { useEffect, useRef, useState } from "react";
import {
  AgentInfo,
  agentLabel,
  focusPane,
  markPaneRead,
  setWallMode,
  TabSnapshot,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { useUiStore, WallFilter } from "../state/uiStore";
import { getTerminal } from "../terminal/registry";
import { modelName, useAgentActivity } from "./ActivityPanel";
import { AgentStatusBar } from "./AgentStatusBar";
import { STATUS_LABEL, StatusMark, StatusPip } from "./StatusMark";
import { SubagentSheet } from "./SubagentSheet";
import { elapsed } from "./time";
import { useLentTerminal } from "./useLentTerminal";

/** Columns and rows of a page, fixed by its size so tiles keep theirs as
 *  agents come and go. */
const SHAPE: Record<number, string> = { 4: "per-4", 6: "per-6", 9: "per-9" };

/**
 * Grid (20a–20c): the tab's agents in pages of `agentsPerTab`, in team
 * order (`wallPages`, from Rust), every terminal live and full size.
 * A strip of page tabs appears once there is more than one page.
 */
export function GridLayout({
  tab,
  agents,
  filter,
  unread,
  visible,
  current,
  keyboard,
  handover,
  now,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  filter: WallFilter;
  unread: string[];
  visible: boolean;
  current: boolean;
  /** The wall is on screen in the active tab: the keyboard is its. */
  keyboard: boolean;
  /** The agent that held the keyboard in Focus, to keep it here. */
  handover: React.MutableRefObject<string | null>;
  now: number;
}) {
  const perTab = useConfigStore((s) => s.config?.agentsPerTab ?? 4);
  const byPane = new Map(agents.map((a) => [a.paneId, a]));
  const pages = tab.wallPages.map((page) => page.flatMap((p) => byPane.get(p) ?? []));
  const stored = useUiStore((s) => s.gridPage[tab.id] ?? 0);
  const page = Math.max(0, Math.min(stored, pages.length - 1));
  // Fewer pages than before — agentsPerTab went up, agents left: the page
  // shown is the last there is, and stays so when more come back.
  useEffect(() => {
    if (page !== stored) useUiStore.getState().setGridPage(tab.id, page);
  }, [page, stored, tab.id]);

  // The filter hides tiles within the page rather than cutting the pages
  // again: a page keeps its agents, and the strip its tabs, whatever the
  // chips say. An agent with no known status is only under All.
  const onPage = pages[page] ?? [];
  const tiles = filter === "all" ? onPage : onPage.filter((a) => a.status === filter);
  const free = page === pages.length - 1 ? perTab - onPage.length : 0;
  // Where the free space is drawn: one cell cannot follow an L, so it takes
  // the empty rows below the tiles if there are any, else what is left of
  // the last row.
  const cols = perTab === 4 ? 2 : 3;
  const partial = tiles.length % cols ? cols - (tiles.length % cols) : 0;
  const emptyRows = Math.floor((perTab - tiles.length - partial) / cols);
  const space = emptyRows > 0 ? `rows-${emptyRows}` : `cols-${partial}`;

  // The keyboard goes to a tile as the grid comes on screen: the agent
  // Focus had it on, else the tab's focused pane's, else the first shown.
  // And again when the filter or the page hides the tile that had it —
  // not on every snapshot, or a click in another tile would be undone.
  const gridRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!keyboard) return;
    const wanted = [handover.current, tab.focusedPane];
    handover.current = null;
    const tile = tiles.find((a) => wanted.includes(a.paneId)) ?? tiles[0];
    const term = tile && getTerminal(tile.paneId);
    if (term?.element && gridRef.current?.contains(term.element)) term.focus();
  }, [keyboard, filter, page]);

  return (
    <div className="grid-layout" ref={gridRef}>
      {agents.length > perTab && (
        <div className="grid-strip">
          {pages.map((agentsOn, i) => {
            const asking = agentsOn.filter((a) => a.status === "needsYou").length;
            const classes = ["grid-page", i === page ? "active" : asking > 0 && "asking"];
            return (
              <button
                key={i}
                className={classes.filter(Boolean).join(" ")}
                onClick={() => useUiStore.getState().setGridPage(tab.id, i)}
              >
                <span>{i + 1}</span>
                {agentsOn.map((a) => (
                  <StatusPip key={a.paneId} status={a.status} />
                ))}
                {i !== page && asking > 0 && <span className="grid-page-asking">{asking}</span>}
              </button>
            );
          })}
        </div>
      )}
      <div className={`grid-page-tiles ${SHAPE[perTab] ?? "per-4"}`}>
        {tiles.map((a) => (
          <GridTile
            key={a.paneId}
            tab={tab}
            agent={a}
            unread={unread.includes(a.paneId)}
            visible={visible}
            current={current}
            now={now}
          />
        ))}
        {tiles.length === 0 && (
          <div className="grid-space rows-all">No agents on this page match this filter.</div>
        )}
        {tiles.length > 0 && free > 0 && (
          <div className={`grid-space ${space}`}>
            Space for {free} more. New agents in this team land here.
          </div>
        )}
      </div>
    </div>
  );
}

/** One agent of the grid: what it is doing, its live terminal at full
 *  size, and a compact status bar. */
function GridTile({
  tab,
  agent,
  unread,
  visible,
  current,
  now,
}: {
  tab: TabSnapshot;
  agent: AgentInfo;
  unread: boolean;
  visible: boolean;
  current: boolean;
  now: number;
}) {
  const pane = agent.paneId;
  const { hostRef, shown } = useLentTerminal(pane, visible, { fit: true });
  const [sheetOpen, setSheetOpen] = useState(false);
  const { activity } = useAgentActivity(pane, visible && agent.hasActivity);
  const status = agent.status;
  const model = modelName(activity?.session.model ?? null) ?? modelName(agent.model);
  const task = agent.topic ?? activity?.session.tasks?.current ?? null;
  const focus = () => {
    useUiStore.getState().pinAgent(tab.id, pane);
    void setWallMode(pane, "focus");
  };
  return (
    <div className={`grid-tile${status ? ` ${status}` : ""}${unread ? " unread" : ""}`}>
      <div className="grid-tile-head">
        <div className="grid-tile-line">
          <StatusMark status={status} small />
          <span className="grid-tile-name">{agentLabel(agent)}</span>
          {status && (
            <span className={`grid-tile-status ${status}`}>
              {STATUS_LABEL[status]}
              {agent.sinceMs ? ` · ${elapsed(agent.sinceMs, now)}` : ""}
            </span>
          )}
          <button className="grid-tile-focus" title="Show this agent large, in Focus" onClick={focus}>
            {model ? `${model} · ` : ""}Focus ›
          </button>
        </div>
        {status === "needsYou" ? (
          agent.message && <div className="grid-tile-ask">{agent.message}</div>
        ) : status === "idle" ? (
          agent.topic && <div className="grid-tile-task">Last: {agent.topic}</div>
        ) : (
          task && <div className="grid-tile-task">Task: {task}</div>
        )}
      </div>
      <div className="grid-tile-body">
        <div
          className="grid-tile-term"
          data-pane={pane}
          onMouseDown={() => {
            if (unread) void markPaneRead(pane);
            // On the board, a wall in another tab's frame: its tab becomes
            // the active one, so the keyboard stays in the tile pressed.
            if (!current) void focusPane(pane);
          }}
        >
          {/* The terminal's element is appended here by `lendTerminal`, so
              React must own nothing inside it. */}
          <div className="grid-tile-host" ref={hostRef} />
          {!shown && <span className="wall-term-missing">shown elsewhere</span>}
        </div>
        {sheetOpen && (
          <SubagentSheet activity={activity} now={now} onClose={() => setSheetOpen(false)} />
        )}
      </div>
      <AgentStatusBar
        agent={agent}
        activity={activity}
        now={now}
        sheetOpen={sheetOpen}
        onToggleSheet={() => setSheetOpen(!sheetOpen)}
        compact
      />
    </div>
  );
}
