import CopyButton from "./CopyButton";
import { GITHUB_URL } from "../config";

// This block used to sit under a "Get started" heading, where it sent people
// who had just downloaded a .dmg off to run a Rust toolchain. It is the
// contributor path, so it says so and sits at the bottom of the page.
const SCRIPT = ["npm install", "npm run tauri dev", "npm run tauri build", "npm run build:cli"].join("\n");

export default function Contribute() {
  return (
    <section id="contribute" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)" }}>
      <div style={{ textAlign: "center", marginBottom: 34 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>CONTRIBUTE</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>Build it from source</h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, maxWidth: 580, margin: "14px auto 0", textWrap: "pretty" }}>
          Hacking on Mirador — or running it on Linux until the packaged builds land.
          Issues and pull requests are welcome on{" "}
          <a href={GITHUB_URL}>GitHub</a>.
        </p>
      </div>

      <div style={{ maxWidth: 720, margin: "0 auto", background: "var(--crust)", border: "1px solid var(--surface)", borderRadius: 12, overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 16px", borderBottom: "1px solid var(--surface)", background: "var(--bg-alt)" }}>
          <span className="mono" style={{ fontSize: 11.5, color: "var(--muted)" }}>shell &mdash; macOS, Windows or Linux</span>
          <CopyButton
            text={SCRIPT}
            idle="copy"
            done={"copied ✓"}
            style={{ background: "none", border: "none", color: "var(--subtext)", fontFamily: "var(--mono)", fontSize: 11.5 }}
          />
        </div>
        <pre className="mono" style={{ margin: 0, padding: "20px 22px", fontSize: 13, lineHeight: 1.9, color: "var(--text)", overflowX: "auto" }}>
<span style={{ color: "var(--muted)" }}># run it in dev mode</span>{"\n"}
npm install{"\n"}
npm run tauri dev{"\n"}
<span style={{ color: "var(--muted)" }}># build installers &mdash; .dmg on macOS, .exe/.msi on Windows</span>{"\n"}
npm run tauri build{"\n"}
<span style={{ color: "var(--muted)" }}># the mira CLI alone (it also ships inside the app)</span>{"\n"}
npm run build:cli
        </pre>
      </div>

      <p style={{ maxWidth: 720, margin: "16px auto 0", fontSize: 12.5, lineHeight: 1.6, color: "var(--muted)", textAlign: "center", textWrap: "pretty" }}>
        Needs Node&nbsp;20+ and the Rust toolchain. On Windows, also the{" "}
        <span className="mono" style={{ color: "var(--subtext)" }}>Desktop development with C++</span> workload from the
        Visual Studio Build Tools. Bundles land in{" "}
        <span className="mono" style={{ color: "var(--subtext)" }}>src-tauri/target/release/bundle/</span>.
      </p>
    </section>
  );
}
