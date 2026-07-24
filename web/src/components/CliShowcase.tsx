import CopyButton from "./CopyButton";

const COMMANDS = [
  'mira run --wait npm test',
  'mira browser open localhost:3000',
  'mira ssh open prod-box',
  'mira notify "build green"',
  'mira hooks setup',
].join("\n");

export default function CliShowcase() {
  return (
    <section id="cli" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)", background: "var(--bg-alt)" }}>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1.2fr", gap: 44, alignItems: "center", maxWidth: 1100, margin: "0 auto" }} className="cli-grid">
        <div>
          <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>THE CLI</span>
          <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10, lineHeight: 1.1 }}>
            Everything is<br />drivable from <span className="mono" style={{ color: "var(--accent)" }}>mira</span>
          </h2>
          <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
            Named for <em>mira</em> — Spanish for "look!". A single command and Unix socket expose every
            action, so your agents can open panes, run commands, drive a browser, and ping you when they're stuck.
          </p>
          <CopyButton
            text={COMMANDS}
            style={{
              marginTop: 22,
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              background: "var(--surface)",
              color: "var(--text)",
              border: "1px solid var(--overlay)",
              fontSize: 13,
              fontWeight: 600,
              padding: "9px 15px",
              borderRadius: 8,
            }}
          />
        </div>
        <div className="mono" style={{ background: "var(--crust)", border: "1px solid var(--surface)", borderRadius: 12, padding: "22px 24px", fontSize: 13, lineHeight: 2, boxShadow: "0 16px 40px rgba(0,0,0,.4)" }}>
          <div><span style={{ color: "var(--muted)" }}>$</span> mira run --wait npm test<span style={{ color: "var(--muted)" }}>      # observable, exit code</span></div>
          <div><span style={{ color: "var(--muted)" }}>$</span> mira browser open <span style={{ color: "var(--green)" }}>localhost:3000</span></div>
          <div><span style={{ color: "var(--muted)" }}>$</span> mira ssh open <span style={{ color: "var(--mauve)" }}>prod-box</span></div>
          <div><span style={{ color: "var(--muted)" }}>$</span> mira notify <span style={{ color: "var(--green)" }}>"build green"</span></div>
          <div><span style={{ color: "var(--muted)" }}>$</span> mira hooks setup<span style={{ color: "var(--muted)" }}>           # Claude Code</span></div>
        </div>
      </div>
    </section>
  );
}
