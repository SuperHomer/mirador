/**
 * The task board, as a static mock: the omakase team's board, a few
 * minutes after TeamsShowcase's handoff, on the same "add OAuth" task.
 *
 * Kept honest to the app (src/agents/TaskBoard.tsx and its rules in
 * styles.css): its chrome — the Tasks badge, the tab's name (its team's,
 * in a team's tab), the Cards | History | Trail and This team | All
 * agents switches — and its four columns, in order, each card newest
 * first. A card shows what the app's does: its title, ✎ for one written
 * with `mira task add` or ⇢ for one Mirador gave, its agent's name, its
 * age and ⋯ to the Trail; no harness mark, which the board does not draw.
 * The cards fold the log as cmux-core's tasks.rs does: the planner's
 * steps are written by hand and moved by it, so they never follow a turn,
 * while a task sent without a card gets one that does. One card is open
 * on its history, in `mira agent history`'s words. At phone width the
 * columns go two by two, as the app's do in a narrow split.
 *
 * Under it, the Trail view on the worker, as TaskBoard.tsx's TrailView
 * draws it: the scope's agents with their status dot and harness, and
 * the picked one's turns newest first, each a prompt's first line (the
 * line `mira agent send` prefixes, so it opens to the rest) over its
 * actions — mark, tool, cmux-core's trail.rs one-line summary (a path
 * as the agent gave it, absolute), exit code and time — one opened on
 * its input and output, as the app reads them from the transcript.
 * At phone width the agents become a row over the turns, as in a
 * narrow split.
 */

type Card = {
  title: string;
  manual: boolean;
  agent: string;
  age: string;
  /** Its history, oldest first, when it is open. */
  history?: { when: string; what: string; muted?: boolean }[];
};

/** The app's --wall-working, --wall-needs-you and --wall-idle. */
const COLUMNS: { label: string; color: string; cards: Card[] }[] = [
  {
    label: "To do",
    color: "var(--muted)",
    cards: [
      { title: "Step 5: document the refresh flow", manual: true, agent: "worker", age: "21m" },
      { title: "Step 4: revoke tokens on logout", manual: true, agent: "worker", age: "21m" },
    ],
  },
  {
    label: "Doing",
    color: "var(--accent)",
    cards: [{ title: "Step 3: expire refresh tokens after 30 days", manual: true, agent: "worker", age: "1m" }],
  },
  {
    label: "Needs you",
    color: "#fab387",
    cards: [
      { title: "APPROVE — rotation is atomic and the test pins reuse.", manual: false, agent: "planner", age: "40s" },
    ],
  },
  {
    label: "Done",
    color: "var(--green)",
    cards: [
      { title: "Step 2: rotate the refresh token on use", manual: true, agent: "worker", age: "2m" },
      {
        title: "Step 2 is done: review the diff",
        manual: false,
        agent: "reviewer",
        age: "3m",
        history: [
          { when: "6m", what: "planner → reviewer  agent-send  Step 2 is done: review the diff" },
          { when: "3m", what: "reviewer done after 3m", muted: true },
        ],
      },
      { title: "Step 1: add the refresh_tokens table", manual: true, agent: "worker", age: "14m" },
      { title: "add OAuth", manual: false, agent: "planner", age: "16m" },
    ],
  },
];

const SWITCH_BUTTON = { padding: "2px 8px", whiteSpace: "nowrap" as const };

function Switch({ labels, on }: { labels: string[]; on: number }) {
  return (
    <div
      style={{
        display: "flex",
        flexShrink: 0,
        border: "1px solid var(--surface)",
        borderRadius: 4,
        overflow: "hidden",
        fontSize: 11,
      }}
    >
      {labels.map((l, i) => (
        <span
          key={l}
          style={{
            ...SWITCH_BUTTON,
            background: i === on ? "var(--surface)" : "transparent",
            color: i === on ? "var(--text)" : "var(--subtext)",
          }}
        >
          {l}
        </span>
      ))}
    </div>
  );
}

function TaskCard({ card, color }: { card: Card; color: string }) {
  return (
    <div
      style={{
        flexShrink: 0,
        padding: "5px 7px",
        border: "1px solid var(--surface)",
        borderLeft: `3px solid ${color}`,
        borderRadius: 4,
        background: "var(--bg-alt)",
        fontSize: 12,
      }}
    >
      <div style={{ color: "var(--text)", lineHeight: 1.35, overflowWrap: "anywhere" }}>{card.title}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 3, fontSize: 11, color: "var(--muted)", minWidth: 0 }}>
        <span style={{ flexShrink: 0, color: "var(--subtext)" }}>{card.manual ? "✎" : "⇢"}</span>
        <span style={{ color: "var(--subtext)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {card.agent}
        </span>
        <span style={{ flexShrink: 0, marginLeft: "auto" }}>{card.age}</span>
        <span style={{ flexShrink: 0, padding: "0 2px", lineHeight: 1 }}>⋯</span>
      </div>
      {card.history && (
        <ol
          style={{
            listStyle: "none",
            marginTop: 5,
            paddingTop: 4,
            borderTop: "1px solid var(--surface)",
            fontSize: 11,
            lineHeight: 1.5,
          }}
        >
          {card.history.map((h) => (
            <li
              key={h.what}
              style={{ color: h.muted ? "var(--muted)" : "var(--subtext)", overflowWrap: "anywhere" }}
            >
              <span style={{ display: "inline-block", minWidth: "3.2em", marginRight: 6, color: "var(--muted)" }}>
                {h.when}
              </span>
              {h.what}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

/** The pane's header, as the app draws it, on one of its three views. */
function Chrome({ view }: { view: number }) {
  return (
    <div
      className="tasks-chrome"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "4px 6px",
        borderBottom: "1px solid var(--surface)",
        minHeight: 30,
      }}
    >
      <span
        style={{
          flexShrink: 0,
          background: "var(--accent)",
          color: "var(--bg)",
          fontSize: 10,
          fontWeight: 700,
          letterSpacing: ".04em",
          textTransform: "uppercase",
          padding: "2px 6px",
          borderRadius: 4,
        }}
      >
        Tasks
      </span>
      <span style={{ fontSize: 12, fontWeight: 600, marginRight: "auto" }}>omakase</span>
      <Switch labels={["Cards", "History", "Trail"]} on={view} />
      <Switch labels={["This team", "All agents"]} on={0} />
      <span style={{ fontSize: 14, color: "var(--subtext)" }}>✕</span>
    </div>
  );
}

function TaskBoardMockup() {
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "44px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg-alt)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
      }}
    >
      <Chrome view={0} />
      <div
        className="tasks-columns"
        style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 6, padding: 6, minHeight: 330 }}
      >
        {COLUMNS.map((col) => (
          <section
            key={col.label}
            style={{
              display: "flex",
              flexDirection: "column",
              minWidth: 0,
              border: "1px solid var(--surface)",
              borderRadius: 5,
              background: "var(--bg)",
            }}
          >
            <h3
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "4px 8px",
                fontSize: 11,
                fontWeight: 600,
                color: col.color,
                borderBottom: "1px solid var(--surface)",
              }}
            >
              {col.label}
              <span style={{ color: "var(--muted)", fontWeight: 400 }}>{col.cards.length}</span>
            </h3>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: 4 }}>
              {col.cards.map((c) => (
                <TaskCard key={c.title} card={c} color={col.color} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}

/** The Trail view's marks, and the app's colors for them. */
const MARKS = {
  ok: { text: "✓", color: "var(--green)" },
  error: { text: "✗", color: "var(--red)" },
  running: { text: "…", color: "var(--accent)" },
} as const;

type Action = {
  status: keyof typeof MARKS;
  tool: string;
  summary: string;
  exit?: number;
  took?: string;
  /** Its input and output, when it is open. */
  detail?: { input: string; output: string };
};

const REFRESH = "/Users/me/api/src/auth/refresh.rs";
const FROM_PLANNER = "[message from planner, via mira agent send]";

const FAILED_TEST = `Exit code 101
   Compiling api v0.4.0 (/Users/me/api)
    Finished \`test\` profile [unoptimized + debuginfo] target(s) in 4.12s
     Running unittests src/main.rs (target/debug/deps/api-3f9c2a1e)
test auth::refresh::expires_after_30_days ... FAILED

---- auth::refresh::expires_after_30_days stdout ----
thread 'auth::refresh::expires_after_30_days' panicked at src/auth/refresh.rs:212:9:
assertion \`left == right\` failed
  left: Ok(Refreshed)
 right: Err(Expired)

test result: FAILED. 43 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass \`-p api --bin api\``;

/** The worker's turns, newest first. */
const TURNS: { when: string; actions: Action[] }[] = [
  {
    when: "1m",
    actions: [
      { status: "ok", tool: "Read", summary: REFRESH, took: "9ms" },
      { status: "ok", tool: "Grep", summary: "issued_at", took: "41ms" },
      { status: "ok", tool: "Edit", summary: REFRESH, took: "22ms" },
      {
        status: "error",
        tool: "Bash",
        summary: "cargo test -p api",
        exit: 101,
        took: "9.8s",
        detail: {
          input: `{\n  "command": "cargo test -p api",\n  "description": "Run the api tests"\n}`,
          output: FAILED_TEST,
        },
      },
      { status: "ok", tool: "Edit", summary: REFRESH, took: "18ms" },
      { status: "ok", tool: "Bash", summary: "cargo test -p api", took: "7.2s" },
      { status: "running", tool: "Bash", summary: "cargo clippy -p api -- -D warnings" },
    ],
  },
  {
    when: "6m",
    actions: [
      { status: "ok", tool: "Edit", summary: REFRESH, took: "25ms" },
      { status: "ok", tool: "Bash", summary: "cargo test -p api", took: "6.9s" },
    ],
  },
];

/** The team's agents, as the Trail view lists them, the worker picked. */
const AGENTS = [
  { name: "planner", harness: "Claude Code", dot: "#fab387" },
  { name: "worker", harness: "Claude Code", dot: "var(--accent)", on: true },
  { name: "reviewer", harness: "Codex", dot: "var(--green)" },
];

const DETAIL_HEAD = {
  fontSize: 10,
  color: "var(--muted)",
  textTransform: "uppercase" as const,
  letterSpacing: ".04em",
  marginTop: 4,
};

const DETAIL_TEXT = {
  marginTop: 2,
  maxHeight: 240,
  overflow: "auto",
  padding: "5px 7px",
  background: "var(--bg)",
  borderRadius: 4,
  color: "var(--text)",
  fontSize: 11,
  whiteSpace: "pre-wrap" as const,
  overflowWrap: "anywhere" as const,
};

function ActionLine({ action }: { action: Action }) {
  return (
    <li>
      <div style={{ display: "flex", alignItems: "baseline", gap: 6, padding: "1px 0", fontSize: 11, lineHeight: 1.5, color: "var(--subtext)" }}>
        <span style={{ flexShrink: 0, width: "1em", color: MARKS[action.status].color }}>{MARKS[action.status].text}</span>
        <span style={{ flexShrink: 0, color: "var(--text)" }}>{action.tool}</span>
        <span className="mono" style={{ flex: 1, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {action.summary}
        </span>
        {action.exit !== undefined && <span style={{ flexShrink: 0, color: "var(--red)" }}>exit {action.exit}</span>}
        <span style={{ flexShrink: 0, color: "var(--muted)" }}>{action.took ?? ""}</span>
      </div>
      {action.detail && (
        <div style={{ margin: "2px 0 6px 1.6em" }}>
          <div style={DETAIL_HEAD}>input</div>
          <pre className="mono" style={DETAIL_TEXT}>{action.detail.input}</pre>
          <div style={DETAIL_HEAD}>output (error)</div>
          <pre className="mono" style={DETAIL_TEXT}>{action.detail.output}</pre>
        </div>
      )}
    </li>
  );
}

function TrailMockup() {
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "18px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg-alt)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
      }}
    >
      <Chrome view={2} />
      <div className="tasks-trail" style={{ display: "grid", gridTemplateColumns: "minmax(110px, 180px) minmax(0, 1fr)" }}>
        <ul className="tasks-agents" style={{ listStyle: "none", padding: 4, borderRight: "1px solid var(--surface)" }}>
          {AGENTS.map((a) => (
            <li
              key={a.name}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "4px 6px",
                borderRadius: 4,
                background: a.on ? "var(--surface)" : "none",
                color: a.on ? "var(--text)" : "var(--subtext)",
                fontSize: 12,
                whiteSpace: "nowrap",
              }}
            >
              <span style={{ flexShrink: 0, width: 7, height: 7, borderRadius: "50%", background: a.dot }} />
              <span>{a.name}</span>
              <span className="tasks-agent-meta" style={{ flexShrink: 0, marginLeft: "auto", fontSize: 10, color: "var(--muted)" }}>
                {a.harness}
              </span>
            </li>
          ))}
        </ul>
        <ol style={{ listStyle: "none", padding: "6px 10px" }}>
          {TURNS.map((turn) => (
            <li key={turn.when} style={{ marginBottom: 10 }}>
              {/* Its prompt is more than its first line, so it opens. */}
              <details style={{ fontSize: 12, lineHeight: 1.5, overflowWrap: "anywhere" }}>
                <summary>
                  <span style={{ display: "inline-block", minWidth: "3.2em", marginRight: 6, color: "var(--muted)" }}>
                    {turn.when}
                  </span>
                  <span style={{ color: "var(--text)" }}>{FROM_PLANNER}</span>
                </summary>
              </details>
              <ol style={{ listStyle: "none", marginLeft: "3.8em" }}>
                {turn.actions.map((a, i) => (
                  <ActionLine key={i} action={a} />
                ))}
              </ol>
            </li>
          ))}
        </ol>
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
          The task board is the agent wall’s other side — one switch, Agents | Tasks —
          and shows what each agent was given, for one team or every tab.
          Mirador adds a card each time it hands an agent a task, and moves it as the
          agent’s turns end — Doing, Needs you, Done — or to Done once the agent is
          given a newer one. Agents write cards of their own
          with <span className="mono" style={{ color: "var(--text)" }}>mira task add</span> —
          the omakase planner puts up its steps that way and moves them as they go.
          Open a card for its history: who sent what, how each turn ended, and how long
          it took. The Trail shows what each agent actually did, turn by turn: its
          prompt, then every command, edit, read and search with how it ended, each
          opening to its full input and output — read from Claude Code’s and Codex’s
          own session files, never copied.
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
            or the agent wall’s Agents | Tasks switch, or the palette’s Agents: Task Board
          </span>
        </div>
      </div>
      <TaskBoardMockup />
      <TrailMockup />
    </section>
  );
}
