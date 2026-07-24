import AppMockup from "./AppMockup";
import { GITHUB_URL, DOWNLOAD_URL, DOWNLOAD_VERSION, SHOW_APP_MOCKUP } from "../config";

export default function Hero() {
  return (
    <header
      style={{
        position: "relative",
        padding: "78px 48px 64px",
        textAlign: "center",
        overflow: "hidden",
        background:
          "radial-gradient(680px 340px at 50% -8%, rgba(137,180,250,.16), transparent 70%)",
      }}
    >
      <div
        className="mono"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 9,
          fontSize: 12,
          letterSpacing: ".08em",
          color: "var(--accent)",
          border: "1px solid var(--surface)",
          background: "var(--bg-alt)",
          borderRadius: 999,
          padding: "6px 14px",
          marginBottom: 26,
        }}
      >
        TAURI&nbsp;2 <span style={{ color: "var(--muted)" }}>·</span> macOS{" "}
        <span style={{ color: "var(--muted)" }}>·</span> Windows{" "}
        <span style={{ color: "var(--muted)" }}>·</span> Linux
      </div>
      <h1
        style={{
          fontSize: 60,
          lineHeight: 1.04,
          fontWeight: 780,
          letterSpacing: "-.03em",
          maxWidth: 840,
          margin: "0 auto",
          textWrap: "balance",
        }}
      >
        A lookout tower
        <br />
        over your agents
      </h1>
      <p
        style={{
          fontSize: 18,
          lineHeight: 1.6,
          color: "var(--subtext)",
          maxWidth: 620,
          margin: "24px auto 0",
          textWrap: "pretty",
        }}
      >
        A cross-platform terminal built for AI coding-agent workflows. Tabs, splits,
        attention rings, a scriptable browser pane, and remote workspaces — every action
        drivable from the <span className="mono" style={{ color: "var(--text)" }}>mira</span> CLI.
      </p>
      <div style={{ display: "flex", justifyContent: "center", gap: 14, marginTop: 34, flexWrap: "wrap" }}>
        <a
          href={DOWNLOAD_URL}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 9,
            background: "var(--accent)",
            color: "var(--bg-alt)",
            fontWeight: 700,
            fontSize: 15,
            padding: "13px 24px",
            borderRadius: 10,
          }}
        >
          Download for macOS
          <span className="mono" style={{ fontSize: 11, opacity: 0.7 }}>Apple silicon · {DOWNLOAD_VERSION}</span>
        </a>
        <a
          href={GITHUB_URL}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 9,
            background: "var(--surface)",
            color: "var(--text)",
            fontWeight: 650,
            fontSize: 15,
            padding: "13px 24px",
            borderRadius: 10,
            border: "1px solid var(--overlay)",
          }}
        >
          View on GitHub
        </a>
      </div>
      <div
        className="mono"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 12,
          marginTop: 24,
          fontSize: 13,
          color: "var(--subtext)",
          background: "var(--crust)",
          border: "1px solid var(--surface)",
          borderRadius: 9,
          padding: "10px 16px",
        }}
      >
        <span style={{ color: "var(--muted)" }}>$</span> npm run tauri dev
        <span className="cursor" style={{ width: 8, height: 16 }} />
      </div>

      {SHOW_APP_MOCKUP && <AppMockup />}
    </header>
  );
}
