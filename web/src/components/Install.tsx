import CopyButton from "./CopyButton";

const SCRIPT = [
  "npm install",
  "npm run tauri dev",
  "npm run tauri build",
  "npm run build:cli",
].join("\n");

export default function Install() {
  return (
    <section id="install" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)" }}>
      <div style={{ textAlign: "center", marginBottom: 34 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>GET STARTED</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>Up and running in a minute</h2>
      </div>
      <div style={{ maxWidth: 720, margin: "0 auto", background: "var(--crust)", border: "1px solid var(--surface)", borderRadius: 12, overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 16px", borderBottom: "1px solid var(--surface)", background: "var(--bg-alt)" }}>
          <span className="mono" style={{ fontSize: 11.5, color: "var(--muted)" }}>shell &mdash; macOS, Windows or Linux</span>
          <CopyButton
            text={SCRIPT}
            idle="copy"
            done={"copied \u2713"}
            style={{ background: "none", border: "none", color: "var(--subtext)", fontFamily: "var(--mono)", fontSize: 11.5 }}
          />
        </div>
        <pre className="mono" style={{ margin: 0, padding: "20px 22px", fontSize: 13, lineHeight: 1.9, color: "var(--text)", overflowX: "auto" }}>
{``}<span style={{ color: "var(--muted)" }}># install &amp; run</span>{"\n"}
npm install{"\n"}
npm run tauri dev{"\n"}
<span style={{ color: "var(--muted)" }}># build installers &mdash; .dmg on macOS, .exe/.msi on Windows</span>{"\n"}
npm run tauri build{"\n"}
<span style={{ color: "var(--muted)" }}># the mira CLI alone (it also ships inside the app)</span>{"\n"}
npm run build:cli
        </pre>
      </div>
    </section>
  );
}
