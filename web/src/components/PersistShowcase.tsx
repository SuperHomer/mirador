/**
 * Session persistence, as a three-step timeline: quit with work in
 * flight, the work carrying on while Mirador is closed, and the next
 * launch picking up the same processes.
 *
 * Kept honest to the app: the notification title is the one the app
 * raises for what arrived while it was closed, and the ways to end
 * sessions are the real palette entry and command.
 */

function Key({ children }: { children: string }) {
  return (
    <span
      className="mono"
      style={{
        fontSize: 12,
        color: "var(--text)",
        background: "var(--surface)",
        border: "1px solid var(--overlay)",
        borderRadius: 6,
        padding: "2px 8px",
      }}
    >
      {children}
    </span>
  );
}

type Line = { text: string; color?: string };

function Pane({ title, lines, dim }: { title: string; lines: Line[]; dim?: boolean }) {
  return (
    <div
      style={{
        background: "var(--bg)",
        border: "1px solid var(--surface)",
        borderRadius: 10,
        overflow: "hidden",
        opacity: dim ? 0.55 : 1,
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
        {title}
      </div>
      <div className="mono" style={{ fontSize: 12.5, lineHeight: 1.85, padding: "10px 12px" }}>
        {lines.map((l, i) => (
          <div key={i} style={{ color: l.color ?? "var(--text)", whiteSpace: "pre" }}>
            {l.text}
          </div>
        ))}
      </div>
    </div>
  );
}

type Step = { when: string; label: string; panes: { title: string; lines: Line[] }[]; dim?: boolean; toast?: boolean };

const STEPS: Step[] = [
  {
    when: "14:02",
    label: "⌘Q, with work in flight",
    panes: [
      { title: "npm run build", lines: [{ text: "$ npm run build" }, { text: "  bundling…  41%", color: "var(--yellow)" }] },
      { title: "claude", lines: [{ text: "❯ add refresh-token rotation" }, { text: "✻ Thinking…", color: "var(--mauve)" }] },
    ],
  },
  {
    when: "while closed",
    label: "Mirador is gone. The work isn't.",
    dim: true,
    panes: [
      { title: "npm run build", lines: [{ text: "  bundling…  87%", color: "var(--yellow)" }, { text: "  still running" , color: "var(--muted)"}] },
      { title: "claude", lines: [{ text: "● Edit src/auth/token.ts", color: "var(--sapphire)" }, { text: "  still working", color: "var(--muted)" }] },
    ],
  },
  {
    when: "14:20",
    label: "Relaunch: the same processes",
    toast: true,
    panes: [
      { title: "npm run build", lines: [{ text: "  bundling… 100%", color: "var(--yellow)" }, { text: "✓ built in 3m 12s", color: "var(--green)" }] },
      { title: "claude", lines: [{ text: "● Done — 4 files changed", color: "var(--green)" }, { text: "❯ ▌" }] },
    ],
  },
];

function Timeline() {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: 18,
        maxWidth: 1100,
        margin: "40px auto 0",
      }}
    >
      {STEPS.map((step) => (
        <div key={step.when} style={{ flex: "1 1 280px", minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 10 }}>
            <span className="mono" style={{ fontSize: 12, color: "var(--accent)" }}>
              {step.when}
            </span>
            <span style={{ fontSize: 13.5, color: "var(--subtext)" }}>{step.label}</span>
          </div>
          <div style={{ display: "grid", gap: 10 }}>
            {step.panes.map((p) => (
              <Pane key={p.title} title={p.title} lines={p.lines} dim={step.dim} />
            ))}
            {step.toast && (
              <div
                style={{
                  background: "var(--surface)",
                  border: "1px solid var(--overlay)",
                  borderLeft: "3px solid var(--accent)",
                  borderRadius: 8,
                  padding: "8px 12px",
                  fontSize: 12.5,
                  lineHeight: 1.5,
                }}
              >
                <div style={{ color: "var(--text)", fontWeight: 600 }}>While Mirador was closed</div>
                <div style={{ color: "var(--subtext)" }}>Claude Code: finished responding</div>
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

export default function PersistShowcase() {
  return (
    <section
      id="sessions"
      style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)", background: "var(--crust)" }}
    >
      <div style={{ textAlign: "center", maxWidth: 720, margin: "0 auto" }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>
          SESSIONS
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Quit. Your terminals keep working.
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          Shells, builds, dev servers and agents keep running when Mirador quits, and the next
          launch reattaches to the same processes — with what they printed while it was closed.
          Notifications that arrived meanwhile come back as one. A crash loses nothing: a session
          with no pane to return to reopens in a tab of its own.
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
          <span>To end them all:</span>
          <Key>⌘K</Key>
          <span style={{ color: "var(--muted)", fontSize: 12.5 }}>→ “Quit and End All Sessions”</span>
          <span>or</span>
          <span className="mono" style={{ color: "var(--text)" }}>
            mira quit --end-sessions
          </span>
        </div>
      </div>
      <Timeline />
    </section>
  );
}
