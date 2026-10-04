/**
 * The commit-graph pane, as a static mock.
 *
 * Lanes are drawn the way the real pane draws them — the same geometry
 * constants and the same link model (a segment from a lane on one row to a
 * lane on the next), so the picture cannot drift into showing a graph the
 * app would never produce.
 */

const ROW_H = 26;
const LANE_W = 16;
const DOT_R = 4;

const LANE_COLORS = ["var(--accent)", "var(--green)", "var(--yellow)"];
const laneX = (lane: number) => lane * LANE_W + LANE_W / 2;

type Ref = { name: string; kind: "head" | "branch" | "remote" | "tag" };
type Row = {
  lane: number;
  /** [from lane on this row, to lane on the next] */
  links: [number, number][];
  merge?: boolean;
  sha: string;
  subject: string;
  author: string;
  ago: string;
  refs?: Ref[];
};

/** Two merged branches, which is what makes lanes worth drawing at all. */
const ROWS: Row[] = [
  {
    lane: 0,
    links: [[0, 0], [0, 1]],
    merge: true,
    sha: "a3f1c2d",
    subject: "Merge pull request #128 from feat/refresh",
    author: "Yoan",
    ago: "2h",
    refs: [
      { name: "main", kind: "head" },
      { name: "origin/main", kind: "remote" },
    ],
  },
  {
    lane: 0,
    links: [[0, 0], [1, 1]],
    sha: "9b1a7f3",
    subject: "Tighten the default token TTL",
    author: "Yoan",
    ago: "3h",
  },
  {
    lane: 1,
    links: [[1, 0], [0, 0]],
    sha: "7e4b901",
    subject: "Add refresh-token rotation",
    author: "agent",
    ago: "4h",
    refs: [{ name: "feat/refresh", kind: "branch" }],
  },
  {
    lane: 0,
    links: [[0, 0]],
    sha: "2c8d5e6",
    subject: "Release v1.4.0",
    author: "Yoan",
    ago: "1d",
    refs: [{ name: "v1.4.0", kind: "tag" }],
  },
  {
    lane: 0,
    links: [[0, 0], [0, 1]],
    merge: true,
    sha: "5a2f9b8",
    subject: "Merge pull request #124 from perf/jwks",
    author: "Yoan",
    ago: "2d",
  },
  {
    lane: 0,
    links: [[0, 0], [1, 1]],
    sha: "c41e07a",
    subject: "Log auth failures with request ids",
    author: "agent",
    ago: "2d",
  },
  {
    lane: 1,
    links: [[1, 0], [0, 0]],
    sha: "4d6e8c1",
    subject: "Cache JWKS between requests",
    author: "agent",
    ago: "3d",
    refs: [{ name: "perf/jwks", kind: "branch" }],
  },
  {
    lane: 0,
    links: [[0, 0]],
    sha: "8f0b3d2",
    subject: "Reject tokens without an audience",
    author: "Yoan",
    ago: "3d",
  },
];

const REF_STYLE: Record<Ref["kind"], React.CSSProperties> = {
  head: { background: "var(--accent)", color: "var(--crust)", fontWeight: 700 },
  branch: { border: "1px solid var(--accent)", color: "var(--accent)" },
  remote: { border: "1px solid var(--muted)", color: "var(--muted)" },
  tag: { border: "1px solid var(--yellow)", color: "var(--yellow)" },
};

function RefChip({ chip }: { chip: Ref }) {
  return (
    <span
      className="mono"
      style={{
        ...REF_STYLE[chip.kind],
        fontSize: 9.5,
        padding: "1px 5px",
        borderRadius: 3,
        flexShrink: 0,
        whiteSpace: "nowrap",
      }}
    >
      {chip.kind === "tag" ? "⚑ " : chip.kind === "head" ? "⎇ " : ""}
      {chip.name}
    </span>
  );
}

function Key({ children }: { children: React.ReactNode }) {
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

function GraphMockup() {
  const lanes = 2;
  const gutter = (lanes + 1) * LANE_W;
  return (
    <div
      style={{
        maxWidth: 940,
        margin: "44px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        textAlign: "left",
      }}
    >
      {/* chrome */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "7px 10px",
          background: "var(--bg-alt)",
          borderBottom: "1px solid var(--surface)",
        }}
      >
        <span
          className="mono"
          style={{
            background: "var(--accent)",
            color: "var(--crust)",
            fontSize: 9.5,
            fontWeight: 700,
            letterSpacing: ".04em",
            padding: "2px 6px",
            borderRadius: 4,
          }}
        >
          GRAPH
        </span>
        <span style={{ fontSize: 12, fontWeight: 600 }}>auth-service</span>
        <span style={{ fontSize: 11, color: "var(--muted)" }}>412 commits</span>
        <span style={{ marginLeft: "auto", color: "var(--muted)", fontSize: 12 }}>⟳</span>
      </div>

      <div className="mono" style={{ position: "relative", padding: "6px 0" }}>
        <svg
          width={gutter}
          height={ROWS.length * ROW_H + ROW_H / 2}
          style={{ position: "absolute", top: 6, left: 0, pointerEvents: "none" }}
          aria-hidden="true"
        >
          {ROWS.map((row, i) =>
            row.links.map(([from, to], j) => (
              <path
                key={`${row.sha}-${j}`}
                d={
                  from === to
                    ? `M ${laneX(from)} ${i * ROW_H + ROW_H / 2} V ${(i + 1) * ROW_H + ROW_H / 2}`
                    : `M ${laneX(from)} ${i * ROW_H + ROW_H / 2}` +
                      ` C ${laneX(from)} ${i * ROW_H + ROW_H},` +
                      ` ${laneX(to)} ${i * ROW_H + ROW_H},` +
                      ` ${laneX(to)} ${(i + 1) * ROW_H + ROW_H / 2}`
                }
                stroke={LANE_COLORS[(from === to ? from : to) % LANE_COLORS.length]}
                strokeWidth={1.6}
                fill="none"
              />
            )),
          )}
          {ROWS.map((row, i) => (
            <circle
              key={row.sha}
              cx={laneX(row.lane)}
              cy={i * ROW_H + ROW_H / 2}
              r={DOT_R}
              fill={row.merge ? "var(--bg)" : LANE_COLORS[row.lane % LANE_COLORS.length]}
              stroke={LANE_COLORS[row.lane % LANE_COLORS.length]}
              strokeWidth={1.6}
            />
          ))}
        </svg>

        <div style={{ marginLeft: gutter }}>
          {ROWS.map((row, i) => (
            <div
              key={row.sha}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                height: ROW_H,
                padding: "0 12px 0 4px",
                fontSize: 11.5,
                // One row selected, because clicking a commit is the point.
                background: i === 2 ? "var(--surface)" : undefined,
                whiteSpace: "nowrap",
              }}
            >
              {row.refs?.map((r) => (
                <RefChip key={r.name} chip={r} />
              ))}
              <span
                style={{
                  color: "var(--text)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  flex: "1 1 auto",
                  minWidth: 0,
                }}
              >
                {row.subject}
              </span>
              <span
                style={{
                  display: "flex",
                  gap: 10,
                  flexShrink: 0,
                  color: "var(--muted)",
                  fontSize: 10.5,
                }}
              >
                <span>{row.author}</span>
                <span>{row.sha}</span>
                <span style={{ minWidth: "3ch", textAlign: "right" }}>{row.ago}</span>
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function GraphShowcase() {
  return (
    <section
      id="graph"
      style={{
        padding: "66px 48px",
        borderTop: "1px solid var(--surface)",
        background: "var(--bg)",
      }}
    >
      <div style={{ textAlign: "center", maxWidth: 720, margin: "0 auto" }}>
        <span
          className="mono"
          style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}
        >
          HISTORY
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          See how it got there
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          Branches, merges, tags and remotes, drawn in lanes that keep their colour for
          the whole length of a branch. Click a commit and its diff opens beside the
          graph — in the same pane each time, so walking back through history never
          buries the thing you are reading.
        </p>
        <div
          style={{
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            gap: 10,
            marginTop: 22,
            fontSize: 13.5,
            color: "var(--subtext)",
            flexWrap: "wrap",
          }}
        >
          <span className="mono" style={{ color: "var(--text)" }}>
            mira graph
          </span>
          <span>or</span>
          <Key>⌘K</Key>
          <span style={{ color: "var(--muted)", fontSize: 12.5 }}>→ “Git: Commit Graph”</span>
        </div>
      </div>
      <GraphMockup />
    </section>
  );
}
