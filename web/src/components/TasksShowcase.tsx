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
        <Switch labels={["Cards", "History", "Trail"]} on={0} />
        <Switch labels={["This team", "All agents"]} on={0} />
        <span style={{ fontSize: 14, color: "var(--subtext)" }}>✕</span>
      </div>
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
          The task board shows what each agent was given, for one team or every tab.
          Mirador adds a card each time it hands an agent a task, and moves it as the
          agent’s turns end: Doing, Needs you, Done. Agents write cards of their own
          with <span className="mono" style={{ color: "var(--text)" }}>mira task add</span> —
          the omakase planner puts up its steps that way and moves them as they go.
          Open a card for its history: who sent what, how each turn ended, and how long
          it took.
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
            · mira task add · mira agent history
          </span>
          <span style={{ color: "var(--muted)", fontSize: 12.5 }}>
            or the palette’s Agents: Task Board, or the agent wall’s Task board button
          </span>
        </div>
      </div>
      <TaskBoardMockup />
    </section>
  );
}
