/**
 * Agent teams, as a static mock: the built-in `omakase` team on the agent
 * wall, mid-handoff, beside its team file.
 *
 * Kept honest to the app: the tiles are headed the way the wall heads them
 * (status dot, member name, role, model, status — "—" for Codex, which has
 * no hooks), three tiles make the wall's two columns, a handed message
 * opens with the line `mira agent send` really prefixes, and the team file
 * is the built-in one (cmux-core/builtin/teams/omakase.md), word for word.
 */

type Status = "working" | "needsYou" | "idle" | null;
type Line = { text: string; color?: string };
type Tile = { member: string; role: string; model: string; status: Status; lines: Line[] };

const STATUS: Record<NonNullable<Status>, { label: string; color: string }> = {
  working: { label: "working", color: "var(--accent)" },
  needsYou: { label: "needs you", color: "#fab387" },
  idle: { label: "idle", color: "var(--green)" },
};

const MESSAGE = "var(--mauve)";

const TILES: Tile[] = [
  {
    member: "planner",
    role: "omakase-planner",
    model: "claude-opus-5-5",
    status: "working",
    lines: [
      { text: "⏺ Bash(mira agent send worker \"Step 2: rotate the refresh token on use\" --wait)" },
      { text: "  ⎿ worker is idle", color: "var(--muted)" },
      { text: "⏺ Bash(mira agent send reviewer \"Step 2 is done: review the diff\")" },
      { text: "> [message from reviewer, via mira agent send]", color: MESSAGE },
      { text: "  APPROVE — rotation is atomic and the test pins reuse.", color: "var(--text)" },
      { text: "✶ Planning step 3…", color: "var(--accent)" },
    ],
  },
  {
    member: "worker",
    role: "omakase-worker",
    model: "claude-opus-5-5",
    status: "idle",
    lines: [
      { text: "> [message from planner, via mira agent send]", color: MESSAGE },
      { text: "  Step 2: rotate the refresh token on use", color: "var(--text)" },
      { text: "⏺ Update(src/auth/refresh.rs)" },
      { text: "⏺ Bash(cargo test -p api)" },
      { text: "  ⎿ test result: ok. 43 passed", color: "var(--green)" },
      { text: "⏺ Step 2 done: refresh.rs, one new test.", color: "var(--text)" },
    ],
  },
  {
    member: "reviewer",
    role: "omakase-reviewer",
    model: "gpt-6-luna",
    status: null,
    lines: [
      { text: "› [message from planner, via mira agent send]", color: MESSAGE },
      { text: "  Step 2 is done: review the diff", color: "var(--text)" },
      { text: "• Ran git diff" },
      { text: "• Ran mira agent send planner \"APPROVE — …\"" },
    ],
  },
];

/** The built-in team's file, as it ships. */
const TEAM_FILE = `---
description: A planner, a worker and a Codex reviewer, working the Rails doctrine's way
way: omakase
layout: columns
members:
  - role: omakase-planner
    name: planner
    lead: true
  - role: omakase-worker
    name: worker
  - role: omakase-reviewer
    name: reviewer
---
The planner owns the task: it splits it into small steps and hands them to
the worker one at a time, waiting for each. When a step is done, the
planner asks the reviewer, who reads the diff and answers the planner with
APPROVE or CHANGES. The planner decides what happens next, and tells the
person when the task is done. Nobody pushes or merges: a person does.`;

function TileView({ tile }: { tile: Tile }) {
  const status = tile.status ? STATUS[tile.status] : null;
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        border: "1px solid var(--surface)",
        borderRadius: 5,
        overflow: "hidden",
        background: "var(--bg)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "3px 8px",
          background: "var(--bg-alt)",
          borderBottom: "1px solid var(--surface)",
          fontSize: 11,
          whiteSpace: "nowrap",
          overflow: "hidden",
        }}
      >
        <span
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            flexShrink: 0,
            background: status?.color ?? "var(--muted)",
          }}
        />
        <span style={{ color: "var(--text)", fontWeight: 600 }}>{tile.member}</span>
        {[tile.role, tile.model].map((t) => (
          <span
            key={t}
            style={{ color: "var(--subtext)", background: "var(--surface)", borderRadius: 4, padding: "0 5px" }}
          >
            {t}
          </span>
        ))}
        <span style={{ marginLeft: "auto", color: status?.color ?? "var(--muted)" }}>
          {status?.label ?? "—"}
        </span>
      </div>
      <div className="mono" style={{ fontSize: 11.5, lineHeight: 1.7, padding: "8px 10px", minHeight: 150 }}>
        {tile.lines.map((l, i) => (
          <div
            key={i}
            style={{
              color: l.color ?? "var(--subtext)",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {l.text}
          </div>
        ))}
      </div>
    </div>
  );
}

function TeamMockup() {
  const count = (s: Status) => TILES.filter((t) => t.status === s).length;
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "44px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
      }}
    >
      {/* the wall's chrome: which tab, and how its agents are doing */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "4px 8px",
          minHeight: 30,
          background: "var(--bg-alt)",
          borderBottom: "1px solid var(--surface)",
          fontSize: 11,
          whiteSpace: "nowrap",
        }}
      >
        <span
          style={{
            background: "var(--accent)",
            color: "var(--bg)",
            fontSize: 10,
            fontWeight: 700,
            letterSpacing: ".04em",
            padding: "2px 6px",
            borderRadius: 4,
          }}
        >
          AGENTS
        </span>
        <span style={{ color: "var(--text)", fontWeight: 600, fontSize: 12 }}>omakase</span>
        <span style={{ color: "var(--accent)" }}>{count("working")} working</span>
        <span style={{ color: "var(--muted)" }}>{count("idle")} idle</span>
      </div>
      <div className="teams-wall" style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 6, padding: 6 }}>
        {TILES.map((t) => (
          <TileView key={t.member} tile={t} />
        ))}
      </div>
    </div>
  );
}

function TeamFile() {
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "18px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--crust)",
        textAlign: "left",
      }}
    >
      <div
        className="mono"
        style={{
          fontSize: 11,
          color: "var(--muted)",
          padding: "6px 12px",
          borderBottom: "1px solid var(--surface)",
          background: "var(--bg-alt)",
        }}
      >
        teams/omakase.md · built in
      </div>
      <pre
        className="mono"
        style={{
          fontSize: 12,
          lineHeight: 1.65,
          padding: "12px 14px",
          color: "var(--subtext)",
          whiteSpace: "pre-wrap",
          overflowWrap: "anywhere",
        }}
      >
        {TEAM_FILE}
      </pre>
    </div>
  );
}

export default function TeamsShowcase() {
  return (
    <section
      id="teams"
      style={{
        padding: "66px 48px",
        borderTop: "1px solid var(--surface)",
        background: "var(--crust)",
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 720, margin: "0 auto" }}>
        <span
          className="mono"
          style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}
        >
          TEAMS
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Agents that hand each other work
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          Write a team once in <span className="mono" style={{ color: "var(--text)" }}>.mirador/teams</span>:
          its members and their roles, a lead, a charter and a way of working. Start it
          and it opens in a tab of its own, the task going to the lead. Each member knows
          its teammates by name and hands them work with{" "}
          <span className="mono" style={{ color: "var(--text)" }}>mira agent send</span> —{" "}
          <span className="mono" style={{ color: "var(--text)" }}>--wait</span> blocks until
          they are done. Mirador keeps track of the team across restarts: list it, bring
          back the members that stopped, add one. One team ships with Mirador:{" "}
          <span className="mono" style={{ color: "var(--text)" }}>omakase</span>, a planner,
          a worker and a reviewer.
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
            mira team start omakase "add OAuth"
          </span>
          <span className="mono" style={{ color: "var(--muted)", fontSize: 12.5 }}>
            · mira team list · restart · add
          </span>
        </div>
      </div>
      <TeamMockup />
      <TeamFile />
    </section>
  );
}
