import { useEffect, useRef, useState } from "react";
import {
  AgentInfo,
  agentLabel,
  focusPane,
  markPaneRead,
  setWallMode,
  TabSnapshot,
  WallTool,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { useUiStore, WallFilter } from "../state/uiStore";
import { getTerminal } from "../terminal/registry";
import { modelName, useAgentActivity } from "./ActivityPanel";
import { AgentStatusBar } from "./AgentStatusBar";
import { STATUS_LABEL, StatusMark, StatusPip } from "./StatusMark";
import { SubagentSheet } from "./SubagentSheet";
import { elapsed } from "./time";
import { AskingLine } from "./AskingLine";
import { useLentTerminal } from "./useLentTerminal";
import { SendNextTask } from "./SendNextTask";
import { ToolMark } from "./ToolMark";
import { ToolView } from "./ToolView";

/** Columns and rows of a page, fixed by its size so tiles keep theirs as
 *  agents come and go. */
const SHAPE: Record<number, string> = { 4: "per-4", 6: "per-6", 9: "per-9" };

/**
 * Grid (20a–20c): the tab's agents in pages of `agentsPerTab`, in team
 * order, then its tools (`wallPages`, from Rust), every terminal live and
 * full size.
 * A strip of page tabs appears once there is more than one page.
 */
export function GridLayout({
  tab,
  agents,
  tools,
  filter,
  unread,
  visible,
  current,
  keyboard,
  handover,
  sendable,
  now,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  tools: WallTool[];
  filter: WallFilter;
  unread: string[];
  visible: boolean;
  current: boolean;
  /** The wall is on screen in the active tab: the keyboard is its. */
  keyboard: boolean;
  /** The agent that held the keyboard in Focus, to keep it here. */
  handover: React.MutableRefObject<string | null>;
  /** The agents with a next task to be sent. */
  sendable: Set<string>;
  now: number;
}) {
  const perTab = useConfigStore((s) => s.config?.agentsPerTab ?? 4);
  const byPane = new Map<string, AgentInfo | WallTool>([
    ...agents.map((a) => [a.paneId, a] as const),
    ...tools.map((t) => [t.paneId, t] as const),
  ]);
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
  const tiles =
    filter === "all"
      ? onPage
      : onPage.filter((t) => ("kind" in t ? filter === "tools" : t.status === filter));
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
      {pages.length > 1 && (
        <div className="grid-strip">
          {pages.map((onIt, i) => {
            // A pip per agent; a tool has no status to show.
            const agentsOn = onIt.filter((t): t is AgentInfo => !("kind" in t));
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
        {tiles.map((t) =>
          "kind" in t ? (
            <ToolTile
              key={t.paneId}
              tab={tab}
              tool={t}
              unread={unread.includes(t.paneId)}
              visible={visible}
              current={current}
            />
          ) : (
            <GridTile
              key={t.paneId}
              tab={tab}
              agent={t}
              unread={unread.includes(t.paneId)}
              visible={visible}
              current={current}
              canSend={sendable.has(t.paneId)}
              now={now}
            />
          ),
        )}
        {tiles.length === 0 && (
          <div className="grid-space rows-all">Nothing on this page matches this filter.</div>
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
  canSend,
  now,
}: {
  tab: TabSnapshot;
  agent: AgentInfo;
  unread: boolean;
  visible: boolean;
  current: boolean;
  /** It has a next task to be sent. */
  canSend: boolean;
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
          <AskingLine agent={agent} className="grid-tile-ask" />
        ) : status === "idle" ? (
          (agent.topic || canSend) && (
            <div className="grid-tile-task">
              {agent.topic ? `Last: ${agent.topic} ` : ""}
              {canSend && <SendNextTask pane={agent.paneId} label="Send next task…" />}
            </div>
          )
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

/** One tool of the grid: what it is, and its live terminal at full size —
 *  or, for a diff or a graph, the pane itself. No status: a tool is not
 *  waiting on anyone. */
function ToolTile({
  tab,
  tool,
  unread,
  visible,
  current,
}: {
  tab: TabSnapshot;
  tool: WallTool;
  unread: boolean;
  visible: boolean;
  current: boolean;
}) {
  const pane = tool.paneId;
  const terminal = tool.kind === "terminal";
  const { hostRef, shown } = useLentTerminal(pane, visible && terminal, { fit: true });
  const focus = () => {
    useUiStore.getState().pinAgent(tab.id, pane);
    void setWallMode(pane, "focus");
  };
  return (
    <div className={`grid-tile tool${unread ? " unread" : ""}`}>
      <div className="grid-tile-head">
        <div className="grid-tile-line">
          <span className="tool-mark">
            <ToolMark kind={tool.kind} />
          </span>
          <span className="grid-tile-name">{tool.title}</span>
          <button className="grid-tile-focus" title="Show this tool large, in Focus" onClick={focus}>
            Focus ›
          </button>
        </div>
      </div>
      <div className="grid-tile-body">
        <div
          className="grid-tile-term"
          data-pane={pane}
          onMouseDown={() => {
            if (unread) void markPaneRead(pane);
            if (!current) void focusPane(pane);
          }}
        >
          {terminal ? (
            <>
              {/* The terminal's element is appended here by `lendTerminal`,
                  so React must own nothing inside it. */}
              <div className="grid-tile-host" ref={hostRef} />
              {!shown && <span className="wall-term-missing">shown elsewhere</span>}
            </>
          ) : (
            <ToolView tool={tool} focused={current && tab.focusedPane === pane} visible={visible} />
          )}
        </div>
      </div>
    </div>
  );
}
