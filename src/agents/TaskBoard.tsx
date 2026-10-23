import { memo, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Node,
  TaskCard,
  TaskColumn,
  closePane,
  focusPane,
  taskCards,
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

function paneIds(node: Node): string[] {
  return node.type === "leaf" ? [node.paneId] : node.children.flatMap(paneIds);
}

/** "42s", "3m", "1h", "2d" — how long since a card last moved. */
function age(ms: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

/**
 * The cards of the task log (`cmux_core::tasks`), in four columns: the
 * tasks given to a tab's agents — its team's, in a team's tab — or, at a
 * switch, every tab's. Read again whenever the log is appended to.
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
  const [cards, setCards] = useState<TaskCard[]>([]);
  const live = visible || board;
  const now = useNow(live);

  useEffect(() => {
    if (!live) return;
    // With its tab closed, only every tab's cards are left to show.
    if (!all && !tabId) {
      setCards([]);
      return;
    }
    let alive = true;
    const load = () =>
      taskCards(all ? null : tabId)
        .then((c) => alive && setCards(c))
        .catch((e) => console.error(`mirador: task board: ${e}`));
    load();
    const unlisten = listen("tasks-changed", load);
    return () => {
      alive = false;
      void unlisten.then((f) => f());
    };
  }, [live, all, tabId]);

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

  const scope = shown?.team ? "This team" : "This tab";
  const open = (card: TaskCard) => {
    if (!card.pane || !panes.has(card.pane)) return;
    void focusPane(card.pane);
    if (board) flyToCard(card.pane);
  };

  return (
    <div className="pane task-board">
      <div className="wall-chrome">
        <span className="wall-badge">Tasks</span>
        <span className="wall-tab" title="The tab this board shows the cards of">
          {shown ? (shown.team?.name ?? shown.title) : "closed tab"}
        </span>
        <div className="tb-switch" role="group">
          <button className={all ? "" : "on"} onClick={() => setAll(false)}>
            {scope}
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

      {cards.length === 0 ? (
        <div className="wall-empty">
          <p className="wall-empty-hint">
            No cards yet: giving an agent a task (<code>mira agent send</code>,{" "}
            <code>team start</code>) adds one, and <code>mira task add</code>{" "}
            writes one.
          </p>
        </div>
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
                  {here.map((c) => {
                    const reachable = !!c.pane && panes.has(c.pane);
                    return (
                      <div
                        key={c.id}
                        className={`tb-card${reachable ? " reachable" : ""}`}
                        title={reachable ? "Go to this agent" : undefined}
                        onClick={() => open(c)}
                      >
                        <div className="tb-title">{c.title}</div>
                        <div className="tb-meta">
                          <span
                            className="tb-mark"
                            title={
                              c.source === "manual"
                                ? `Written with mira task add (${c.id})`
                                : "Given to its agent: follows its turns"
                            }
                          >
                            {c.source === "manual" ? "✎" : "⇢"}
                          </span>
                          <span className="tb-assignee">
                            {c.assignee ?? "nobody yet"}
                          </span>
                          {all && (
                            <span className="tb-place">
                              {(c.tab && placeOf.get(c.tab)) ?? "closed tab"}
                            </span>
                          )}
                          <span className="tb-age">{age(c.updatedMs, now)}</span>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
});

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
