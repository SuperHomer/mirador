/**
 * Keyboard shortcuts on the site. Mirador's bindings differ by platform —
 * plain Ctrl belongs to the program in the pane, so Windows and Linux are
 * not simply ⌘→Ctrl — and the page is static and gets shared, so it shows
 * both rather than guessing the reader's OS. Every section names keys
 * through `BothKeys`, so none can show one platform's half again.
 */

export function Key({ children }: { children: React.ReactNode }) {
  return (
    <kbd
      className="mono"
      style={{
        background: "var(--bg-alt)",
        border: "1px solid var(--surface)",
        borderRadius: 6,
        padding: "4px 10px",
        fontSize: 12.5,
        color: "var(--text)",
        whiteSpace: "nowrap",
      }}
    >
      {children}
    </kbd>
  );
}

/** The macOS key, then the Windows/Linux one (see Keybindings.tsx). */
export function BothKeys({ mac, other }: { mac: string; other: string }) {
  return (
    <>
      <Key>{mac}</Key>
      <span style={{ color: "var(--muted)", fontSize: 12.5 }}>/</span>
      <Key>{other}</Key>
    </>
  );
}
