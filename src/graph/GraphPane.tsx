import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  GraphRef,
  GraphRow,
  closePane,
  focusPane,
  graphShowCommit,
  loadGraph,
} from "../bindings";
import { useConfigStore } from "../state/configStore";

type Props = {
  paneId: string;
  repo: string;
  focused: boolean;
};

/** Row height in px, and the lane pitch — both fixed so the SVG lines and
 *  the DOM rows cannot drift apart at any zoom. */
const ROW_H = 24;
const LANE_W = 14;
const DOT_R = 3.5;
/** Lanes drawn before the gutter stops widening; beyond this they overlap
 *  rather than pushing the subject column off the pane. */
const MAX_LANES = 10;

/** Lane colour, by index. Branches keep their colour for their whole length
 *  because a lane is never renumbered mid-history. */
const LANE_COLORS = [
  "#89b4fa",
  "#a6e3a1",
  "#f9e2af",
  "#f38ba8",
  "#cba6f7",
  "#94e2d5",
  "#fab387",
  "#b4befe",
];
const laneColor = (lane: number) => LANE_COLORS[lane % LANE_COLORS.length];

const laneX = (lane: number) => Math.min(lane, MAX_LANES) * LANE_W + LANE_W / 2;

/** "3 days ago" — a graph is read by recency, not by timestamp. */
function ago(epochSeconds: number): string {
  const s = Math.max(0, Date.now() / 1000 - epochSeconds);
  const units: [number, string][] = [
    [60, "s"],
    [3600, "m"],
    [86400, "h"],
    [86400 * 30, "d"],
    [86400 * 365, "mo"],
  ];
  if (s < 60) return `${Math.floor(s)}s`;
  for (let i = 1; i < units.length; i++) {
    if (s < units[i][0]) {
      return `${Math.floor(s / units[i - 1][0])}${units[i][1]}`;
    }
  }
  return `${Math.floor(s / (86400 * 365))}y`;
}

/**
 * The commit graph: one row per commit, with the branch lines drawn as an
 * SVG overlay sized from the same constants the rows use.
 *
 * Lane placement happens in Rust (cmux-core/src/graph.rs); this draws what
 * it is told and knows nothing about parents or merges beyond the links it
 * is handed.
 */
export function GraphPane({ paneId, repo, focused }: Props) {
  const [rows, setRows] = useState<GraphRow[]>([]);
  const [lanes, setLanes] = useState(1);
  const [truncated, setTruncated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<string | null>(null);
  const colors = useConfigStore((s) => s.config?.colors);

  const refresh = useCallback(() => {
    setLoading(true);
    let stale = false;
    void loadGraph(paneId)
      .then((g) => {
        if (stale) return;
        setRows(g.rows);
        setLanes(g.lanes);
        setTruncated(g.truncated);
        setError(null);
      })
      .catch((e: unknown) => {
        if (!stale) setError(String(e));
      })
      .finally(() => {
        if (!stale) setLoading(false);
      });
    return () => {
      stale = true;
    };
  }, [paneId]);

  useEffect(() => refresh(), [refresh]);

  // A commit lands while you are looking at the graph; coming back to the
  // pane is the natural "show me where we are now", as in the diff pane.
  const wasFocused = useRef(focused);
  useEffect(() => {
    if (focused && !wasFocused.current) refresh();
    wasFocused.current = focused;
  }, [focused, refresh]);

  const show = (sha: string) => {
    setSelected(sha);
    void graphShowCommit(paneId, sha).catch((e: unknown) => setError(String(e)));
  };

  const gutter = Math.min(lanes, MAX_LANES + 1) * LANE_W;

  return (
    <div
      className={`pane graph-pane${focused ? " focused" : ""}`}
      style={
        colors
          ? ({ "--term-bg": colors.background } as React.CSSProperties)
          : undefined
      }
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Graph</span>
        <span className="graph-repo" title={repo}>
          {repo.split("/").pop()}
        </span>
        <span className="graph-count">
          {rows.length} {rows.length === 1 ? "commit" : "commits"}
          {truncated && "+"}
        </span>
        <button
          className="graph-refresh"
          title="Refresh"
          disabled={loading}
          onClick={() => refresh()}
        >
          ⟳
        </button>
        <button
          className="graph-close"
          title="Close graph pane"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>

      <div className="graph-body">
        {error ? (
          <p className="graph-error">{error}</p>
        ) : rows.length === 0 ? (
          <p className="graph-empty">
            {loading ? "Reading history…" : "No commits yet"}
          </p>
        ) : (
          <div className="graph-scroll">
            <svg
              className="graph-lines"
              width={gutter}
              // Half a row of slack so the lines leaving the last row
              // show as stubs — the cue that history continues below.
              height={rows.length * ROW_H + ROW_H / 2}
              aria-hidden="true"
            >
              {rows.map((row, i) =>
                row.links.map((link, j) => (
                  <path
                    key={`${row.sha}-${j}`}
                    // Straight down within a lane; an S-curve when the line
                    // changes lane, so a merge reads as one continuous
                    // branch instead of a corner.
                    d={
                      link.from === link.to
                        ? `M ${laneX(link.from)} ${i * ROW_H + ROW_H / 2} V ${(i + 1) * ROW_H + ROW_H / 2}`
                        : `M ${laneX(link.from)} ${i * ROW_H + ROW_H / 2}` +
                          ` C ${laneX(link.from)} ${i * ROW_H + ROW_H},` +
                          ` ${laneX(link.to)} ${i * ROW_H + ROW_H},` +
                          ` ${laneX(link.to)} ${(i + 1) * ROW_H + ROW_H / 2}`
                    }
                    stroke={laneColor(link.from === link.to ? link.from : link.to)}
                    strokeWidth={1.5}
                    fill="none"
                  />
                )),
              )}
              {rows.map((row, i) => (
                <circle
                  key={row.sha}
                  cx={laneX(row.lane)}
                  cy={i * ROW_H + ROW_H / 2}
                  r={DOT_R}
                  // A merge is hollow: it is a joining point, not work.
                  fill={row.merge ? "var(--term-bg)" : laneColor(row.lane)}
                  stroke={laneColor(row.lane)}
                  strokeWidth={1.5}
                />
              ))}
            </svg>
            <div className="graph-rows" style={{ marginLeft: gutter }}>
              {rows.map((row) => (
                <div
                  key={row.sha}
                  className={`graph-row${row.sha === selected ? " selected" : ""}`}
                  style={{ height: ROW_H }}
                  onClick={() => show(row.sha)}
                  // The subject is ellipsized to keep rows one line tall,
                  // so hovering is how the rest of it is read.
                  title={rowTooltip(row)}
                >
                  {row.refs.map((r) => (
                    <RefChip key={`${r.kind}:${r.name}`} chip={r} />
                  ))}
                  <span className="graph-subject">{row.subject}</span>
                  <span className="graph-meta">
                    <span className="graph-author">{row.author}</span>
                    <span className="graph-sha">{row.short}</span>
                    <span className="graph-ago">{ago(row.timestamp)}</span>
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** The whole subject, plus what the row had no room to show. */
const rowTooltip = (row: GraphRow) =>
  [
    row.subject,
    "",
    `${row.short} · ${row.author} · ${new Date(row.timestamp * 1000).toLocaleString()}`,
    row.refs.length > 0 ? row.refs.map((r) => r.name).join(", ") : null,
  ]
    .filter((line) => line !== null)
    .join("\n");

const RefChip = ({ chip }: { chip: GraphRef }) => (
  <span className={`graph-ref ${chip.kind}`}>
    {chip.kind === "tag" ? "⚑ " : chip.kind === "head" ? "⎇ " : ""}
    {chip.name}
  </span>
);

export default memo(GraphPane);
