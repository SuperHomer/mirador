/**
 * The task board, as a static mock: the omakase team's board, a few
 * minutes after TeamsShowcase's handoff, on the same "add OAuth" task.
 *
 * Kept honest to the app (src/agents/TaskBoard.tsx and its rules in
 * styles.css): the one toolbar — Cards · History · Trail, Columns |
 * Swimlanes, Scope ▾ and + Card — and its four columns in order, Needs you
 * first and widest, each headed by its status mark. The cards are the
 * app's variants: a title and a description; for one whose agent waits on
 * you, what it asks and Open (there is no answering from the board); one
 * written for no one, dashed, with Assign ▾; one in Doing with its
 * failing steps and History and Trail; ⋯ to the Trail on the rest; and
 * Done folded to today's. At phone width the columns go two by two, as
 * the app's do in a narrow split.
 *
 * Under it, the Trail view on the worker, as TaskBoard.tsx's TrailView
 * draws it: the scope's agents with their status mark, the picked one's
 * turns newest first, each summed up in a line — when, its number, how it
 * stands, reads, edits, failures and how long — then its prompt and its
 * actions, a failure's output shown inline, the older turn folded to its
 * prompt; and the All / Failures / Edits chips. At phone width the agents
 * become a row over the turns, as in a narrow split.
 */

import { Mark, type Status } from "./MockAgents";

type Card = {
  title: string;
  description?: string;
  /** Its agent; none for a card written for no one. */
  agent?: string;
  age: string;
  /** What its agent waits on, in Needs you. */
  asks?: string;
  failing?: number;
};

type Column = { key: "needsYou" | "todo" | "doing" | "done"; label: string; mark: Status | "done"; cards: Card[] };

const COLUMNS: Column[] = [
  {
    key: "needsYou",
    label: "Needs you",
    mark: "needsYou",
    cards: [{ title: "add OAuth", agent: "planner", age: "40s", asks: "Bash git push origin feat/oauth" }],
  },
  {
    key: "todo",
    label: "To do",
    mark: null,
    cards: [
      {
        title: "Document the refresh flow",
        description: "Where the tokens live, how long each lasts, and what logout revokes.",
        age: "5m",
      },
      {
        title: "Step 4: revoke tokens on logout",
        description: "Delete the user's refresh tokens on logout; a test that one left behind fails.",
        agent: "worker",
        age: "21m",
      },
    ],
  },
  {
    key: "doing",
    label: "Doing",
    mark: "working",
    cards: [
      {
        title: "Step 3: expire refresh tokens after 30 days",
        description: "Reject a refresh token older than 30 days with Expired, and test it.",
        agent: "worker",
        age: "1m",
        failing: 1,
      },
    ],
  },
  {
    key: "done",
    label: "Done",
    mark: "done",
    cards: [
      { title: "Step 2: rotate the refresh token on use", agent: "worker", age: "12m" },
      { title: "Step 1: add the refresh_tokens table", agent: "worker", age: "26m" },
    ],
  },
];

/** The board's one toolbar, on one of its three views. */
function Toolbar({ view }: { view: "cards" | "history" | "trail" }) {
  return (
    <div className="tk-toolbar">
      {(["cards", "history", "trail"] as const).map((v) => (
        <span key={v} className={`tk-tab${view === v ? " selected" : ""}`}>
          {v[0].toUpperCase() + v.slice(1)}
        </span>
      ))}
      <span className="tk-spacer" />
      {view === "cards" && (
        <span className="ms-segmented tk-layout">
          <span className="selected">Columns</span>
          <span>Swimlanes</span>
        </span>
      )}
      <span className="tk-outline">Scope: omakase ▾</span>
      <span className="tk-outline">+ Card</span>
    </div>
  );
}

function TaskCard({ card, column }: { card: Card; column: Column["key"] }) {
  const variant = card.asks ? " needs-you" : !card.agent ? " unassigned" : "";
  return (
    <div className={`tk-card${variant}`}>
      <div className="tk-title">{card.title}</div>
      {card.description && <div className="tk-description">{card.description}</div>}
      {card.asks && (
        <div className="tk-asks">
          <span>Asks:</span> <span className="tk-asks-line">{card.asks}</span>
        </div>
      )}
      <div className="tk-meta">
        <span className="tk-assignee">{card.agent ?? "Unassigned"}</span>
        <span>· {card.age}</span>
        {card.failing ? (
          <span className="tk-failing">
            {card.failing} failing {card.failing === 1 ? "step" : "steps"}
          </span>
        ) : null}
        <span className="tk-actions">
          {card.asks ? (
            <span className="tk-open">Open</span>
          ) : !card.agent ? (
            <span className="tk-outline">Assign ▾</span>
          ) : column === "doing" ? (
            <>
              <span className="tk-outline">History</span>
              <span className="tk-outline">Trail</span>
            </>
          ) : (
            <span className="tk-more">⋯</span>
          )}
        </span>
      </div>
    </div>
  );
}

function TaskBoardMockup() {
  return (
    <div className="tk-mock">
      <Toolbar view="cards" />
      <div className="tk-columns tasks-columns">
        {COLUMNS.map((col) => (
          <section key={col.key} className={`tk-column ${col.key}`}>
            <h3 className="tk-column-head">
              {col.mark === "done" ? <span className="tk-done-mark">✓</span> : <Mark status={col.mark} small />}
              {col.label}
              <span className="tk-count">{col.cards.length}</span>
              {col.key === "done" && <span className="tk-done-toggle">today ▾</span>}
            </h3>
            <div className="tk-cards">
              {col.cards.map((c) => (
                <TaskCard key={c.title} card={c} column={col.key} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}

type Action = {
  status: "ok" | "error" | "running";
  tool: string;
  summary: string;
  /** Its exit code, shown in place of how long it took. */
  exit?: number;
  took?: string;
  /** A failure's output, shown under it without opening it. */
  error?: string;
  kind?: "read" | "edit";
};

const MARKS = { ok: "✓", error: "✗", running: "●" } as const;

const REFRESH = "/Users/me/api/src/auth/refresh.rs";
const FROM_PLANNER = "[message from planner, via mira agent send]";

const FAILED_TEST = `test auth::refresh::expires_after_30_days ... FAILED
assertion \`left == right\` failed
  left: Ok(Refreshed)
 right: Err(Expired)
test result: FAILED. 43 passed; 1 failed`;

/** The worker's turns, newest first: the latest open, the other folded. */
const TURNS: { when: string; number: number; state: string; took: string; prompt: string; actions: Action[] }[] = [
  {
    when: "14:05",
    number: 4,
    state: "running",
    took: "52s",
    prompt: FROM_PLANNER,
    actions: [
      { status: "ok", tool: "Read", summary: REFRESH, took: "9ms", kind: "read" },
      { status: "ok", tool: "Grep", summary: "issued_at", took: "41ms", kind: "read" },
      { status: "ok", tool: "Edit", summary: REFRESH, took: "22ms", kind: "edit" },
      { status: "error", tool: "Bash", summary: "cargo test -p api", exit: 101, error: FAILED_TEST },
      { status: "ok", tool: "Edit", summary: REFRESH, took: "18ms", kind: "edit" },
      { status: "ok", tool: "Bash", summary: "cargo test -p api", took: "7.2s" },
      { status: "running", tool: "Bash", summary: "cargo clippy -p api -- -D warnings" },
    ],
  },
  {
    when: "13:59",
    number: 3,
    state: "done",
    took: "3m",
    prompt: FROM_PLANNER,
    actions: [],
  },
];

/** The team's agents, as the Trail view lists them, the worker picked. */
const AGENTS: { name: string; status: Status; meta: string; on?: boolean }[] = [
  { name: "planner", status: "needsYou", meta: "Claude Code" },
  { name: "worker", status: "working", meta: "4 turns", on: true },
  { name: "reviewer", status: null, meta: "Codex" },
];

function TurnSummary({ turn }: { turn: (typeof TURNS)[number] }) {
  const reads = turn.actions.filter((a) => a.kind === "read").length;
  const edits = turn.actions.filter((a) => a.kind === "edit").length;
  const failed = turn.actions.filter((a) => a.status === "error").length;
  return (
    <span className={`tk-turn-summary${failed > 0 ? " failed" : ""}`}>
      ▾ {turn.when} · Turn {turn.number} · {turn.state} · {reads} reads · {edits} edits · {failed} failed · {turn.took}
    </span>
  );
}

function TrailMockup() {
  const all = TURNS.flatMap((t) => t.actions);
  const failures = all.filter((a) => a.status === "error").length;
  const edits = all.filter((a) => a.kind === "edit").length;
  return (
    <div className="tk-mock trail">
      <Toolbar view="trail" />
      <div className="tk-trail tasks-trail">
        <ul className="tk-agents tasks-agents">
          {AGENTS.map((a) => (
            <li key={a.name} className={`tk-agent${a.on ? " on" : ""}`}>
              <Mark status={a.status} small />
              <span>{a.name}</span>
              <span className="tk-agent-meta tasks-agent-meta">{a.meta}</span>
            </li>
          ))}
        </ul>
        <div className="tk-timeline">
          <div className="tk-trail-head">
            <span className="tk-trail-name">worker</span>
            <span className="tk-trail-status">working</span>
            <span className="tk-trail-card">Card: Step 3: expire refresh tokens after 30 days</span>
            <span className="tk-spacer" />
            <span className="tk-chip selected">All</span>
            <span className="tk-chip">Failures {failures}</span>
            <span className="tk-chip">Edits {edits}</span>
          </div>
          <ol className="tk-turns">
            {TURNS.map((turn) =>
              turn.actions.length > 0 ? (
                <li key={turn.number} className="tk-turn">
                  <TurnSummary turn={turn} />
                  <div className="tk-prompt">{turn.prompt}</div>
                  <ol className="tk-action-list">
                    {turn.actions.map((a, i) => (
                      <li key={i} className={`tk-action ${a.status}`}>
                        <div className="tk-action-line">
                          <span className="tk-action-mark">{MARKS[a.status]}</span>
                          <span className="tk-action-tool">{a.tool}</span>
                          <span className="tk-action-summary mono">{a.summary}</span>
                          <span className="tk-action-took">{a.exit !== undefined ? `exit ${a.exit}` : (a.took ?? "")}</span>
                        </div>
                        {a.error && (
                          <div className="tk-action-error">
                            <div className="tk-detail-head">output (error)</div>
                            <pre className="tk-detail-text mono">{a.error}</pre>
                          </div>
                        )}
                      </li>
                    ))}
                  </ol>
                </li>
              ) : (
                <li key={turn.number} className="tk-turn folded">
                  <span className="tk-fold">▸</span> <span className="tk-when">{turn.when}</span>{" "}
                  <span className="tk-prompt">{turn.prompt}</span>
                </li>
              ),
            )}
          </ol>
        </div>
      </div>
    </div>
  );
}

export default function TasksShowcase() {
  return (
    <section
      id="tasks"
      style={{
        padding: "66px 48px",
        borderTop: "1px solid var(--surface)",
        background: "var(--bg-alt)",
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 720, margin: "0 auto" }}>
        <span
          className="mono"
          style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}
        >
          TASKS
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Who was given what, and where it stands
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          The task board is the wall’s Tasks side and shows what each agent was
          given, for one team or every tab: Needs you, To do, Doing and Done, or
          swimlanes, a row per agent. Mirador adds a card each time it hands an agent
          a task, and moves it as the agent’s turns end. Write one yourself with a
          title and a description, assign it to an agent, or send an idle agent its
          next card; agents add their own
          with <span className="mono" style={{ color: "var(--text)" }}>mira task add</span> —
          the omakase planner puts up its steps that way. History lists every task
          given, card moved and turn ended, by day; open a card for its own. The Trail
          shows what each agent actually did, turn by turn: its prompt, then every
          command, edit, read and search with how it ended, each opening to its full
          input and output — read from Claude Code’s and Codex’s own session files,
          never copied.
        </p>
        <div
          style={{
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            gap: 10,
            marginTop: 22,
            fontSize: 13.5,
            color: "var(--subtext)",
            flexWrap: "wrap",
          }}
        >
          <span className="mono" style={{ color: "var(--text)" }}>
            mira agent board
          </span>
          <span className="mono" style={{ color: "var(--muted)", fontSize: 12.5 }}>
            · mira task add · mira agent history · mira agent trail
          </span>
          <span style={{ color: "var(--muted)", fontSize: 12.5 }}>
            or Tasks in the sidebar’s switch, or the palette’s Agents: Task Board
          </span>
        </div>
      </div>
      <TaskBoardMockup />
      <TrailMockup />
    </section>
  );
}
