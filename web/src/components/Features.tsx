type Feature = { icon: string; title: string; body: string; code?: string };

const FEATURES: Feature[] = [
  { icon: "⧉", title: "Tabs & splits", body: "Horizontal and vertical splits, WebGL rendering with fallback, and flow-controlled PTY streaming — a runaway <cat> can’t freeze the UI." },
  { icon: "⇣", title: "Drop a file in", body: "Drag files onto a pane and their paths are typed in — onto an agent’s tile on the wall, and they go to that agent." },
  { icon: "◎", title: "Inbox & notifications", body: "Panes get an attention ring and tabs light up on OSC 9/99/777 or <mira notify>, and an agent’s alerts say which agent, in which tab. The Inbox puts every Claude Code agent waiting on you first, across every tab, with what each is asking and Open to go to it — then what happened while you looked away: turns ended, cards moved, notifications. Native alerts when the window is unfocused." },
  { icon: "◔", title: "Agent status bar", body: "Every agent’s pane has a status bar: its harness, model and effort, and for Claude Code how full its context is and its subagents — open the sheet for each one with its task list done as a percentage, its model, tokens and run time, running ones first, read from its own transcripts, or with <mira agent activity>. What a harness does not report, it says so." },
  { icon: "▦", title: "Agent wall", body: "Switch a tab from its splits to its agents — Claude Code, Codex, OpenCode, Pi, Gemini CLI, Aider and more, found with no setup — their live terminals on one wall. In Focus, whoever needs you is large and the rest wait in a rail, the wall following the next one that needs you unless you pin one; in Grid, they come in pages of 4, 6 or 9, in team order. A Claude Code agent’s tile says whether it is working, idle or waiting on you, and what it waits on; the tile is its real terminal, so you type into it to answer. The sidebar’s Splits / Wall / Tasks / Board switch, or <mira agent wall --focus> and <--grid>; each tab remembers its view across restarts. Roles — Markdown files committed in <.mirador/roles> — start Claude Code or Codex with their own model, prompt and shared way of working — and only the MCP servers, plugins and skills each one needs: <mira agent new --role reviewer>." },
  { icon: "⊞", title: "Board", body: "Every tab as a frame and every pane as a card, on one canvas you pan and pinch — and a tab showing its wall shows it in its frame. Each frame’s title carries its agents’ status and its view, a frame where someone waits on you has a peach edge, and the corner says how many wait and flies you to each in turn. Zoom out to see all your agents at once; double-click a card’s header to work in it at 100%. <mira view board>." },
  { icon: "⚑", title: "Agent teams", body: "Write a team once — a lead, two devs, a reviewer, a charter and a way of working — in <.mirador/teams>, or on its page in the editor: its members as cards, each one’s role a click away, and what each member is told, layer by layer. Start it in a tab of its own: each agent knows its teammates by name and hands them work with <mira agent send> — <--wait> blocks until a Claude Code teammate is done; a Codex one reports back with its own <mira agent send>. One ships with Mirador — <omakase>: an Opus planner and worker and a Codex reviewer. <mira team list> shows who is running, <mira team restart> brings back the ones that stopped, <mira team add> grows it." },
  { icon: "▤", title: "Task board", body: "The wall’s Tasks side: what each agent was given and where it stands — Needs you, To do, Doing, Done, or as swimlanes, a row per agent — for one team or every tab. Mirador adds a card each time it hands an agent a task, and moves Claude Code’s and Codex’s as their turns end; write your own with a title and a description, assign it, or send an idle agent its next one. Agents add theirs with <mira task add>. History lists who sent what and how each turn ended, by day; Trail shows what each agent actually did — every command and how it ended, every file it edited, each opening to its full input and output — read from the agent’s own session files, never copied. <mira agent board>, or <mira agent history> and <mira agent trail>." },
  { icon: "▶", title: "Command panes", body: "Agent-launched commands run in a visible, interruptible pane. <--wait> returns clean output and an exit code to the caller." },
  { icon: "±", title: "Diff panes", body: "Review what changed without leaving the terminal: file tree, hunks, and your own theme. <mira diff> for uncommitted work, a commit, or a branch, in any git worktree — untracked files included." },
  { icon: "⑂", title: "Commit graph", body: "The branch structure across every ref, with lanes, tags and remotes. Click a commit and its diff opens beside it — <mira graph>, or the palette." },
  { icon: "◱", title: "Scriptable browser", body: "An agent tests the page it is building, the way you would: it opens it, clicks, types and presses keys, waits for what should appear, and reads the console’s errors — while you watch in a real pane. <mira browser wait>, <mira browser console --errors>." },
  { icon: "⇄", title: "Remote workspaces", body: "Panes run the system <ssh> — 2FA and ProxyJump just work. ControlMaster port forwarding brings remote dev servers to your browser pane." },
  { icon: "⟳", title: "Sessions that outlive the app", body: "Quit and your shells, builds and agents keep running; the next launch reattaches to the same processes. Layout and scrollback survive a reboot. A Settings pane shows and edits every <mirador.json> setting (palette: App: Settings, or <mira settings>), and the file still hot-reloads." },
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
