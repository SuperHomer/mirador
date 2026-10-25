type Feature = { icon: string; title: string; body: string; code?: string };

const FEATURES: Feature[] = [
  { icon: "⧉", title: "Tabs & splits", body: "Horizontal and vertical splits, WebGL rendering with fallback, and flow-controlled PTY streaming — a runaway <cat> can’t freeze the UI." },
  { icon: "◎", title: "Agent notifications", body: "Panes get an attention ring and tabs light up on OSC 9/99/777 or <mira notify>. Native alerts when the window is unfocused." },
  { icon: "◔", title: "Subagent activity", body: "Right-click an agent to see what it delegated: every subagent with its task list done as a percentage, its model, tokens and run time, running ones first, and on the agent wall under each agent’s tile — read from Claude Code’s own transcripts, or with <mira agent activity>." },
  { icon: "▦", title: "Agent wall", body: "Switch a tab from its splits to its agents — Claude Code, Codex, OpenCode, Pi, Gemini CLI, Aider and more, found with no setup — their live terminals in one grid, each tagged with its model, and Claude Code’s with its role and whether it is working, idle or waiting on you — answer a prompt right there, and switch back with the same key or <mira agent wall>. Turn it over to the tab’s task board with its Agents | Tasks switch. Each tab remembers its view across restarts. Roles — Markdown files committed in <.mirador/roles> — start Claude Code or Codex with their own model, prompt and shared way of working — and only the MCP servers, plugins and skills each one needs: <mira agent new --role reviewer>. Write them in an editor pane that checks as you type — <mira agent edit>." },
  { icon: "⊞", title: "Board", body: "Every tab as a frame and every pane as a card, on one canvas you pan and pinch. Zoom out to see all your agents at once; double-click a card’s header to work in it at 100%. <mira view board>, or <⌘⇧O>." },
  { icon: "⚑", title: "Agent teams", body: "Write a team once — a lead, two devs, a reviewer, a charter and a way of working — in <.mirador/teams> and start it in a tab of its own: each agent knows its teammates by name and hands them work with <mira agent send> — <--wait> blocks until a Claude Code teammate is done; a Codex one, which nobody can wait on, reports back with its own <mira agent send>. One ships with Mirador — <omakase>: an Opus planner and worker and a Codex reviewer, working the Rails doctrine's way. Mirador keeps track of a team across restarts — <mira team list> shows who is running, <mira team restart> brings back the ones that stopped, <mira team add> grows it." },
  { icon: "▤", title: "Task board", body: "The agent wall’s other side, a switch away — Agents | Tasks: what each agent was given and where it stands — To do, Doing, Needs you, Done — for one team or every tab. Mirador adds a card each time it hands an agent a task, and moves Claude Code’s and Codex’s as their turns end; agents write their own with <mira task add>, and the built-in planner keeps its steps there. Open a card for its history: who sent what, how each turn ended and how long it took. Its Trail shows what each agent actually did — every command and how it ended, every file it edited, each opening to its full input and output — read from the agent’s own session files, never copied. <mira agent board>, or <mira agent history> and <mira agent trail> in the terminal." },
  { icon: "▶", title: "Command panes", body: "Agent-launched commands run in a visible, interruptible pane. <--wait> returns clean output and an exit code to the caller." },
  { icon: "±", title: "Diff panes", body: "Review what changed without leaving the terminal: file tree, hunks, and your own theme. <mira diff> for uncommitted work, a commit, or a branch, in any git worktree — untracked files included." },
  { icon: "⑂", title: "Commit graph", body: "The branch structure across every ref, with lanes, tags and remotes. Click a commit and its diff opens beside it — <mira graph>, or the palette." },
  { icon: "◱", title: "Scriptable browser", body: "An agent tests the page it is building, the way you would: it opens it, clicks, types and presses keys, waits for what should appear, and reads the console’s errors — while you watch in a real pane. <mira browser wait>, <mira browser console --errors>." },
  { icon: "⇄", title: "Remote workspaces", body: "Panes run the system <ssh> — 2FA and ProxyJump just work. ControlMaster port forwarding brings remote dev servers to your browser pane." },
  { icon: "⟳", title: "Sessions that outlive the app", body: "Quit and your shells, builds and agents keep running; the next launch reattaches to the same processes. Layout and scrollback survive a reboot. Config hot-reloads from <mirador.json>." },
];

function Body({ text }: { text: string }) {
  // segments wrapped in <...> render as inline mono/code
  const parts = text.split(/<([^>]+)>/g);
  return (
    <>
      {parts.map((p, i) =>
        i % 2 === 1 ? (
          <span key={i} className="mono" style={{ color: "var(--text)" }}>{p}</span>
        ) : (
          <span key={i}>{p}</span>
        )
      )}
    </>
  );
}

export default function Features() {
  return (
    <section id="features" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)" }}>
      <div style={{ textAlign: "center", marginBottom: 44 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>FEATURES</span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Built for watching agents work
        </h2>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))", gap: 18, maxWidth: 1100, margin: "0 auto" }}>
        {FEATURES.map((f) => (
          <div key={f.title} style={{ background: "var(--bg-alt)", border: "1px solid var(--surface)", borderRadius: 14, padding: 26 }}>
            <div style={{ width: 40, height: 40, borderRadius: 10, background: "rgba(137,180,250,.12)", display: "flex", alignItems: "center", justifyContent: "center", fontFamily: "var(--mono)", fontSize: 16, color: "var(--accent)", marginBottom: 16 }}>
              {f.icon}
            </div>
            <h3 style={{ fontSize: 17, fontWeight: 650 }}>{f.title}</h3>
            <p style={{ color: "var(--subtext)", fontSize: 13.5, lineHeight: 1.6, marginTop: 8 }}>
              <Body text={f.body} />
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}
