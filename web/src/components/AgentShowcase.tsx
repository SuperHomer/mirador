/**
 * The agent wall, as a real screenshot rather than a mock: what it shows
 * is other programs' terminals, which no mock would draw faithfully.
 */

import wallUrl from "../assets/agent-wall.webp";
import { BothKeys } from "./Keys";

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

      <img
        src={wallUrl}
        alt="The agent wall: four Claude Code sessions working on this repository as a team — an orchestrator directing dev-1 and dev-2 on Sonnet and a planner on Opus — each tile headed by its name, model, branch and status, the planner's two finished subagents listed beside its terminal, and the agents named at the foot of the sidebar."
        width={1558}
        height={1047}
        loading="lazy"
        style={{
          display: "block",
          width: "100%",
          maxWidth: 1000,
          height: "auto",
          margin: "40px auto 0",
        }}
      />

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
