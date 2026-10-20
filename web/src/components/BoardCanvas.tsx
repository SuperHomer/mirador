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
 */

import TermLines, { type Line } from "./TermLines";

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
export type Frame = { title: string; current?: boolean; cards: Card[] };

/** Cards stacked down their frame, CARD_GAP apart, as the board lays them. */
export function layout(frame: Frame) {
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
        onClick: (e: React.MouseEvent) => {
          e.stopPropagation();
          onClick();
        },
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
  const placed = frames.map((frame, i) => ({ frame, x: i * (FRAME_W + FRAME_GAP), ...layout(frame) }));
  const view = { w: screen.w / zoom, h: screen.h / zoom };
  return (
    <>
      <svg
        viewBox={`${camera.x} ${camera.y} ${view.w} ${view.h}`}
        style={{ display: "block", width: "100%", height: "auto", fontFamily: "var(--sans)" }}
        role="img"
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
            onClick={onCard && (() => onCard(fi))}
            style={onCard && { cursor: "pointer" }}
          >
            <rect
              width={FRAME_W}
              height={h}
              rx={10}
              fill="color-mix(in srgb, var(--bg) 55%, transparent)"
              stroke={frame.current ? "var(--muted)" : "var(--surface)"}
            />
            {/* the frame's title bar: its name, "+" and × */}
            <text x={16} y={23} fontSize={14} fontWeight={600} fill={frame.current ? "var(--text)" : "var(--subtext)"}>
              {frame.title}
            </text>
            <text x={FRAME_W - 46} y={24} fontSize={16} fill="var(--subtext)" textAnchor="middle">
              +
            </text>
            <text x={FRAME_W - 20} y={24} fontSize={16} fill="var(--subtext)" textAnchor="middle">
              ×
            </text>
            {cards.map((c, ci) => (
              <CardView key={ci} {...c} onClick={onCard && (() => onCard(fi, ci))} />
            ))}
          </g>
        ))}
      </svg>
      <div
        className="board-mock-controls"
        style={{
          position: "absolute",
          right: 12,
          bottom: 12,
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
    </>
  );
}
