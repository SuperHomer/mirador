// Plain Ctrl belongs to the program in the pane (Ctrl+C interrupts, Ctrl+D
// is EOF), so the Windows/Linux defaults are not simply ⌘→Ctrl. Ctrl+Alt is
// avoided too: it is AltGr on international keyboards.
const KEYS: [string, string, string][] = [
  ["New tab / close pane", "⌘T · ⌘W", "Ctrl+Shift+T · Ctrl+Shift+W"],
  ["Split right / down", "⌘D · ⌘⇧D", "Ctrl+Shift+D · Ctrl+Shift+E"],
  ["Focus pane by direction", "⌘⌥arrows", "Ctrl+Alt+arrows"],
  ["Jump to tab", "⌘1…9", "Alt+1…9"],
  ["Command palette", "⌘K", "Ctrl+Shift+K"],
  ["Notifications panel", "⌘I", "Ctrl+Shift+I"],
  ["Toggle sidebar", "⌘B", "Ctrl+Shift+B"],
  ["Copy / paste", "⌘C · ⌘V", "Ctrl+Shift+C · Ctrl+Shift+V"],
];

// minmax(0,…) so a long combo shrinks its column instead of overflowing.
const COLUMNS = "minmax(0, 1.5fr) minmax(0, 1fr) minmax(0, 1.5fr)";

export default function Keybindings() {
  return (
    <section style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)", background: "var(--bg-alt)" }}>
      <div style={{ textAlign: "center", marginBottom: 36 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>KEYBINDINGS</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>Default keys</h2>
        <p style={{ color: "var(--muted)", fontSize: 14, marginTop: 8 }}>
          Plain Ctrl stays with the program in your pane · all rebindable in{" "}
          <span className="mono" style={{ color: "var(--subtext)" }}>mirador.json</span>
        </p>
      </div>
      {/* Grid, not flex: three columns whose widths must add up exactly,
          which percentage flex-bases plus gaps cannot do without wrapping. */}
      <div style={{ maxWidth: 820, margin: "0 auto", display: "flex", flexDirection: "column", gap: 2 }}>
        <div style={{ display: "grid", gridTemplateColumns: COLUMNS, gap: 16, padding: "0 16px 8px" }}>
          <span />
          <span className="mono" style={{ fontSize: 11, letterSpacing: ".1em", color: "var(--muted)" }}>MACOS</span>
          <span className="mono" style={{ fontSize: 11, letterSpacing: ".1em", color: "var(--muted)" }}>WINDOWS / LINUX</span>
        </div>
        {KEYS.map(([label, mac, win], i) => (
          <div key={label} style={{ display: "grid", gridTemplateColumns: COLUMNS, alignItems: "center", gap: 16, padding: "11px 16px", borderRadius: 7, background: i % 2 === 0 ? "var(--bg)" : "transparent" }}>
            <span style={{ fontSize: 14, color: "var(--text)" }}>{label}</span>
            <span className="mono" style={{ fontSize: 12, color: "var(--accent)" }}>{mac}</span>
            <span className="mono" style={{ fontSize: 12, color: "var(--accent)" }}>{win}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
