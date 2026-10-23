import { memo, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Node,
  TaskCard,
  TaskColumn,
  TaskHistoryEntry,
  closePane,
  focusPane,
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

const NO_CARDS: TaskCard[] = [];
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
  const [view, setView] = useState<"cards" | "history">("cards");
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

      {view === "history" ? (
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
}: {
  card: TaskCard;
  place: string | null;
  reachable: boolean;
  live: boolean;
  now: number;
  onGoTo: (pane: string) => void;
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
