/** A terminal's lines as the mocks draw them, shared so they cannot drift. */

export type Line = { text: string; color?: string };

export default function TermLines({ lines, cursor }: { lines: Line[]; cursor?: boolean }) {
  return lines.map((l, j) => (
    <div key={j} style={{ color: l.color ?? "var(--subtext)", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
      {l.text}
      {cursor && j === lines.length - 1 && (
        <span className="cursor" style={{ width: 7, height: 13, verticalAlign: "middle", marginLeft: 2 }} />
      )}
    </div>
  ));
}
