import { memo, useEffect, useMemo, useRef, useState } from "react";
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
  closePane,
  focusPane,
  getAgentTrail,
  getTrailDetail,
  taskCards,
  taskHistory,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { flyToCard } from "../state/boardStore";

type Props = {
  paneId: string;
  /** The tab whose cards it shows: the one it was opened from. */
  tabId: string | null;
  /** On screen: only then does it read the log. */
  visible: boolean;
  /** Drawn as a card on the board. */
  board: boolean;
};

const COLUMNS: [TaskColumn, string][] = [
  ["todo", "To do"],
  ["doing", "Doing"],
  ["needsYou", "Needs you"],
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

/** What the board reads: every tab's ("*"), one tab's, or nothing (""). */
const scopeOf = (key: string) => (key === "*" ? null : key);

/**
 * The task log's cards (`cmux_core::tasks`), in four columns — the tasks
 * given to a tab's agents, its team's in a team's tab, or every tab's —
 * or the log itself, newest first. Read again whenever it is appended to.
 */
export const TaskBoard = memo(function TaskBoard({
  paneId,
  tabId,
  visible,
  board,
}: Props) {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const shown = snapshot?.tabs.find((t) => t.id === tabId);
  const [all, setAll] = useState(false);
  const [view, setView] = useState<"cards" | "history" | "trail">("cards");
  // The agent the Trail view shows, and the time of the card it was
  // opened from, to scroll to that card's turn.
  const [trailOf, setTrailOf] = useState<{ pane: string; label: string; at: number | null } | null>(null);
  const live = visible || board;
  const now = useNow(live);

  // With its tab closed, only every tab's is left to show.
  const scope = all ? "*" : (tabId ?? "");
  const cards = useLogRead(
    live,
    view === "cards" ? scope : "",
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
    if (board) flyToCard(pane);
  };

  const agents = useMemo(
    () => (snapshot?.agents ?? []).filter((a) => all || a.tabId === tabId),
    [snapshot, all, tabId],
  );
  const showTrail = (card: TaskCard) => {
    setTrailOf({ pane: card.pane!, label: card.assignee ?? "its agent", at: card.createdMs });
    setView("trail");
  };

  return (
    <div className="pane task-board">
      <div className="wall-chrome">
        <span className="wall-badge">Tasks</span>
        <span className="wall-tab" title="The tab this board shows the cards of">
          {shown ? (shown.team?.name ?? shown.title) : "closed tab"}
        </span>
        <div className="tb-switch" role="group">
          <button
            className={view === "cards" ? "on" : ""}
            onClick={() => setView("cards")}
          >
            Cards
          </button>
          <button
            className={view === "history" ? "on" : ""}
            title="Every task given and every turn ended, newest first"
            onClick={() => setView("history")}
          >
            History
          </button>
          <button
            className={view === "trail" ? "on" : ""}
            title="What each agent did, turn by turn, from its own transcript"
            onClick={() => setView("trail")}
          >
            Trail
          </button>
        </div>
        <div className="tb-switch" role="group">
          <button className={all ? "" : "on"} onClick={() => setAll(false)}>
            {shown?.team ? "This team" : "This tab"}
          </button>
          <button className={all ? "on" : ""} onClick={() => setAll(true)}>
            All agents
          </button>
        </div>
        <button
          className="wall-close"
          title="Close task board"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>

      {view === "trail" ? (
        <TrailView
          agents={agents}
          running={snapshot?.agents ?? NO_AGENTS}
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
      ) : cards.length === 0 ? (
        <Empty />
      ) : (
        <div className="tb-columns">
          {COLUMNS.map(([column, label]) => {
            const here = cards
              .filter((c) => c.column === column)
              .sort((a, b) => b.updatedMs - a.updatedMs);
            return (
              <section key={column} className={`tb-column ${column}`}>
                <h3 className="tb-column-head">
                  {label}
                  <span className="tb-count">{here.length}</span>
                </h3>
                <div className="tb-cards">
                  {here.map((c) => (
                    <Card
                      key={c.id}
                      card={c}
                      place={
                        all ? ((c.tab && placeOf.get(c.tab)) ?? "closed tab") : null
                      }
                      reachable={!!c.pane && panes.has(c.pane)}
                      live={live}
                      now={now}
                      onGoTo={goTo}
                      onTrail={showTrail}
                    />
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
});

function Empty() {
  return (
    <div className="wall-empty">
      <p className="wall-empty-hint">
        No cards yet: giving an agent a task (<code>mira agent send</code>,{" "}
        <code>team start</code>) adds one, and <code>mira task add</code>{" "}
        writes one.
      </p>
    </div>
  );
}

/**
 * One card: its title opens its history under it; its agent's name, when
 * that agent's pane is still open, goes to it.
 */
function Card({
  card,
  place,
  reachable,
  live,
  now,
  onGoTo,
  onTrail,
}: {
  card: TaskCard;
  place: string | null;
  reachable: boolean;
  live: boolean;
  now: number;
  onGoTo: (pane: string) => void;
  onTrail: (card: TaskCard) => void;
}) {
  const [open, setOpen] = useState(false);
  const history = useLogRead(
    live,
    open ? card.id : "",
    (id) => taskHistory(null, id),
    NO_HISTORY,
  );
  const manual = card.source === "manual";
  return (
    <div className={`tb-card${open ? " open" : ""}`}>
      <button
        className="tb-title"
        title={open ? "Hide its history" : "Show its history"}
        onClick={() => setOpen((o) => !o)}
      >
        {card.title}
      </button>
      <div className="tb-meta">
        <span
          className="tb-mark"
          title={
            manual
              ? `Written with mira task add (${card.id})`
              : "Given to its agent: follows its turns"
          }
        >
          {manual ? "✎" : "⇢"}
        </span>
        {reachable && card.pane ? (
          <button
            className="tb-assignee reachable"
            title="Go to this agent"
            onClick={() => onGoTo(card.pane!)}
          >
            {card.assignee ?? "its agent"}
          </button>
        ) : (
          <span className="tb-assignee">{card.assignee ?? "nobody yet"}</span>
        )}
        {place && <span className="tb-place">{place}</span>}
        <span className="tb-age">{age(card.updatedMs, now)}</span>
        {card.pane && (
          <button
            className="tb-trail-open"
            title="What its agent did for it: the Trail view, at this task's turn"
            onClick={() => onTrail(card)}
          >
            ⋯
          </button>
        )}
      </div>
      {open && (
        <div className="tb-card-history">
          <HistoryLines entries={history} now={now} />
        </div>
      )}
    </div>
  );
}

/**
 * The Trail view: the scope's agents, and what the picked one did, turn
 * by turn, newest first, read from its own transcript or rollout while on
 * screen — its latest [`TRAIL_TURNS`] turns, more on asking, and from a
 * card, back to that card's turn. An agent that has exited stays pickable
 * from its cards: the app keeps its last file.
 */
function TrailView({
  agents,
  running,
  picked,
  onPick,
  live,
  now,
}: {
  /** The scope's agents. */
  agents: AgentInfo[];
  /** Every agent running, in any tab. */
  running: AgentInfo[];
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
  // Picked from a card: out of this scope but running, or gone.
  const alive = running.some((a) => a.paneId === pane);
  return (
    <div className="tb-trail">
      <ul className="tb-agents">
        {!listed && picked && (
          <li>
            <button
              className="tb-agent on"
              title={alive ? "Its agent runs in another tab" : "Its agent is no longer running"}
            >
              <span className="tb-agent-name">{picked.label}</span>
              <span className="tb-agent-meta">{alive ? "other tab" : "exited"}</span>
            </button>
          </li>
        )}
        {agents.map((a) => (
          <li key={a.paneId}>
            <button
              className={`tb-agent${a.paneId === pane ? " on" : ""}`}
              onClick={() => onPick(a)}
            >
              <span className={`tb-agent-dot ${a.status ?? "unknown"}`} />
              <span className="tb-agent-name">{agentLabel(a)}</span>
              <span className="tb-agent-meta">{HARNESS_LABEL[a.harness]}</span>
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
            <Timeline key={pane} trail={trail} at={readSince === since ? at : null} now={now} />
            {trail.turns.length >= limit && (
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

/**
 * Turns, newest first, each with its actions in order; one action open at
 * a time, its detail read when opened. Opened from a card, it scrolls once
 * to that card's turn: the first to begin after the card was given.
 */
function Timeline({
  trail,
  at,
  now,
}: {
  trail: AgentTrail;
  at: number | null;
  now: number;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const target = useRef<HTMLLIElement>(null);
  // The card it last scrolled to: once per link, not on every read.
  const scrolledTo = useRef<number | null>(null);
  const turns = trail.turns;
  const after = at === null ? -1 : turns.findIndex((t) => t.startedMs !== null && t.startedMs >= at - GIVEN_SLACK_MS);
  // No turn begun since yet: the latest, where its turn will come.
  const goal = at !== null && after === -1 ? turns.length - 1 : after;
  useEffect(() => {
    if (at === null || scrolledTo.current === at) return;
    target.current?.scrollIntoView({ block: "start" });
    scrolledTo.current = at;
  }, [at, trail]);
  return (
    <ol className="tb-turns">
      {turns
        .map((turn, i) => ({ turn, i }))
        .reverse()
        .map(({ turn, i }) => (
          <li key={`${turn.startedMs}-${i}`} ref={i === goal ? target : undefined} className="tb-turn">
            <TurnHead turn={turn} now={now} />
            <ol className="tb-actions">
              {turn.actions.map((a) => (
                <ActionLine
                  key={a.id}
                  pane={trail.paneId}
                  action={a}
                  open={open === a.id}
                  onToggle={() => setOpen((o) => (o === a.id ? null : a.id))}
                />
              ))}
            </ol>
          </li>
        ))}
    </ol>
  );
}

function TurnHead({ turn, now }: { turn: TrailTurn; now: number }) {
  const when = (
    <span className="tb-when" title={turn.startedMs ? new Date(turn.startedMs).toLocaleString() : undefined}>
      {turn.startedMs ? age(turn.startedMs, now) : "–"}
    </span>
  );
  const prompt = <span className="tb-prompt">{turn.prompt ?? "(no prompt)"}</span>;
  // Only a prompt with more than its first line opens.
  if (!turn.text || turn.text === turn.prompt) {
    return (
      <div className="tb-turn-head">
        {when}
        {prompt}
      </div>
    );
  }
  return (
    <details className="tb-turn-head">
      <summary>
        {when}
        {prompt}
      </summary>
      <pre className="tb-text">{turn.text}</pre>
    </details>
  );
}

function ActionLine({
  pane,
  action,
  open,
  onToggle,
}: {
  pane: string;
  action: TrailAction;
  open: boolean;
  onToggle: () => void;
}) {
  const exit = action.exitCode !== null && action.exitCode !== 0 ? `exit ${action.exitCode}` : null;
  return (
    <li className={`tb-action ${action.status}`}>
      <button className="tb-action-line" title={action.output || undefined} onClick={onToggle}>
        <span className="tb-action-mark">{MARKS[action.status]}</span>
        <span className="tb-action-tool">{action.tool}</span>
        <span className="tb-action-summary">{action.summary}</span>
        {exit && <span className="tb-action-exit">{exit}</span>}
        <span className="tb-action-took">{action.durationMs === null ? "" : took(action.durationMs)}</span>
      </button>
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

/** What a trail read says, in short: a read saying the same as the last
 *  changes nothing on screen, and is not drawn again. A turn's `lastMs`
 *  moves with every entry that lands in it, a result included. */
function fingerprint(trail: AgentTrail): string {
  return `${trail.reason}|${trail.turns.map((t) => `${t.startedMs}:${t.lastMs}:${t.actions.length}`).join(",")}`;
}

/**
 * `pane`'s trail — its last `turns` turns, and any begun since `sinceMs` —
 * read again [`POLL_MS`] after each read returns, while `enabled`: one
 * read at a time, as `useAgentActivity` reads, and a failed read keeps the
 * last good one. A read that says what the last did is not set, so the
 * view is not drawn again every two seconds for nothing. Null until the
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
            const f = fingerprint(t);
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
