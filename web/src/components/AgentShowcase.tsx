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
          watch a tab’s agents at once. The tiles are the agents’ live terminals, not
          previews: each says what its agent is doing (working, idle, or waiting on
          you) and takes your keystrokes, so you answer a permission prompt without
          leaving the wall. In the sidebar, each tab lists its agents below the tabs,
          and a click shows one alone.
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
        alt="The agent wall: four Claude Code agents in a grid — a planner on opus and a reviewer on sonnet still working, two more idle — each tile headed by its role, model and status, with the agents listed apart at the foot of the sidebar."
        width={2000}
        height={1189}
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
