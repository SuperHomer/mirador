type Feature = { icon: string; title: string; body: string; code?: string };

const FEATURES: Feature[] = [
  { icon: "⧉", title: "Tabs & splits", body: "Horizontal and vertical splits, WebGL rendering with fallback, and flow-controlled PTY streaming — a runaway <cat> can’t freeze the UI." },
  { icon: "◎", title: "Agent notifications", body: "Panes get an attention ring and tabs light up on OSC 9/99/777 or <mira notify>. Native alerts when the window is unfocused." },
  { icon: "▦", title: "Agent wall", body: "A tab’s Claude Code agents, their live terminals in one grid, each tagged with its role, its model, and whether it is working, idle or waiting on you — answer a prompt right there. Roles start Claude with their own model and prompt: <mira agent new --role reviewer>." },
  { icon: "▶", title: "Command panes", body: "Agent-launched commands run in a visible, interruptible pane. <--wait> returns clean output and an exit code to the caller." },
  { icon: "±", title: "Diff panes", body: "Review what changed without leaving the terminal: file tree, hunks, and your own theme. <mira diff> for uncommitted work, a commit, or a branch, in any git worktree — untracked files included." },
  { icon: "⑂", title: "Commit graph", body: "The branch structure across every ref, with lanes, tags and remotes. Click a commit and its diff opens beside it — <mira graph>, or the palette." },
  { icon: "◱", title: "Scriptable browser", body: "Agents open pages, snapshot the DOM, click, fill, and eval — while you watch it happen in a real pane via <mira browser>." },
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
