import Logo from "./Logo";
import { GITHUB_URL } from "../config";

export default function Footer() {
  return (
    <footer style={{ padding: "44px 48px", borderTop: "1px solid var(--surface)", display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 20 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
        <Logo size={26} />
        <div>
          <div style={{ fontSize: 15, fontWeight: 700 }}>Mirador</div>
          <div style={{ fontSize: 12, color: "var(--muted)" }}>Built on Tauri&nbsp;2 · inspired by cmux</div>
        </div>
      </div>
      <div style={{ display: "flex", gap: 26, fontSize: 14 }}>
        <a href={GITHUB_URL} style={{ color: "var(--subtext)" }}>GitHub</a>
        <a href={GITHUB_URL} style={{ color: "var(--subtext)" }}>Docs</a>
        <a href={GITHUB_URL} style={{ color: "var(--subtext)" }}>mira CLI</a>
      </div>
    </footer>
  );
}
