import CopyButton from "./CopyButton";
import { DOWNLOAD_MACOS, DOWNLOAD_WINDOWS } from "../config";

// The path for someone who clicked a download button — not the source build,
// which lives in <Contribute>. Both platforms need step 2: the mira CLI ships
// inside the app bundle, so without `mira install` every command elsewhere on
// this page is unrunnable.
type Step = { text: string; code?: string; note?: string };

const MACOS: Step[] = [
  { text: "Open the .dmg and drag Mirador.app to /Applications." },
  {
    text: "First launch: right-click the app → Open.",
    note: "The build is unsigned (no Developer ID), so a double-click gets stopped by Gatekeeper. macOS remembers the choice — you only do this once.",
  },
  {
    text: "Put the mira CLI on your PATH — it ships inside the app.",
    code: "/Applications/Mirador.app/Contents/MacOS/mira install",
    note: "Installs to ~/.local/bin/mira.",
  },
  {
    text: "Wire up the Claude Code integration.",
    code: "mira hooks setup",
    note: "Tabs light up when your agent needs you, and panes resume their session.",
  },
];

const WINDOWS: Step[] = [
  {
    text: "Run Mirador-Windows-x64-setup.exe.",
    note: "It installs per user, so there is no admin prompt.",
  },
  {
    text: "Put mira.exe on your PATH — it sits next to mirador.exe.",
    code: '& "$env:LOCALAPPDATA\\Mirador\\mira.exe" install',
    note: "This edits your user PATH, so open a new terminal afterwards.",
  },
  {
    text: "Wire up the Claude Code integration.",
    code: "mira hooks setup",
    note: "Tabs light up when your agent needs you, and panes resume their session.",
  },
];

// PowerShell prompts with ">", not "$" — and a "$" in front of `$env:` is
// actively confusing.
function StepList({ steps, prompt }: { steps: Step[]; prompt: string }) {
  return (
    <ol style={{ listStyle: "none", display: "flex", flexDirection: "column", gap: 20 }}>
      {steps.map((s, i) => (
        <li key={s.text} style={{ display: "flex", gap: 13, alignItems: "flex-start" }}>
          <span
            className="mono"
            style={{
              flexShrink: 0,
              width: 22,
              height: 22,
              borderRadius: "50%",
              background: "rgba(137,180,250,.14)",
              color: "var(--accent)",
              fontSize: 11.5,
              fontWeight: 700,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              marginTop: 1,
            }}
          >
            {i + 1}
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <p style={{ fontSize: 14, lineHeight: 1.5, color: "var(--text)", textWrap: "pretty" }}>{s.text}</p>
            {s.code && (
              <div
                className="mono"
                style={{
                  marginTop: 9,
                  background: "var(--crust)",
                  border: "1px solid var(--surface)",
                  borderRadius: 8,
                  padding: "9px 12px",
                  fontSize: 12,
                  color: "var(--text)",
                  overflowX: "auto",
                  whiteSpace: "pre",
                }}
              >
                <span style={{ color: "var(--muted)" }}>{prompt} </span>
                {s.code}
              </div>
            )}
            {s.note && (
              <p style={{ marginTop: 8, fontSize: 12.5, lineHeight: 1.55, color: "var(--muted)", textWrap: "pretty" }}>
                {s.note}
              </p>
            )}
          </div>
        </li>
      ))}
    </ol>
  );
}

function Platform({
  name,
  sub,
  href,
  cta,
  steps,
  prompt,
}: {
  name: string;
  sub: string;
  href: string;
  cta: string;
  steps: Step[];
  prompt: string;
}) {
  const copyable = steps
    .map((s) => s.code)
    .filter(Boolean)
    .join("\n");
  return (
    <div
      style={{
        background: "var(--bg-alt)",
        border: "1px solid var(--surface)",
        borderRadius: 14,
        padding: 26,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
        <h3 style={{ fontSize: 19, fontWeight: 700, letterSpacing: "-.01em" }}>{name}</h3>
        <span className="mono" style={{ fontSize: 11.5, color: "var(--muted)" }}>{sub}</span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 14, margin: "16px 0 24px", flexWrap: "wrap" }}>
        <a
          href={href}
          style={{
            background: "var(--surface)",
            color: "var(--text)",
            border: "1px solid var(--overlay)",
            fontSize: 13.5,
            fontWeight: 650,
            padding: "9px 16px",
            borderRadius: 8,
          }}
        >
          {cta}
        </a>
        <CopyButton
          text={copyable}
          idle="copy commands"
          done={"copied ✓"}
          style={{
            background: "none",
            border: "none",
            color: "var(--subtext)",
            fontFamily: "var(--mono)",
            fontSize: 12,
          }}
        />
      </div>
      <StepList steps={steps} prompt={prompt} />
    </div>
  );
}

export default function Install() {
  return (
    <section id="install" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)" }}>
      <div style={{ textAlign: "center", marginBottom: 40 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>GET STARTED</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          From download to first agent
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, maxWidth: 560, margin: "14px auto 0", textWrap: "pretty" }}>
          Grab the installer, then put the{" "}
          <span className="mono" style={{ color: "var(--text)" }}>mira</span> CLI on your PATH — it
          ships inside the app, and it is what your agents talk to.
        </p>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(360px, 1fr))", gap: 18, maxWidth: 1100, margin: "0 auto" }}>
        <Platform name="macOS" sub="Apple silicon" href={DOWNLOAD_MACOS} cta="Download .dmg" steps={MACOS} prompt="$" />
        <Platform name="Windows" sub="x64 installer" href={DOWNLOAD_WINDOWS} cta="Download .exe" steps={WINDOWS} prompt=">" />
      </div>

      <div
        style={{
          maxWidth: 1100,
          margin: "18px auto 0",
          background: "var(--bg-alt)",
          border: "1px dashed var(--overlay)",
          borderRadius: 14,
          padding: "18px 24px",
          display: "flex",
          alignItems: "center",
          gap: 14,
          flexWrap: "wrap",
        }}
      >
        <span className="mono" style={{ fontSize: 11, letterSpacing: ".12em", color: "var(--yellow)", flexShrink: 0 }}>
          LINUX
        </span>
        <p style={{ flex: 1, minWidth: 240, fontSize: 13.5, lineHeight: 1.6, color: "var(--subtext)" }}>
          Packaged builds are coming soon. In the meantime it runs fine from source —{" "}
          <a href="#contribute">build it yourself</a>.
        </p>
      </div>
    </section>
  );
}
