import Logo from "./Logo";
import { GITHUB_URL, DOWNLOAD_URL, DOWNLOAD_VERSION } from "../config";

export default function Nav() {
  return (
    <nav
      style={{
        position: "sticky",
        top: 0,
        zIndex: 10,
        borderBottom: "1px solid var(--surface)",
        background: "rgba(24,24,37,.72)",
        backdropFilter: "blur(10px)",
      }}
    >
      <div
        style={{
          maxWidth: 1280,
          margin: "0 auto",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "18px 34px",
        }}
      >
      <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
        <Logo size={30} />
        <span style={{ fontSize: 19, fontWeight: 750, letterSpacing: "-.02em" }}>Mirador</span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 28, fontSize: 14, color: "var(--subtext)" }}>
        <a href="#features" style={{ color: "var(--subtext)" }}>Features</a>
        <a href="#cli" style={{ color: "var(--subtext)" }}>CLI</a>
        <a href="#install" style={{ color: "var(--subtext)" }}>Install</a>
        <a href={GITHUB_URL} style={{ color: "var(--subtext)" }}>GitHub</a>
        <a
          href={DOWNLOAD_URL}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            background: "var(--accent)",
            color: "var(--bg-alt)",
            fontWeight: 650,
            padding: "9px 16px",
            borderRadius: 8,
          }}
        >
          Download
          <span className="mono" style={{ fontSize: 11, fontWeight: 600, opacity: 0.7 }}>{DOWNLOAD_VERSION}</span>
        </a>
      </div>
      </div>
    </nav>
  );
}
