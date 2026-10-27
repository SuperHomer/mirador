/**
 * The board, as the mocks draw it: frames of cards on the dot grid, and
 * the controls on screen.
 *
 * Drawn in board units through one SVG viewBox — the camera — so it scales
 * the way the app's canvas does. The measures are the app's own
 * (FRAME_HEADER, FRAME_PADDING, CARD_GAP in cmux-core's board.rs; the dot
 * grid, frame and card header styles in the app's styles.css), and the
 * layout keeps the board's rules: frames sit side by side, FRAME_GAP
 * apart, so no two overlap, and cards stack down their frame, so no two in
 * a frame do. The controls sit on screen, unscaled, as the app's do, so
 * the parent must be positioned.
 *
 * A frame's title is the app's: its tab's name, a pip per agent and the
 * view it shows ("Splits", "Wall · Focus"); a frame where an agent waits
 * on you has a peach edge, the current one a muted edge. A walled tab's
 * frame holds its wall rather than cards. And while anyone waits, the
 * board's bottom row starts with the banner that counts them, the zoom
 * controls at its other end, as BoardHost draws them.
 */

import TermLines, { type Line } from "./TermLines";
import { FocusWall, Pip, type MockAgent, type Status } from "./MockAgents";

const FRAME_HEADER = 36;
const FRAME_PADDING = 16;
const CARD_GAP = 12;
const CARD_W = 400;
export const FRAME_W = CARD_W + 2 * FRAME_PADDING;
export const FRAME_GAP = 80;
/** The dot grid's spacing, in board units. */
const GRID = 24;

export type Card = {
  title: string;
  h: number;
  focused?: boolean;
  /** An agent's status, as the card's dot shows it. */
  status?: "working" | "needsYou" | "idle";
  /** A browser card: its URL, in its bar and in place of the page. */
  url?: string;
  lines?: Line[];
};
export type Frame = {
  title: string;
  current?: boolean;
  /** Where it sits on the board; by default the frames go side by side
      in order. */
  x?: number;
  cards: Card[];
  /** A walled tab: its wall in Focus, this tall, in place of its cards. */
  wall?: { agents: MockAgent[]; large: number; h: number };
};

/** A frame's agents' statuses, in order: its wall's, or its cards'. */
function statuses(frame: Frame): Status[] {
  return frame.wall ? frame.wall.agents.map((a) => a.status) : frame.cards.filter((c) => c.status).map((c) => c.status!);
}

/** Cards stacked down their frame, CARD_GAP apart, as the board lays them. */
export function layout(frame: Frame) {
  if (frame.wall) return { cards: [], h: FRAME_HEADER + frame.wall.h + FRAME_PADDING };
  let y = FRAME_HEADER;
  const cards = frame.cards.map((card) => {
    const at = { x: FRAME_PADDING, y, w: CARD_W, h: card.h };
    y += card.h + CARD_GAP;
    return { card, at };
  });
  return { cards, h: y - CARD_GAP + FRAME_PADDING };
}

const DOT_COLOR: Record<NonNullable<Card["status"]>, string> = {
  working: "var(--accent)",
  needsYou: "#fab387",
  idle: "#a6e3a1",
};

function CardView({ card, at, onClick }: ReturnType<typeof layout>["cards"][number] & { onClick?: () => void }) {
  const body = at.y + 27;
  return (
    <g
      {...(onClick && {
        role: "button",
        tabIndex: 0,
        "aria-label": `Go to ${card.title}`,
        className: "board-card",
        style: { cursor: "pointer" },
        onClick,
        onKeyDown: (e: React.KeyboardEvent) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onClick();
          }
        },
      })}
    >
      <rect x={at.x} y={at.y} width={at.w} height={at.h} rx={4} fill="var(--bg)" stroke="var(--surface)" />
      {/* the card header: what it is, and the handle that moves it */}
      <rect
        x={at.x + 3}
        y={at.y + 3}
        width={at.w - 6}
        height={22}
        rx={4}
        fill={card.focused ? "color-mix(in srgb, var(--muted) 45%, var(--surface))" : "var(--surface)"}
      />
      {card.status && <circle cx={at.x + 15} cy={at.y + 14} r={4} fill={DOT_COLOR[card.status]} />}
      <text
        x={at.x + (card.status ? 25 : 11)}
        y={at.y + 18}
        fontSize={11.5}
        fill={card.focused ? "var(--text)" : "var(--subtext)"}
      >
        {card.title}
      </text>
      <text x={at.x + at.w - 15} y={at.y + 18} fontSize={13} fill="var(--subtext)" textAnchor="middle">
        ×
      </text>
      {card.url ? (
        <g>
          {/* the pane's own bar; the page itself shows only at 100% */}
          <rect x={at.x + 4} y={body} width={at.w - 8} height={26} fill="var(--bg-alt)" />
          <text x={at.x + 12} y={body + 17} fontSize={12} fill="var(--subtext)">
            ‹ › ⟳
          </text>
          <rect x={at.x + 58} y={body + 4} width={at.w - 70} height={18} rx={4} fill="var(--bg)" />
          <text x={at.x + 64} y={body + 17} fontSize={11} fill="var(--text)">
            {card.url}
          </text>
          <text x={at.x + 20} y={body + 54} fontSize={13} fill="var(--subtext)">
            {card.url}
          </text>
        </g>
      ) : (
        // a terminal shows its last rows; what does not fit is cut, as there
        <foreignObject x={at.x + 4} y={body} width={at.w - 8} height={at.h - 31}>
          <div className="mono" style={{ height: "100%", overflow: "hidden", padding: "5px 6px", fontSize: 12.5, lineHeight: "19px" }}>
            <TermLines lines={card.lines ?? []} />
          </div>
        </foreignObject>
      )}
    </g>
  );
}

export default function BoardCanvas({
  frames,
  zoom,
  camera,
  screen,
  dotsId,
  label,
  onCard,
  onTabs,
}: {
  frames: Frame[];
  zoom: number;
  /** The board point at the screen's top left. */
  camera: { x: number; y: number };
  /** The screen, in pixels; the viewBox is this divided by the zoom. */
  screen: { w: number; h: number };
  /** The dot pattern's id, unique on the page. */
  dotsId: string;
  label: string;
  /** A click on a card, or on its frame (card undefined). */
  onCard?: (frame: number, card?: number) => void;
  onTabs?: () => void;
}) {
  const placed = frames.map((frame, i) => ({ frame, x: frame.x ?? i * (FRAME_W + FRAME_GAP), ...layout(frame) }));
  const view = { w: screen.w / zoom, h: screen.h / zoom };
  // Who waits on you, and in how many frames: the banner's two numbers.
  const waiting = frames.reduce((n, f) => n + statuses(f).filter((st) => st === "needsYou").length, 0);
  const waitingFrames = frames.filter((f) => statuses(f).includes("needsYou")).length;
  return (
    <>
      <svg
        viewBox={`${camera.x} ${camera.y} ${view.w} ${view.h}`}
        style={{ display: "block", width: "100%", height: "auto", fontFamily: "var(--sans)" }}
        role={onCard ? "group" : "img"}
        aria-label={label}
      >
        <defs>
          <pattern id={dotsId} width={GRID} height={GRID} patternUnits="userSpaceOnUse">
            <circle cx={GRID / 2} cy={GRID / 2} r={1 / zoom} fill="color-mix(in srgb, var(--muted) 35%, transparent)" />
          </pattern>
        </defs>
        <rect x={camera.x} y={camera.y} width={view.w} height={view.h} fill={`url(#${dotsId})`} />
        {placed.map(({ frame, x, cards, h }, fi) => (
          <g
            key={frame.title}
            transform={`translate(${x} 0)`}
            className={onCard && "board-frame"}
            // a card's click is the card's; the frame takes the rest
            onClick={onCard && ((e) => !(e.target as Element).closest(".board-card") && onCard(fi))}
            style={onCard && { cursor: "pointer" }}
          >
            <rect
              width={FRAME_W}
              height={h}
              rx={10}
              fill="color-mix(in srgb, var(--bg) 55%, transparent)"
              stroke={statuses(frame).includes("needsYou") ? "var(--peach)" : frame.current ? "var(--muted)" : "var(--surface)"}
              strokeWidth={statuses(frame).includes("needsYou") ? 1.5 : 1}
            />
            {/* the frame's title bar: its name, its agents, its view, "+" and × */}
            <foreignObject x={16} y={0} width={FRAME_W - 76} height={FRAME_HEADER}>
              <div className={`board-frame-title${frame.current ? " current" : ""}`}>
                <span className="board-frame-name">{frame.title}</span>
                {statuses(frame).length > 0 && (
                  <span className="ms-pips">
                    {statuses(frame).map((st, i) => (
                      <Pip key={i} status={st} />
                    ))}
                  </span>
                )}
                <span className="board-frame-view">{frame.wall ? "Wall · Focus" : "Splits"}</span>
              </div>
            </foreignObject>
            <text x={FRAME_W - 46} y={24} fontSize={16} fill="var(--subtext)" textAnchor="middle">
              +
            </text>
            <text x={FRAME_W - 20} y={24} fontSize={16} fill="var(--subtext)" textAnchor="middle">
              ×
            </text>
            {frame.wall && (
              <foreignObject x={FRAME_PADDING} y={FRAME_HEADER} width={CARD_W} height={frame.wall.h}>
                <div className="board-frame-wall">
                  <FocusWall agents={frame.wall.agents} large={frame.wall.large} />
                </div>
              </foreignObject>
            )}
            {cards.map((c, ci) => (
              <CardView key={ci} {...c} onClick={onCard && (() => onCard(fi, ci))} />
            ))}
          </g>
        ))}
      </svg>
      <div className="board-mock-overlay">
        {waiting > 0 && (
          <div className="board-mock-banner">
            <span className="board-mock-count">
              {waiting} {waiting === 1 ? "agent needs" : "agents need"} you
            </span>{" "}
            in {waitingFrames} {waitingFrames === 1 ? "frame" : "frames"} · <span className="board-mock-fly">Fly to next</span>
          </div>
        )}
        <div
          className="board-mock-controls"
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "4px 6px 4px 10px",
            borderRadius: 8,
            background: "var(--surface)",
            color: "var(--subtext)",
            fontSize: 12,
          }}
        >
          <span>{Math.round(zoom * 100)}%</span>
          {["Tabs", "New frame", "Fit"].map((name) => {
            const style = {
              border: "none",
              borderRadius: 5,
              background: "var(--bg)",
              color: "var(--text)",
              padding: "3px 10px",
              fontSize: 12,
              whiteSpace: "nowrap" as const,
            };
            return name === "Tabs" && onTabs ? (
              <button key={name} onClick={onTabs} style={style}>
                {name}
              </button>
            ) : (
              <span key={name} style={style}>
                {name}
              </span>
            );
          })}
        </div>
      </div>
    </>
  );
}
