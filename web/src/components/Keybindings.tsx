const KEYS: [string, string][] = [
  ["New tab / close pane", "mod+T · mod+W"],
  ["Split right / down", "mod+D · mod+shift+D"],
  ["Focus pane by direction", "mod+alt+arrows"],
  ["Jump to tab", "mod+1…9"],
  ["Command palette", "mod+K"],
  ["Notifications panel", "mod+I"],
  ["Toggle sidebar", "mod+B"],
];

export default function Keybindings() {
  return (
    <section style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)", background: "var(--bg-alt)" }}>
      <div style={{ textAlign: "center", marginBottom: 36 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>KEYBINDINGS</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>Default keys</h2>
        <p style={{ color: "var(--muted)", fontSize: 14, marginTop: 8 }}>
          <span className="mono">mod</span> = ⌘ on macOS, Ctrl elsewhere · all rebindable in{" "}
          <span className="mono" style={{ color: "var(--subtext)" }}>mirador.json</span>
        </p>
      </div>
      <div style={{ maxWidth: 720, margin: "0 auto", display: "flex", flexDirection: "column", gap: 2 }}>
        {KEYS.map(([label, combo], i) => (
          <div key={label} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "11px 16px", borderRadius: 7, background: i % 2 === 0 ? "var(--bg)" : "transparent" }}>
            <span style={{ fontSize: 14, color: "var(--text)" }}>{label}</span>
            <span className="mono" style={{ fontSize: 12, color: "var(--accent)" }}>{combo}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
