import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ActionDetail,
  AgentInfo,
  AgentTrail,
  HARNESS_LABEL,
  Node,
  TaskCard,
  TaskColumn,
  TaskHistoryEntry,
  TrailAction,
  TrailTurn,
  agentLabel,
  focusPane,
  getAgentTrail,
  getTrailDetail,
  setTaskLayout,
  setWallView,
  TabSnapshot,
  TaskLayout,
  taskAdd,
  taskCards,
  taskHistory,
  taskSend,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { percent, useAgentActivity } from "./ActivityPanel";
import { AskingLine } from "./AskingLine";
import { showOnWall } from "./showOnWall";
import { STATUS_LABEL, StatusMark } from "./StatusMark";
import { elapsed } from "./time";

type Props = {
  /** The tab whose wall it is the Tasks side of, and whose cards it shows. */
  tabId: string;
  /** On screen: only then does it read the log. */
  visible: boolean;
};

/** The columns, left to right: what needs you first. */
const COLUMNS: [TaskColumn, string][] = [
  ["needsYou", "Needs you"],
  ["todo", "To do"],
  ["doing", "Doing"],
  ["done", "Done"],
];

/** How often the Trail view reads its agent's trail again: it has no
 *  event, so it is read on a timer, as the activity panel is. */
const POLL_MS = 2000;

/** How many of an agent's latest turns the Trail view reads, and how
 *  many more each "Show older" adds. */
const TRAIL_TURNS = 20;

/** A turn begins this long after its card was given, at the most before:
 *  the card is logged as the message is typed, the prompt entry after. */
const GIVEN_SLACK_MS = 1000;

const MARKS: Record<TrailAction["status"], string> = {
  ok: "✓",
  error: "✗",
  running: "…",
  interrupted: "-",
};

const NO_CARDS: TaskCard[] = [];
const NO_AGENTS: AgentInfo[] = [];
const NO_HISTORY: TaskHistoryEntry[] = [];

function paneIds(node: Node): string[] {
  return node.type === "leaf" ? [node.paneId] : node.children.flatMap(paneIds);
}

/** "42s", "3m", "1h", "2d" — how long ago. */
function age(ms: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

/** Local midnight before `now`: where "today" starts for the Done column. */
function startOfDay(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** What the board reads: every tab's ("*"), one tab's, or nothing (""). */
const scopeOf = (key: string) => (key === "*" ? null : key);

type View = "cards" | "history" | "trail";

/** Closes a popover on a click outside it and the button that opens it
 *  (its parent), and on Esc while the keyboard is inside it — never an Esc
 *  typed into a terminal. */
function usePopover(onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (!ref.current?.parentElement?.contains(e.target as globalThis.Node)) onClose();
    };
    document.addEventListener("mousedown", down);
    return () => document.removeEventListener("mousedown", down);
  }, [onClose]);
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    }
  };
  return { ref, onKeyDown };
}

/** A menu takes the keyboard as it opens — its first item — so Esc right
 *  away reaches it, and gives it back to the button that opened it when it
 *  closes, unless something else took it meanwhile. */
function useMenuFocus(ref: React.RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const menu = ref.current;
    const trigger = menu?.parentElement?.querySelector<HTMLElement>(":scope > button");
    // With nothing to pick, the menu itself (tabIndex -1), so Esc still
    // reaches it.
    (menu?.querySelector<HTMLElement>("button:not(:disabled)") ?? menu)?.focus();
    return () => {
      requestAnimationFrame(() => {
        const now = document.activeElement;
        if (!now || now === document.body) trigger?.focus();
      });
    };
  }, [ref]);
}

/**
 * The task log's cards (`cmux_core::tasks`), in four columns — the tasks
 * given to a tab's agents, its team's in a team's tab, or every tab's —
 * or the log itself, newest first. Read again whenever it is appended to.
 */
export const TaskBoard = memo(function TaskBoard({ tabId, visible }: Props) {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const shown = snapshot?.tabs.find((t) => t.id === tabId);
  const [all, setAll] = useState(false);
  const [view, setView] = useState<View>("cards");
  // The agent the Trail view shows, and the time of the card it was
  // opened from, to scroll to that card's turn.
  const [trailOf, setTrailOf] = useState<{ pane: string; label: string; at: number | null } | null>(null);
  const live = visible;
  const now = useNow(live);

  const scope = all ? "*" : tabId;
  const cards = useLogRead(
    live,
    // The Trail view names each agent's card in Doing too.
    view === "cards" || view === "trail" ? scope : "",
    (k) => taskCards(scopeOf(k)),
    NO_CARDS,
  );
  const history = useLogRead(
    live,
    view === "history" ? scope : "",
    (k) => taskHistory(scopeOf(k), null),
    NO_HISTORY,
  );

  // Where each pane is, for a card's agent and, across tabs, its place.
  const { panes, placeOf } = useMemo(() => {
    const panes = new Set<string>();
    const placeOf = new Map<string, string>();
    for (const t of snapshot?.tabs ?? []) {
      for (const p of paneIds(t.root)) panes.add(p);
      placeOf.set(t.id, t.team?.name ?? t.title);
    }
    return { panes, placeOf };
  }, [snapshot]);

  const goTo = (pane: string) => {
    void focusPane(pane);
    // An agent of this tab is under the wall: its tile shows it.
    if (shown && paneIds(shown.root).includes(pane)) {
      void setWallView(pane, "agents");
    }
  };

  const agents = useMemo(
    () => (snapshot?.agents ?? []).filter((a) => all || a.tabId === tabId),
    [snapshot, all, tabId],
  );
  // The agents a card can go to: this tab's, idle first, then working.
  const tabAgents = useMemo(
    () => (snapshot?.agents ?? []).filter((a) => a.tabId === tabId),
    [snapshot, tabId],
  );
  const showTrail = (card: TaskCard) => {
    setTrailOf({ pane: card.pane!, label: card.assignee ?? "its agent", at: card.createdMs });
    setView("trail");
  };
  const layout: TaskLayout = shown?.taskLayout ?? "columns";
  // One card, in a column or, compact, in a swimlane's cell.
  const card = (c: TaskCard, compact: boolean) => (
    <Card
      key={c.id}
      card={c}
      agent={snapshot?.agents.find((a) => a.paneId === c.pane) ?? null}
      tabAgents={(snapshot?.agents ?? NO_AGENTS).filter((a) => a.tabId === (c.tab ?? tabId))}
      place={all && !compact ? ((c.tab && placeOf.get(c.tab)) ?? "closed tab") : null}
      reachable={!!c.pane && panes.has(c.pane)}
      live={live}
      now={now}
      compact={compact}
      onGoTo={goTo}
      onTrail={showTrail}
    />
  );

  return (
    <div className="task-board">
      <BoardToolbar
        view={view}
        onView={setView}
        all={all}
        onAll={setAll}
        team={!!shown?.team}
        tabId={tabId}
        agents={tabAgents}
        layout={layout}
        onLayout={(l) => shown && void setTaskLayout(shown.focusedPane, l)}
      />

      {view === "trail" ? (
        <TrailView
          agents={agents}
          running={snapshot?.agents ?? NO_AGENTS}
          cards={cards}
          picked={trailOf}
          onPick={(a) => setTrailOf({ pane: a.paneId, label: agentLabel(a), at: null })}
          live={live}
          now={now}
        />
      ) : view === "history" ? (
        history.length === 0 ? (
          <Empty />
        ) : (
          <div className="tb-history">
            <HistoryLines entries={[...history].reverse()} now={now} />
          </div>
        )
      ) : cards.length === 0 && layout === "columns" ? (
        <Empty />
      ) : layout === "swimlanes" ? (
        <Swimlanes
          tabs={all ? (snapshot?.tabs ?? []) : shown ? [shown] : []}
          agents={snapshot?.agents ?? NO_AGENTS}
          cards={cards}
          grouped={all}
          now={now}
          card={card}
        />
      ) : (
        <div className="tb-columns">
          {COLUMNS.map(([column, label]) => (
            <Column
              key={column}
              column={column}
              label={label}
              cards={cards.filter((c) => c.column === column).sort((a, b) => b.updatedMs - a.updatedMs)}
              now={now}
            >
              {(c) => card(c, false)}
            </Column>
          ))}
        </div>
      )}
    </div>
  );
});

/**
 * The board's one row: Cards · History · Trail on the left; the scope and
 * + Card on the right.
 */
function BoardToolbar({
  view,
  onView,
  all,
  onAll,
  team,
  tabId,
  agents,
  layout,
  onLayout,
}: {
  view: View;
  onView: (view: View) => void;
  all: boolean;
  onAll: (all: boolean) => void;
  /** The tab runs a team: its scope is "This team". */
  team: boolean;
  tabId: string;
  agents: AgentInfo[];
  layout: TaskLayout;
  onLayout: (layout: TaskLayout) => void;
}) {
  const [scopeOpen, setScopeOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const closeScope = useCallback(() => setScopeOpen(false), []);
  const closeAdd = useCallback(() => setAddOpen(false), []);
  const mine = team ? "This team" : "This tab";
  const tabs: [View, string, string][] = [
    ["cards", "Cards", "The tasks given, by where each stands"],
    ["history", "History", "Every task given and every turn ended, newest first"],
    ["trail", "Trail", "What each agent did, turn by turn, from its own transcript"],
  ];
  return (
    <div className="tb-toolbar">
      {tabs.map(([id, label, title]) => (
        <button
          key={id}
          className={`tb-tab${view === id ? " selected" : ""}`}
          title={title}
          onClick={() => onView(id)}
        >
          {label}
        </button>
      ))}
      <span className="tb-toolbar-spacer" />
      {view === "cards" && (
        <span className="wall-segments">
          {(["columns", "swimlanes"] as TaskLayout[]).map((l) => (
            <button
              key={l}
              className={layout === l ? "selected" : undefined}
              onClick={() => layout !== l && onLayout(l)}
            >
              {l === "columns" ? "Columns" : "Swimlanes"}
            </button>
          ))}
        </span>
      )}
      <div className="tb-popover-anchor">
        <button className="tb-scope" onClick={() => setScopeOpen((o) => !o)}>
          Scope: {all ? "All agents" : mine} ▾
        </button>
        {scopeOpen && (
          <ScopeMenu
            options={[
              [false, mine],
              [true, "All agents"],
            ]}
            value={all}
            onPick={(v) => {
              onAll(v);
              closeScope();
            }}
            onClose={closeScope}
          />
        )}
      </div>
      <div className="tb-popover-anchor">
        <button className={`tb-add${addOpen ? " open" : ""}`} onClick={() => setAddOpen((o) => !o)}>
          + Card
        </button>
        {addOpen && <AddCard tabId={tabId} agents={agents} onClose={closeAdd} />}
      </div>
    </div>
  );
}

function ScopeMenu({
  options,
  value,
  onPick,
  onClose,
}: {
  options: [boolean, string][];
  value: boolean;
  onPick: (value: boolean) => void;
  onClose: () => void;
}) {
  const { ref, onKeyDown } = usePopover(onClose);
  useMenuFocus(ref);
  return (
    <div className="tb-menu" ref={ref} tabIndex={-1} onKeyDown={onKeyDown}>
      {options.map(([v, label]) => (
        <button key={label} className={v === value ? "selected" : undefined} onClick={() => onPick(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

/**
 * + Card: a card written in this tab, for one of its agents or no one —
 * and, for an agent with nothing on, sent to it at once.
 */
function AddCard({ tabId, agents, onClose }: { tabId: string; agents: AgentInfo[]; onClose: () => void }) {
  const { ref, onKeyDown } = usePopover(onClose);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [assignee, setAssignee] = useState("");
  const [error, setError] = useState<string | null>(null);
  const to = agents.find((a) => a.paneId === assignee);
  const add = async (send: boolean) => {
    try {
      const id = await taskAdd(tabId, title.trim(), description.trim() || null, assignee || null);
      if (send && to) await taskSend(id, to.paneId);
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <div className="tb-popover" ref={ref} onKeyDown={onKeyDown}>
      <label className="tb-popover-label" htmlFor="tb-card-title">
        Title
      </label>
      <input
        id="tb-card-title"
        className="tb-popover-input"
        value={title}
        autoFocus
        onChange={(e) => setTitle(e.target.value)}
      />
      <label className="tb-popover-label" htmlFor="tb-card-description">
        Description (optional)
      </label>
      <textarea
        id="tb-card-description"
        className="tb-popover-input"
        rows={3}
        value={description}
        onChange={(e) => setDescription(e.target.value)}
      />
      <label className="tb-popover-label" htmlFor="tb-card-assignee">
        Assign to
      </label>
      <select
        id="tb-card-assignee"
        className="tb-popover-input"
        value={assignee}
        onChange={(e) => setAssignee(e.target.value)}
      >
        <option value="">No one yet</option>
        {agents.map((a) => (
          <option key={a.paneId} value={a.paneId}>
            {agentLabel(a)}
          </option>
        ))}
      </select>
      {error && <div className="tb-popover-error">{error}</div>}
      <div className="tb-popover-foot">
        <button className="tb-popover-primary" disabled={!title.trim()} onClick={() => void add(false)}>
          Add
        </button>
        {to?.status === "idle" && (
          <button className="tb-popover-secondary" disabled={!title.trim()} onClick={() => void add(true)}>
            Add and send
          </button>
        )}
      </div>
    </div>
  );
}

/** One column: its head, and its cards scrolling on their own. Done shows
 *  only today's until asked for all of them. */
function Column({
  column,
  label,
  cards,
  now,
  children,
}: {
  column: TaskColumn;
  label: string;
  cards: TaskCard[];
  now: number;
  children: (card: TaskCard) => React.ReactNode;
}) {
  const [everything, setEverything] = useState(false);
  const shown = column === "done" && !everything ? doneToday(cards, now) : cards;
  return (
    <section className={`tb-column ${column}`}>
      <ColumnHead
        column={column}
        label={label}
        count={cards.length}
        everything={everything}
        onEverything={setEverything}
      />
      <div className="tb-cards">{shown.map(children)}</div>
    </section>
  );
}

/** The cards of `cards` done today. */
function doneToday(cards: TaskCard[], now: number): TaskCard[] {
  const today = startOfDay(now);
  return cards.filter((c) => c.updatedMs >= today);
}

/** A column's head: its mark, name and count; Done's switch between
 *  today's cards and all of them. */
function ColumnHead({
  column,
  label,
  count,
  everything,
  onEverything,
}: {
  column: TaskColumn;
  label: string;
  count: number;
  everything: boolean;
  onEverything: (everything: boolean) => void;
}) {
  const mark =
    column === "needsYou" ? (
      <StatusMark status="needsYou" small />
    ) : column === "todo" ? (
      <StatusMark status={null} small />
    ) : column === "doing" ? (
      <StatusMark status="working" small />
    ) : (
      <span className="tb-done-mark">✓</span>
    );
  return (
    <h3 className={`tb-column-head ${column}`}>
      {mark}
      {label}
      <span className="tb-count">{count}</span>
      {column === "done" && (
        <button className="tb-done-toggle" onClick={() => onEverything(!everything)}>
          {everything ? "all ▴" : "today ▾"}
        </button>
      )}
    </h3>
  );
}

/** A lane: an agent of a tab, one whose cards outlived it, or no one. */
type Lane = { key: string; pane: string | null; agent: AgentInfo | null; name: string };

/**
 * Swimlanes (9d): a row per agent, in its tab's urgency order (whoever
 * needs you first), then one for the cards given to no one, each row
 * across the same four columns. With every tab's cards, the lanes group
 * under their tabs, and a lane with nothing open folds to one line.
 */
function Swimlanes({
  tabs,
  agents,
  cards,
  grouped,
  now,
  card,
}: {
  tabs: TabSnapshot[];
  agents: AgentInfo[];
  cards: TaskCard[];
  grouped: boolean;
  now: number;
  card: (c: TaskCard, compact: boolean) => React.ReactNode;
}) {
  const [everything, setEverything] = useState(false);
  const counts = (column: TaskColumn) => cards.filter((c) => c.column === column).length;
  // With every tab's cards, those of tabs since closed, in one lane at the
  // end: the columns call each of them "closed tab".
  const closed = grouped ? cards.filter((c) => !tabs.some((t) => t.id === c.tab)) : [];
  return (
    <div className="tb-lanes">
      <div className="tb-lane-grid tb-lane-heads">
        <span />
        {COLUMNS.map(([column, label]) => (
          <ColumnHead
            key={column}
            column={column}
            label={label}
            count={counts(column)}
            everything={everything}
            onEverything={setEverything}
          />
        ))}
      </div>
      {tabs.map((tab) => {
        const inTab = cards.filter((c) => c.tab === tab.id);
        const lanes = lanesOf(tab, agents, inTab);
        return (
          <section key={tab.id} className="tb-lane-group">
            {grouped && <h4 className="tb-lane-tab">{tab.team?.name ?? tab.title}</h4>}
            {lanes.map((lane) => (
              <LaneRow
                key={lane.key}
                lane={lane}
                cards={inTab.filter((c) => c.pane === lane.pane)}
                unassigned={inTab.filter((c) => !c.pane && c.column === "todo")}
                collapsible={grouped}
                everything={everything}
                now={now}
                card={card}
              />
            ))}
          </section>
        );
      })}
      {closed.length > 0 && (
        <section className="tb-lane-group">
          <h4 className="tb-lane-tab">Closed tabs</h4>
          <LaneRow
            lane={{ key: "closed", pane: null, agent: null, name: "closed tab" }}
            cards={closed}
            unassigned={[]}
            collapsible={grouped}
            everything={everything}
            now={now}
            card={card}
          />
        </section>
      )}
    </div>
  );
}

/** The lanes of `tab`: its agents by urgency, then any agent its cards name
 *  that is no longer running, then no one. */
function lanesOf(tab: TabSnapshot, agents: AgentInfo[], cards: TaskCard[]): Lane[] {
  const running = tab.wallUrgency.flatMap((p) => agents.filter((a) => a.paneId === p));
  const lanes: Lane[] = running.map((a) => ({ key: a.paneId, pane: a.paneId, agent: a, name: agentLabel(a) }));
  for (const c of cards) {
    if (c.pane && !lanes.some((l) => l.pane === c.pane)) {
      lanes.push({ key: c.pane, pane: c.pane, agent: null, name: c.assignee ?? "its agent" });
    }
  }
  lanes.push({ key: `none:${tab.id}`, pane: null, agent: null, name: "Unassigned" });
  return lanes;
}

function LaneRow({
  lane,
  cards,
  unassigned,
  collapsible,
  everything,
  now,
  card,
}: {
  lane: Lane;
  cards: TaskCard[];
  /** The tab's cards in To do given to no one: what an idle agent can take. */
  unassigned: TaskCard[];
  collapsible: boolean;
  everything: boolean;
  now: number;
  card: (c: TaskCard, compact: boolean) => React.ReactNode;
}) {
  const open = cards.some((c) => c.column !== "done");
  const a = lane.agent;
  const head = (
    <div className={`tb-lane-agent${a ? "" : " none"}`}>
      <div className="tb-lane-name">
        {a ? <StatusMark status={a.status} small /> : null}
        <span>{lane.name}</span>
      </div>
      {a && (
        <>
          <div className="tb-lane-meta">
            {[HARNESS_LABEL[a.harness], a.model].filter(Boolean).join(" · ")}
          </div>
          {a.status && (
            <div className={`tb-lane-meta ${a.status}`}>
              {STATUS_LABEL[a.status]}
              {a.sinceMs ? ` · ${elapsed(a.sinceMs, now)}` : ""}
            </div>
          )}
        </>
      )}
      {!a && lane.pane && <div className="tb-lane-meta">not running</div>}
    </div>
  );
  if (collapsible && !open) {
    return <div className="tb-lane-grid tb-lane collapsed">{head}</div>;
  }
  return (
    <div className="tb-lane-grid tb-lane">
      {head}
      {COLUMNS.map(([column]) => {
        const here = cards.filter((c) => c.column === column).sort((x, y) => y.updatedMs - x.updatedMs);
        const shown = column === "done" && !everything ? doneToday(here, now) : here;
        const idleTarget = column === "todo" && here.length === 0 && a?.status === "idle" && unassigned.length > 0;
        return (
          <div key={column} className={`tb-lane-cell ${column}`}>
            {shown.map((c) => card(c, true))}
            {idleTarget && <IdleTarget agent={a} cards={unassigned} />}
          </div>
        );
      })}
    </div>
  );
}

/** An idle agent's empty To do: the tab's unassigned cards to send it. */
function IdleTarget({ agent, cards }: { agent: AgentInfo; cards: TaskCard[] }) {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="tb-popover-anchor tb-idle">
      <button className="tb-idle-target" onClick={() => setOpen((o) => !o)}>
        Idle: send it a card
      </button>
      {open && (
        <PickList
          items={cards.map((c) => ({ key: c.id, label: c.title }))}
          empty="No cards for no one in To do."
          error={error}
          onPick={(key) => taskSend(key, agent.paneId).then(close, (e) => setError(String(e)))}
          onClose={close}
        />
      )}
    </div>
  );
}

function Empty() {
  return (
    <div className="wall-empty">
      <p className="wall-empty-hint">
        No cards yet: giving an agent a task (<code>mira agent send</code>,{" "}
        <code>team start</code>) adds one, and <em>+ Card</em> or{" "}
        <code>mira task add</code> writes one.
      </p>
    </div>
  );
}

/**
 * One card: its title opens its history under it, its description opens
 * in full, its agent's name goes to the agent while its pane is open.
 * Waiting on a person, it says what the agent asks and leads to it; given
 * to no one, it can be sent to an agent; in Doing, it leads to its history
 * and its trail and says how its turn is going.
 */
function Card({
  card,
  agent,
  tabAgents,
  place,
  reachable,
  live,
  now,
  compact = false,
  onGoTo,
  onTrail,
}: {
  card: TaskCard;
  /** In a swimlane's cell: one line of description, no "Asks" box or
   *  progress — the lane says who and how. */
  compact?: boolean;
  /** Its agent, while running. */
  agent: AgentInfo | null;
  tabAgents: AgentInfo[];
  place: string | null;
  reachable: boolean;
  live: boolean;
  now: number;
  onGoTo: (pane: string) => void;
  onTrail: (card: TaskCard) => void;
}) {
  const [open, setOpen] = useState(false);
  const [full, setFull] = useState(false);
  const history = useLogRead(
    live,
    open ? card.id : "",
    (id) => taskHistory(null, id),
    NO_HISTORY,
  );
  const asking = card.column === "needsYou";
  const unassigned = !card.pane;
  // Its agent's own task list, read only while it waits on a person.
  const { activity } = useAgentActivity(card.pane, live && asking && !compact && !!agent?.hasActivity);
  const tasks = activity?.session.tasks ?? null;
  const variant = (asking ? " needs-you" : unassigned ? " unassigned" : "") + (compact ? " compact" : "");
  return (
    <div className={`tb-card${variant}`}>
      <button
        className="tb-title"
        title={open ? "Hide its history" : "Show its history"}
        onClick={() => setOpen((o) => !o)}
      >
        {card.title}
      </button>
      {card.description && (
        <button
          className={`tb-description${full ? " full" : ""}`}
          title={full ? "Show less" : "Show all of it"}
          onClick={() => setFull((f) => !f)}
        >
          {card.description}
        </button>
      )}
      {/* Compact, what it waits on in one line, in place of the box. */}
      {asking && compact && agent && <AskingLine agent={agent} className="tb-asks-compact" />}
      {asking && !compact && agent && (agent.asking || agent.message) && (
        <div className="tb-asks">
          <span className="tb-asks-label">Asks:</span>
          <AskingLine agent={agent} className="tb-asks-line" />
        </div>
      )}
      {asking && !compact && tasks && (
        <div className="tb-progress" title={tasks.current ?? "Its task list"}>
          <span style={{ "--fill": `${percent(tasks)}%` } as React.CSSProperties} />
        </div>
      )}
      <div className="tb-meta">
        {unassigned ? (
          <span className="tb-assignee">Unassigned</span>
        ) : reachable && card.pane ? (
          <button className="tb-assignee reachable" title="Go to this agent" onClick={() => onGoTo(card.pane!)}>
            {card.assignee ?? "its agent"}
          </button>
        ) : (
          <span className="tb-assignee">{card.assignee ?? "its agent"}</span>
        )}
        <span className="tb-age">· {age(card.updatedMs, now)}</span>
        {place && <span className="tb-place">{place}</span>}
        {card.column === "doing" && card.failing > 0 && (
          <span className="tb-failing">
            {card.failing} failing {card.failing === 1 ? "step" : "steps"}
          </span>
        )}
        <span className="tb-actions-right">
          {asking && agent ? (
            <button className="tb-open" title="Show this agent large on its wall" onClick={() => {
              void focusPane(agent.paneId);
              showOnWall(agent.tabId, agent.paneId);
            }}>
              Open
            </button>
          ) : unassigned && card.source === "manual" ? (
            <AssignMenu card={card} agents={tabAgents} />
          ) : card.column === "doing" && card.pane ? (
            <>
              <button className="tb-outline" onClick={() => setOpen((o) => !o)}>
                History
              </button>
              <button className="tb-outline" onClick={() => onTrail(card)}>
                Trail
              </button>
            </>
          ) : card.pane ? (
            <button
              className="tb-trail-open"
              title="What its agent did for it: the Trail view, at this task's turn"
              onClick={() => onTrail(card)}
            >
              ⋯
            </button>
          ) : null}
        </span>
      </div>
      {open && (
        <div className="tb-card-history">
          <HistoryLines entries={history} now={now} />
        </div>
      )}
    </div>
  );
}

/** Assign ▾: sends a card written for no one to one of the tab's agents —
 *  the idle first, then the working; one waiting on a person cannot take
 *  it. */
function AssignMenu({ card, agents }: { card: TaskCard; agents: AgentInfo[] }) {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const close = useCallback(() => setOpen(false), []);
  const rank = (a: AgentInfo) => (a.status === "idle" ? 0 : a.status === "needsYou" ? 2 : 1);
  const sorted = [...agents].sort((a, b) => rank(a) - rank(b));
  return (
    <span className="tb-popover-anchor">
      <button className="tb-outline" onClick={() => setOpen((o) => !o)}>
        Assign ▾
      </button>
      {open && (
        <PickList
          items={sorted.map((a) => ({
            key: a.paneId,
            label: agentLabel(a),
            mark: <StatusMark status={a.status} small />,
            disabled: a.status === "needsYou" ? "It is waiting on a person: answer it first" : undefined,
          }))}
          empty="No agents in this tab."
          error={error}
          onPick={(pane) => taskSend(card.id, pane).then(close, (e) => setError(String(e)))}
          onClose={close}
        />
      )}
    </span>
  );
}

/** One choice of a pick list; `disabled` says why it cannot be picked. */
type PickItem = { key: string; label: string; mark?: React.ReactNode; disabled?: string };

/** A small menu of choices — agents to send a card to, cards to send an
 *  agent — that takes the keyboard as it opens. */
function PickList({
  items,
  empty,
  error,
  onPick,
  onClose,
}: {
  items: PickItem[];
  empty: string;
  error: string | null;
  onPick: (key: string) => void;
  onClose: () => void;
}) {
  const { ref, onKeyDown } = usePopover(onClose);
  useMenuFocus(ref);
  return (
    <div className="tb-menu right" ref={ref} tabIndex={-1} onKeyDown={onKeyDown}>
      {items.length === 0 && <div className="tb-menu-note">{empty}</div>}
      {items.map((item) => (
        <button
          key={item.key}
          disabled={!!item.disabled}
          title={item.disabled}
          onClick={() => onPick(item.key)}
        >
          {item.mark}
          <span className="tb-menu-label">{item.label}</span>
        </button>
      ))}
      {error && <div className="tb-popover-error">{error}</div>}
    </div>
  );
}

/**
 * The Trail view (21a): the scope's agents, and what the picked one did,
 * turn by turn, newest first, read from its own transcript or rollout
 * while on screen — its latest [`TRAIL_TURNS`] turns, more on asking, and
 * from a card, back to that card's turn. An agent that has exited stays
 * pickable from its cards: the app keeps its last file.
 */
function TrailView({
  agents,
  running,
  cards,
  picked,
  onPick,
  live,
  now,
}: {
  /** The scope's agents. */
  agents: AgentInfo[];
  /** Every agent running, in any tab. */
  running: AgentInfo[];
  /** The scope's cards: an agent's own in Doing heads its trail. */
  cards: TaskCard[];
  picked: { pane: string; label: string; at: number | null } | null;
  onPick: (agent: AgentInfo) => void;
  live: boolean;
  now: number;
}) {
  const windowVisible = useWindowVisible();
  const pane = picked?.pane ?? agents[0]?.paneId ?? null;
  const [older, setOlder] = useState<{ pane: string | null; turns: number }>({ pane, turns: TRAIL_TURNS });
  const limit = older.pane === pane ? older.turns : TRAIL_TURNS;
  const at = picked?.pane === pane ? picked.at : null;
  const since = at === null ? null : at - GIVEN_SLACK_MS;
  const { trail, readSince } = useTrail(pane, live && windowVisible, limit, since);
  const listed = agents.some((a) => a.paneId === pane);
  const agent = running.find((a) => a.paneId === pane) ?? null;
  const name = agent ? agentLabel(agent) : (picked?.label ?? "");
  const card = cards.find((c) => c.pane === pane && c.column === "doing") ?? null;
  const total = trail ? trail.older + trail.gap + trail.turns.length : null;
  return (
    <div className="tb-trail">
      <ul className="tb-agents">
        {!listed && picked && (
          <li>
            <button
              className="tb-agent on"
              title={agent ? "Its agent runs in another tab" : "Its agent is no longer running"}
            >
              <StatusMark status={agent?.status ?? null} small />
              <span className="tb-agent-name">{picked.label}</span>
              <span className="tb-agent-meta">{agent ? "other tab" : "exited"}</span>
            </button>
          </li>
        )}
        {agents.map((a) => (
          <li key={a.paneId}>
            <button
              className={`tb-agent${a.paneId === pane ? " on" : ""}`}
              onClick={() => onPick(a)}
            >
              <StatusMark status={a.status} small />
              <span className="tb-agent-name">{agentLabel(a)}</span>
              <span className="tb-agent-meta">
                {a.paneId === pane && total !== null
                  ? `${total} ${total === 1 ? "turn" : "turns"}`
                  : HARNESS_LABEL[a.harness]}
              </span>
            </button>
          </li>
        ))}
      </ul>
      <div className="tb-timeline">
        {pane === null ? (
          <p className="tb-reason">No agents here.</p>
        ) : trail === null ? null : trail.turns.length === 0 ? (
          <p className="tb-reason">{trail.reason ?? "Nothing done yet."}</p>
        ) : (
          <>
            {/* Scrolled to a card's turn only once read back to it. */}
            <Timeline
              key={pane}
              trail={trail}
              name={name}
              agent={agent}
              card={card}
              live={live}
              at={readSince === since ? at : null}
              now={now}
            />
            {trail.older + trail.gap > 0 && (
              <button className="tb-older" onClick={() => setOlder({ pane, turns: limit + TRAIL_TURNS })}>
                Show older turns
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

type TrailFilter = "all" | "failures" | "edits";

/** "14:05": the time of day a turn began. */
function clock(ms: number | null): string {
  if (ms === null) return "–";
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** How a turn stands: waiting on you, running, or done — only the latest
 *  of an agent can be anything but done. */
function turnState(latest: boolean, agent: AgentInfo | null): "waiting on you" | "running" | "done" {
  if (!latest) return "done";
  if (agent?.status === "needsYou") return "waiting on you";
  if (agent?.status === "working") return "running";
  return "done";
}

/**
 * The picked agent's trail: who it is, its card in Doing, and chips to
 * narrow the actions shown to failures or edits; then its turns, newest
 * first, each summed up in a line, the latest open and the rest folded.
 * Opened from a card, it opens and scrolls once to that card's turn: the
 * first to begin after the card was given.
 */
function Timeline({
  trail,
  name,
  agent,
  card,
  live,
  at,
  now,
}: {
  trail: AgentTrail;
  name: string;
  agent: AgentInfo | null;
  card: TaskCard | null;
  live: boolean;
  at: number | null;
  now: number;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const [filter, setFilter] = useState<TrailFilter>("all");
  const [cardHistoryOpen, setCardHistoryOpen] = useState(false);
  const cardHistory = useLogRead(
    live,
    cardHistoryOpen && card ? card.id : "",
    (id) => taskHistory(null, id),
    NO_HISTORY,
  );
  const target = useRef<HTMLLIElement>(null);
  // The card it last scrolled to: once per link, not on every read.
  const scrolledTo = useRef<number | null>(null);
  const turns = trail.turns;
  const latest = turns.length - 1;
  const after = at === null ? -1 : turns.findIndex((t) => t.startedMs !== null && t.startedMs >= at - GIVEN_SLACK_MS);
  // No turn begun since yet: the latest, where its turn will come.
  const goal = at !== null && after === -1 ? latest : after;
  // Turns opened or folded by hand, by index; the latest and a card's
  // own start open.
  const [toggled, setToggled] = useState<Map<number, boolean>>(new Map());
  const isOpen = (i: number) => toggled.get(i) ?? (i === latest || i === goal);
  useEffect(() => {
    if (at === null || scrolledTo.current === at) return;
    target.current?.scrollIntoView({ block: "start" });
    scrolledTo.current = at;
  }, [at, trail]);
  const all = turns.flatMap((t) => t.actions);
  const failures = all.filter((a) => a.status === "error").length;
  const edits = all.filter((a) => a.kind === "edit").length;
  const shows = (a: TrailAction) =>
    filter === "all" || (filter === "failures" ? a.status === "error" : a.kind === "edit");
  // The turn's number among all the agent's: those not read come first,
  // and a card's gap after the first one shown.
  const number = (i: number) => trail.older + i + 1 + (trail.gap > 0 && i > 0 ? trail.gap : 0);
  return (
    <>
      <div className="tb-trail-head">
        <span className="tb-trail-name">{name}</span>
        {agent?.status && (
          <span className={`tb-trail-status ${agent.status}`}>{STATUS_LABEL[agent.status]}</span>
        )}
        {card && (
          <button className="tb-trail-card" title="Its history" onClick={() => setCardHistoryOpen((o) => !o)}>
            Card: {card.title}
          </button>
        )}
        <span className="tb-toolbar-spacer" />
        {(
          [
            ["all", "All", null],
            ["failures", "Failures", failures],
            ["edits", "Edits", edits],
          ] as [TrailFilter, string, number | null][]
        ).map(([id, label, n]) => (
          <button
            key={id}
            className={`tb-chip ${id}${filter === id ? " selected" : ""}`}
            onClick={() => setFilter(id)}
          >
            {label}
            {n !== null ? ` ${n}` : ""}
          </button>
        ))}
      </div>
      {cardHistoryOpen && card && (
        <div className="tb-card-history tb-trail-card-history">
          <HistoryLines entries={cardHistory} now={now} />
        </div>
      )}
      <ol className="tb-turns">
        {turns
          .map((turn, i) => ({ turn, i }))
          .reverse()
          .map(({ turn, i }) => {
            const state = turnState(i === latest, agent);
            const actions = turn.actions.filter(shows);
            return (
              <li
                key={`${turn.startedMs}-${i}`}
                ref={i === goal ? target : undefined}
                className={`tb-turn${state === "waiting on you" ? " waiting" : ""}`}
              >
                {i === 0 && trail.gap > 0 && (
                  <p className="tb-gap">
                    {trail.gap} {trail.gap === 1 ? "turn" : "turns"} not shown
                  </p>
                )}
                {isOpen(i) ? (
                  <>
                    <button
                      className="tb-turn-line"
                      title="Fold it"
                      onClick={() => setToggled((m) => new Map(m).set(i, false))}
                    >
                      ▾ <TurnSummary turn={turn} number={number(i)} state={state} now={now} />
                    </button>
                    <TurnHead turn={turn} />
                    <ol className="tb-actions">
                      {actions.map((a) => (
                        <ActionLine
                          key={a.id}
                          pane={trail.paneId}
                          action={a}
                          asking={state === "waiting on you"}
                          open={open === a.id}
                          onToggle={() => setOpen((o) => (o === a.id ? null : a.id))}
                        />
                      ))}
                    </ol>
                  </>
                ) : (
                  <button
                    className="tb-turn-line folded"
                    title={turn.startedMs ? new Date(turn.startedMs).toLocaleString() : undefined}
                    onClick={() => setToggled((m) => new Map(m).set(i, true))}
                  >
                    <span className="tb-turn-fold">▸</span>
                    <span className="tb-when">{clock(turn.startedMs)}</span>
                    <span className="tb-prompt">{turn.prompt ?? "(no prompt)"}</span>
                  </button>
                )}
              </li>
            );
          })}
      </ol>
    </>
  );
}

/** A turn in one line: when, which, how it stands, what it did. */
function TurnSummary({
  turn,
  number,
  state,
  now,
}: {
  turn: TrailTurn;
  number: number;
  state: string;
  now: number;
}) {
  const reads = turn.actions.filter((a) => a.kind === "read").length;
  const edits = turn.actions.filter((a) => a.kind === "edit").length;
  const failed = turn.actions.filter((a) => a.status === "error").length;
  const end = state === "done" ? turn.lastMs : now;
  const lasted = turn.startedMs !== null && end !== null ? took(Math.max(0, end - turn.startedMs)) : null;
  const rest = [
    state,
    `${reads} ${reads === 1 ? "read" : "reads"}`,
    `${edits} ${edits === 1 ? "edit" : "edits"}`,
    `${failed} failed`,
    lasted,
  ];
  return (
    <span className={`tb-turn-summary${failed > 0 ? " failed" : ""}`}>
      <span>{clock(turn.startedMs)} ·</span>
      <span>Turn {number} ·</span>
      <span className="tb-turn-rest">{rest.filter(Boolean).join(" · ")}</span>
    </span>
  );
}

/** The turn's prompt: its first line, opening to all of it when it says
 *  more. */
function TurnHead({ turn }: { turn: TrailTurn }) {
  const prompt = <span className="tb-prompt">{turn.prompt ?? "(no prompt)"}</span>;
  // Only a prompt with more than its first line opens.
  if (!turn.text || turn.text === turn.prompt) {
    return <div className="tb-turn-head">{prompt}</div>;
  }
  return (
    <details className="tb-turn-head">
      <summary>{prompt}</summary>
      <pre className="tb-text">{turn.text}</pre>
    </details>
  );
}

function ActionLine({
  pane,
  action,
  asking,
  open,
  onToggle,
}: {
  pane: string;
  action: TrailAction;
  /** Its turn waits on a person: a call still running is what it asks. */
  asking: boolean;
  open: boolean;
  onToggle: () => void;
}) {
  const exit = action.exitCode !== null && action.exitCode !== 0 ? `exit ${action.exitCode}` : null;
  const waiting = asking && action.status === "running";
  const mark = waiting ? "?" : action.status === "running" ? "●" : MARKS[action.status];
  return (
    <li className={`tb-action ${action.status}${waiting ? " asking" : ""}`}>
      <button className="tb-action-line" title={action.output || undefined} onClick={onToggle}>
        <span className="tb-action-mark">{mark}</span>
        <span className="tb-action-tool">{action.tool}</span>
        <span className="tb-action-summary">{action.summary}</span>
        <span className="tb-action-took">
          {exit ?? (action.durationMs === null ? "" : took(action.durationMs))}
        </span>
      </button>
      {action.status === "error" && action.output && !open && (
        <div className="tb-action-error">
          <div className="tb-detail-head">output (error)</div>
          <pre className="tb-detail-text">{action.output}</pre>
        </div>
      )}
      {open && <ActionDetailView pane={pane} id={action.id} />}
    </li>
  );
}

/** One action's input and output, read once when it is opened. As text:
 *  what an agent read or ran is never drawn as HTML. */
function ActionDetailView({ pane, id }: { pane: string; id: string }) {
  const [detail, setDetail] = useState<ActionDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void getTrailDetail(pane, id).then(
      (d) => !cancelled && setDetail(d),
      (e) => !cancelled && setError(String(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [pane, id]);
  if (error) return <p className="tb-reason">{error}</p>;
  if (!detail) return <p className="tb-reason">Reading…</p>;
  return (
    <div className="tb-detail">
      <div className="tb-detail-head">input</div>
      <pre className="tb-detail-text">{detail.input}</pre>
      <div className="tb-detail-head">{detail.isError ? "output (error)" : "output"}</div>
      <pre className="tb-detail-text">{detail.output ?? "(no result yet)"}</pre>
    </div>
  );
}

/** How long an action took, as `mira agent trail` says it. */
function took(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return age(0, ms);
}

/**
 * `pane`'s trail — its last `turns` turns, and any begun since `sinceMs` —
 * read again [`POLL_MS`] after each read returns, while `enabled`: one
 * read at a time, as `useAgentActivity` reads, and a failed read keeps the
 * last good one. A read of the same version as the last (Rust's: the file,
 * how far into it was read, the window) is not set, so the view is not
 * drawn again every two seconds for nothing. Null until the
 * first read for this pane: the last pane's trail is never shown, not even
 * for a render, under another's name or scrolled to another's card. With
 * it, the `sinceMs` it was read with.
 */
function useTrail(
  pane: string | null,
  enabled: boolean,
  turns: number,
  sinceMs: number | null,
) {
  const [read, setRead] = useState<{ trail: AgentTrail; since: number | null } | null>(null);
  const seen = useRef("");
  useEffect(() => {
    if (!pane || !enabled) return;
    let cancelled = false;
    let timer: number | undefined;
    seen.current = "";
    const load = () => {
      void getAgentTrail(pane, turns, sinceMs)
        .then(
          (t) => {
            if (cancelled) return;
            const f = t.version ?? `reason:${t.reason}`;
            if (f !== seen.current) {
              seen.current = f;
              setRead({ trail: t, since: sinceMs });
            }
          },
          (e) => console.error(`mirador: task board: ${e}`),
        )
        .finally(() => {
          if (!cancelled) timer = window.setTimeout(load, POLL_MS);
        });
    };
    load();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [pane, enabled, turns, sinceMs]);
  const mine = read?.trail.paneId === pane ? read : null;
  return { trail: mine?.trail ?? null, readSince: mine?.since ?? null };
}

/** Whether the window is visible: not minimized, nor behind another space. */
function useWindowVisible(): boolean {
  const [visible, setVisible] = useState(() => document.visibilityState === "visible");
  useEffect(() => {
    const update = () => setVisible(document.visibilityState === "visible");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  return visible;
}

/**
 * Lines of the log as `mira agent history` prints them (each entry's
 * `summary`, made in Rust); a task given opens to its whole text.
 */
function HistoryLines({
  entries,
  now,
}: {
  entries: TaskHistoryEntry[];
  now: number;
}) {
  return (
    <ol className="tb-lines">
      {entries.map((e, i) => {
        const when = (
          <span className="tb-when" title={new Date(e.atMs).toLocaleString()}>
            {age(e.atMs, now)}
          </span>
        );
        return (
          <li key={`${e.atMs}-${i}`} className={`tb-line ${e.kind}`}>
            {e.kind === "given" && e.text ? (
              <details>
                <summary>
                  {when}
                  <span className="tb-what">{e.summary}</span>
                </summary>
                <pre className="tb-text">{e.text}</pre>
              </details>
            ) : (
              <>
                {when}
                <span className="tb-what">{e.summary}</span>
              </>
            )}
          </li>
        );
      })}
    </ol>
  );
}

/**
 * `read(key)`, again whenever the task log is appended to, while `live`;
 * `empty` for the key "", which reads nothing. One read at a time, as
 * `useAgentActivity` does: an append or a new key during a read marks it
 * dirty, and that read's answer, no longer the latest, is dropped for
 * exactly one more read when it returns. The key is kept in a ref the
 * read takes it from, so a new one is one more reason to read rather than
 * a second reader.
 */
function useLogRead<T>(
  live: boolean,
  key: string,
  read: (key: string) => Promise<T>,
  empty: T,
): T {
  const [value, setValue] = useState<T>(empty);
  const keyRef = useRef(key);
  const readRef = useRef(read);
  readRef.current = read;
  const kickRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!live) return;
    let cancelled = false;
    let reading = false;
    let dirty = false;
    const kick = () => {
      if (cancelled) return;
      if (reading) {
        dirty = true;
        return;
      }
      dirty = false;
      const want = keyRef.current;
      if (want === "") {
        setValue(empty);
        return;
      }
      reading = true;
      void readRef
        .current(want)
        .then(
          (v) => {
            if (!cancelled && !dirty) setValue(v);
          },
          (e) => console.error(`mirador: task board: ${e}`),
        )
        .finally(() => {
          reading = false;
          if (!cancelled && dirty) kick();
        });
    };
    kickRef.current = kick;
    // Listening first: an append between the first read and the
    // subscription would otherwise go unseen.
    const unlisten = listen("tasks-changed", kick);
    void unlisten.then(kick);
    return () => {
      cancelled = true;
      kickRef.current = () => {};
      void unlisten.then((f) => f());
    };
  }, [live, empty]);

  useEffect(() => {
    if (keyRef.current === key) return;
    keyRef.current = key;
    kickRef.current();
  }, [key]);

  return value;
}

/** The current time, ticking every ten seconds while `active`. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 10_000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}
