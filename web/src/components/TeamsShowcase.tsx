/**
 * Agent teams, as a static mock: the built-in `omakase` team's tab,
 * mid-handoff, shown as its wall in Focus, beside its team file.
 *
 * Kept honest to the app: the sidebar is the app's (MockAgents.tsx), the
 * team's ⚑ tab current with its members nested under it, each with its
 * status mark, a grey harness mark and how long in its status. The wall
 * draws the lead large with its status bar and the others in the rail, as
 * FocusLayout does; the Codex reviewer reports no status or context, so
 * its mark is the muted ring and its bar says so. A handed message opens
 * with the line `mira agent send` really prefixes. The team file is the
 * built-in one (cmux-core/builtin/teams/omakase.md) word for word, except
 * that the charter is one line, so it reflows at phone width.
 */

import { type Line } from "./TermLines";
import { FocusWall, MockSidebar, MockTabBar, type MockAgent, type MockTab } from "./MockAgents";

type Member = { name: string; harness: "claude" | "codex"; lines: Line[] };

const MESSAGE = "var(--mauve)";

export const MEMBERS: Member[] = [
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

/** The sidebar's harness marks: Claude Code's spark, Codex's prompt — grey,
    as the app draws them, so no harness's colour competes with needs you. */
export const MARK = { claude: { text: "✳", color: "var(--subtext)" }, codex: { text: ">_", color: "var(--subtext)" } };

/** The team's members as the wall and the sidebar show them: the planner
    working, the worker idle after its step, the Codex reviewer with no
    status to report. */
export const TEAM_AGENTS: MockAgent[] = [
  { ...MEMBERS[0], status: "working", since: "38s", model: "Opus 5.5", context: 34 },
  { ...MEMBERS[1], status: "idle", since: "2m", model: "Opus 5.5", context: 61 },
  { ...MEMBERS[2], status: null, model: "gpt-6-luna" },
];

/** The api tab's agent, waiting on a permission. */
export const API_AGENT: MockAgent = {
  name: "claude",
  harness: "claude",
  status: "needsYou",
  since: "2m",
  asking: "Bash npm run migrate",
  model: "Sonnet 5.5",
  effort: "high",
  context: 47,
  lines: [
    { text: "> wire up the refresh flow", color: "var(--text)" },
    { text: "⏺ Read(src/auth/refresh.ts)" },
    { text: "⏺ Update(src/auth/refresh.ts)" },
    { text: "  ⎿ Added 18 lines, removed 3", color: "var(--muted)" },
    { text: "⏺ Bash(npm run migrate)" },
    { text: "Do you want to proceed?", color: "var(--text)" },
    { text: "❯ 1. Yes", color: "var(--accent)" },
    { text: "  2. No, and tell Claude what to do differently" },
  ],
};

/** The sidebar's tabs, the hero's three. */
const TABS: MockTab[] = [
  { title: "api", agents: [API_AGENT] },
  { title: "omakase", team: true, agents: TEAM_AGENTS, branch: "feat/oauth" },
  { title: "web", agents: [] },
];

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
        height: 360,
      }}
    >
      <MockSidebar tabs={TABS} current={1} view="wall" className="teams-sidebar" />
      <div className="ms-content">
        <MockTabBar title="omakase" branch="feat/oauth" />
        <FocusWall agents={TEAM_AGENTS} large={0} />
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
          back the members that stopped, add one. In the editor a team is a page —
          its members as cards, the selected one’s role and settings beside them, and
          what it is told, layer by layer, down to the part Mirador writes. Before
          you start it, the page, or{" "}
          <span className="mono" style={{ color: "var(--text)" }}>mira team cost</span>,
          estimates what its prompts cost to start and each turn, model by model, ranks
          the parts that weigh most — a way every member is told counts once per
          member — and warns when the start is over your limit. One team
          ships with Mirador:{" "}
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
            · mira team list · restart · add · cost
          </span>
        </div>
      </div>
      <TeamMockup />
      <TeamFile />
    </section>
  );
}
