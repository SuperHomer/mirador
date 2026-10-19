/**
 * Agent teams, as a static mock: the built-in `omakase` team's tab,
 * mid-handoff, beside its team file.
 *
 * Kept honest to the app: `layout: columns` makes the tab one row split,
 * its members' panes side by side (cmux-core's teams.rs), so the mock draws
 * three columns. A pane in a tab has no header; the sidebar names each
 * agent, with its harness's mark, under the team's ⚑ tab. A handed message
 * opens with the line `mira agent send` really prefixes. The team file is
 * the built-in one (cmux-core/builtin/teams/omakase.md) word for word,
 * except that the charter is one line, so it reflows at phone width.
 */

type Line = { text: string; color?: string };
type Member = { name: string; harness: "claude" | "codex"; lines: Line[] };

const MESSAGE = "var(--mauve)";

const MEMBERS: Member[] = [
  {
    name: "planner",
    harness: "claude",
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
    name: "worker",
    harness: "claude",
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
    name: "reviewer",
    harness: "codex",
    lines: [
      { text: "› [message from planner, via mira agent send]", color: MESSAGE },
      { text: "  Step 2 is done: review the diff", color: "var(--text)" },
      { text: "• Ran git diff" },
      { text: "• Ran mira agent send planner \"APPROVE — rotation is atomic and the test pins reuse.\"" },
    ],
  },
];

/** The built-in team's file, as it ships but for the charter's line breaks. */
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
The planner owns the task: it splits it into small steps and hands them to the worker one at a time, waiting for each. When a step is done, the planner asks the reviewer, who reads the diff and answers the planner with APPROVE or CHANGES. The planner decides what happens next, and tells the person when the task is done. Nobody pushes or merges: a person does.`;

/** The sidebar's harness marks: Claude Code's spark, Codex's prompt. */
const MARK = { claude: { text: "✳", color: "#d97757" }, codex: { text: ">_", color: "var(--subtext)" } };

function TeamMockup() {
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
        display: "flex",
        height: 330,
      }}
    >
      <div
        className="teams-sidebar"
        style={{
          width: 180,
          flexShrink: 0,
          background: "var(--bg-alt)",
          borderRight: "1px solid var(--surface)",
          padding: 6,
          fontSize: 12.5,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 8, borderRadius: 6, background: "var(--surface)" }}>
          <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>1</span>
          <span style={{ fontWeight: 600 }}>
            <span style={{ color: "var(--accent)", marginRight: 5, fontSize: 11 }}>⚑</span>api
          </span>
        </div>
        <div
          style={{
            margin: "10px 8px 4px",
            fontSize: 10,
            fontWeight: 600,
            letterSpacing: ".06em",
            textTransform: "uppercase",
            color: "var(--muted)",
          }}
        >
          Agents
        </div>
        {MEMBERS.map((m, i) => (
          <div key={m.name} style={{ display: "flex", alignItems: "center", gap: 8, padding: "5px 8px" }}>
            <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>{i + 2}</span>
            <span className="mono" style={{ width: 14, textAlign: "center", fontSize: 11, color: MARK[m.harness].color }}>
              {MARK[m.harness].text}
            </span>
            <span>{m.name}</span>
          </div>
        ))}
      </div>
      {/* the tab, as layout: columns splits it */}
      <div className="teams-panes" style={{ flex: 1, minWidth: 0, overflowX: "auto" }}>
        <div style={{ display: "flex", gap: 6, padding: 6, height: "100%", minWidth: 600 }}>
          {MEMBERS.map((m, i) => (
            <div
              key={m.name}
              className="mono"
              style={{
                flex: 1,
                minWidth: 0,
                border: `1px solid ${i === 0 ? "var(--muted)" : "var(--surface)"}`,
                borderRadius: 5,
                background: "var(--crust)",
                padding: "8px 10px",
                fontSize: 11,
                lineHeight: 1.6,
                overflow: "hidden",
              }}
            >
              {m.lines.map((l, j) => (
                <div key={j} style={{ color: l.color ?? "var(--subtext)", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
                  {l.text}
                </div>
              ))}
            </div>
          ))}
        </div>
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
          a Claude Code teammate is done; nobody can wait on a Codex agent, so a Codex
          teammate reports back with its own{" "}
          <span className="mono" style={{ color: "var(--text)" }}>mira agent send</span>.
          Mirador keeps track of the team across restarts: list it, bring
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
