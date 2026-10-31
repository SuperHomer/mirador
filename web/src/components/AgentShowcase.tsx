/**
 * The agent wall, as a static mock: a tab of four agents and two tools,
 * shown as its wall in Grid, beside the sidebar.
 *
 * Kept honest to the app, like the other mocks: the sidebar and the wall
 * are MockAgents.tsx's, drawn from the app's own components. The tab's
 * agents are nested under it, then its tools; on the wall the agents come
 * in team order, then the tools, on one page of six. A Claude Code agent
 * says how it stands and what it asks or works on; the Codex one reports
 * no status or context, so its mark is the muted ring and its tile says
 * nothing it could not know. Its terminals' lines are what each program
 * prints, shortened.
 */

import { BothKeys } from "./Keys";
import { GridWall, MockSidebar, type MockAgent, type MockTab, type MockTool } from "./MockAgents";

/** A shop's checkout, in a tab of its own: one agent waiting on a
    permission, one working, a Codex one, one idle after its task. */
const AGENTS: MockAgent[] = [
  {
    name: "webhooks",
    harness: "claude",
    status: "needsYou",
    since: "1m",
    asking: "Bash stripe trigger checkout.session.completed",
    model: "Sonnet 5.5",
    context: 44,
    subagents: 0,
    lines: [
      { text: "> handle checkout.session.completed", color: "var(--text)" },
      { text: "⏺ Update(src/webhooks/stripe.ts)" },
      { text: "  ⎿ Added 24 lines, removed 2", color: "var(--muted)" },
      { text: "⏺ Bash(stripe trigger checkout.session.completed)" },
      { text: "Do you want to proceed?", color: "var(--text)" },
      { text: "❯ 1. Yes", color: "var(--accent)" },
      { text: "  2. No, and tell Claude what to do differently" },
    ],
  },
  {
    name: "tests",
    harness: "claude",
    status: "working",
    since: "3m",
    task: "Cover refunds in the webhook tests",
    model: "Opus 5.5",
    context: 31,
    subagents: 2,
    lines: [
      { text: "⏺ Read(tests/webhooks.test.ts)" },
      { text: "⏺ Task(Find every refund path)" },
      { text: "  ⎿ Done (6 tool uses · 41s)", color: "var(--muted)" },
      { text: "⏺ Update(tests/webhooks.test.ts)" },
      { text: "✶ Writing refund cases…", color: "var(--accent)" },
    ],
  },
  {
    name: "codex",
    harness: "codex",
    status: null,
    model: "gpt-6-luna",
    lines: [
      { text: "› store order amounts in cents", color: "var(--text)" },
      { text: "• Ran rg -n amount src/db" },
      { text: "• Edited src/db/migrations/0012_cents.sql (+18 -0)" },
      { text: "• Running npm run db:check" },
    ],
  },
  {
    name: "docs",
    harness: "claude",
    status: "idle",
    since: "6m",
    task: "Document the webhook secret",
    model: "Haiku 5.5",
    context: 18,
    subagents: 0,
    lines: [
      { text: "⏺ Update(README.md)" },
      { text: "  ⎿ Added 12 lines", color: "var(--muted)" },
      { text: "⏺ STRIPE_WEBHOOK_SECRET is in the README's setup now.", color: "var(--text)" },
    ],
  },
];

/** The tab's other panes: the dev server and the page it serves. */
const TOOLS: MockTool[] = [
  {
    title: "npm run dev",
    kind: "terminal",
    lines: [
      { text: "> shop@0.4.0 dev" },
      { text: "> vite --port 3000" },
      { text: "  VITE v7.1.4  ready in 412 ms", color: "var(--green)" },
      { text: "  ➜  Local:   http://localhost:3000/", color: "var(--text)" },
      { text: "12:41:07 [vite] hmr update /src/Checkout.tsx", color: "var(--muted)" },
    ],
  },
  {
    title: "http://localhost:3000/checkout",
    kind: "browser",
    lines: [
      { text: "‹ ›  ⟳  localhost:3000/checkout", color: "var(--muted)" },
      { text: "Checkout", color: "var(--text)" },
      { text: "1 × Trail runner, 42    €129.00" },
      { text: "Shipping                 €4.90" },
      { text: "[ Pay €133.90 ]", color: "var(--accent)" },
    ],
  },
];

const TABS: MockTab[] = [
  { title: "shop", agents: AGENTS, tools: TOOLS, branch: "feat/stripe-webhooks", port: 3000 },
  { title: "infra", agents: [{ ...AGENTS[3], name: "claude", lines: [] }] },
];

function WallMockup() {
  return (
    <div
      style={{
        maxWidth: 1000,
        margin: "40px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
        display: "flex",
        height: 460,
      }}
    >
      <MockSidebar tabs={TABS} current={0} view="wall" className="agents-sidebar" />
      <GridWall agents={AGENTS} tools={TOOLS} />
    </div>
  );
}

export default function AgentShowcase() {
  return (
    <section
      id="agents"
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
          AGENTS
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Every agent, on one wall
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          Give each Claude Code agent a role, with its own model and its own brief, and
          watch a tab’s agents at once — Codex, OpenCode, Pi, Gemini CLI and the rest
          count too, the moment they start in a pane. The tiles are the agents’ live
          terminals, not previews, and take your keystrokes, so you answer a
          permission prompt by typing into the agent’s own terminal without leaving
          the wall; a Claude Code agent’s also says what it is doing — working, idle,
          or waiting on you, and what for. In Focus the agent that needs
          you is drawn large, the rest in a rail beside it, and the wall moves on to
          whoever needs you next unless you pin one; in Grid they come in pages of 4,
          6 or 9. The tab’s other panes come too, after the agents, as Tools —
          shells, browsers, diffs, graphs, live — each drawn large in Focus when you
          click it. Each agent’s status bar gives its model and effort, and for Claude
          Code how full its context is, what the session has cost at API list
          prices (on a subscription, what it would have cost, not what is billed)
          and a sheet of the subagents it started; what a harness does not report,
          it says so. The wall is a
          view of the tab itself, in place of its splits, and each tab remembers which
          it showed. The sidebar’s switch — Splits, Wall, Tasks, Board — picks the
          view, each tab’s agents listed under it with what they are doing, then its
          tools, each with its own icon; a click on one shows it large on its wall.
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
            mira agent wall
          </span>
          <span>or</span>
          <BothKeys mac="⌘⇧A" other="Ctrl+Shift+A" />
        </div>
      </div>

      <WallMockup />

      <pre
        className="mono"
        style={{
          maxWidth: 560,
          margin: "28px auto 0",
          padding: "16px 20px",
          background: "var(--crust)",
          border: "1px solid var(--surface)",
          borderRadius: 10,
          fontSize: 12.5,
          lineHeight: 1.7,
          color: "var(--subtext)",
          overflowX: "auto",
        }}
      >
        <span style={{ color: "var(--muted)" }}>{"// ~/.config/mirador/mirador.json"}</span>
        {"\n"}
        {`"agentRoles": [\n`}
        {`  { "name": "planner",  "model": "opus" },\n`}
        {`  { "name": "reviewer", "model": "haiku",\n`}
        {`    "prompt": "Review the diff; be terse." }\n`}
        {`]`}
      </pre>
    </section>
  );
}
