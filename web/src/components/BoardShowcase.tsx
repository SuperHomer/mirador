/**
 * The board, as a static mock.
 *
 * Drawn in board units through one SVG viewBox — the camera — so it scales
 * the way the app's canvas does, down to phone width. The measures are the
 * app's own (FRAME_HEADER, FRAME_PADDING, CARD_GAP in cmux-core's board.rs;
 * the dot grid, frame and card header styles in the app's styles.css), and
 * the layout keeps the board's rules: no two frames overlap, no two cards
 * in a frame do. The controls sit on screen, unscaled, as the app's do.
 */

import { BothKeys } from "./Keys";

const ZOOM = 0.6;
/** The mock's screen, in pixels; the viewBox is this divided by ZOOM. */
const SCREEN_W = 940;
const SCREEN_H = 500;
const CAMERA = { x: -55, y: -60 };

const FRAME_HEADER = 36;
const FRAME_PADDING = 16;
const CARD_GAP = 12;
const CARD_W = 400;
const FRAME_W = CARD_W + 2 * FRAME_PADDING;
const FRAME_GAP = 80;
/** The dot grid's spacing, in board units. */
const GRID = 24;

type Line = { text: string; color?: string };
type Card = {
  title: string;
  h: number;
  focused?: boolean;
  /** An agent's status, as the card's dot shows it. */
  status?: "working" | "needsYou" | "idle";
  /** A browser card: its URL, in its bar and in place of the page. */
  url?: string;
  lines?: Line[];
};
type Frame = { title: string; current?: boolean; cards: Card[] };

const FRAMES: Frame[] = [
  {
    title: "api",
    current: true,
    cards: [
      {
        title: "api",
        h: 300,
        focused: true,
        lines: [
          { text: "$ cargo run", color: "var(--text)" },
          { text: "   Compiling api v0.4.0", color: "var(--green)" },
          { text: "    Finished dev in 3.1s", color: "var(--green)" },
          { text: "     Running target/debug/api" },
          { text: "listening on 127.0.0.1:8080" },
          { text: "GET /health 200 0.4ms" },
          { text: "POST /login 200 12ms" },
        ],
      },
      {
        title: "api",
        h: 250,
        lines: [
          { text: "$ cargo test", color: "var(--text)" },
          { text: "test auth::rejects_expired ... ok" },
          { text: "test auth::rotates_refresh ... ok" },
          { text: "test result: ok. 42 passed", color: "var(--green)" },
        ],
      },
    ],
  },
  {
    title: "web",
    cards: [
      { title: "http://localhost:5173/login", h: 330, url: "http://localhost:5173/login" },
      {
        title: "web",
        h: 190,
        lines: [
          { text: "$ npm run dev", color: "var(--text)" },
          { text: "  VITE ready in 312 ms", color: "var(--green)" },
          { text: "  ➜  Local: http://localhost:5173/" },
        ],
      },
    ],
  },
  {
    title: "agents",
    cards: [
      {
        title: "reviewer",
        h: 260,
        status: "needsYou",
        lines: [
          { text: "✳ Reviewing the token rotation", color: "var(--text)" },
          { text: "  Bash(cargo test -p api)" },
          { text: "  Do you want to proceed?", color: "#fab387" },
          { text: "  ❯ 1. Yes" },
          { text: "    2. No" },
        ],
      },
      {
        title: "planner",
        h: 260,
        status: "working",
        lines: [
          { text: "✳ Splitting the login work", color: "var(--text)" },
          { text: "  ☒ Read the auth module" },
          { text: "  ☐ Hand step 2 to the worker" },
          { text: "  ✶ Thinking…", color: "var(--accent)" },
        ],
      },
    ],
  },
];

/** Cards stacked down their frame, CARD_GAP apart, as the board lays them. */
function layout(frame: Frame) {
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

function CardView({ card, at }: ReturnType<typeof layout>["cards"][number]) {
  const body = at.y + 27;
  return (
    <g>
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
        <text className="mono" fontSize={12.5} fill="var(--subtext)">
          {card.lines?.map((l, i) => (
            <tspan key={i} x={at.x + 10} y={body + 20 + i * 19} fill={l.color}>
              {l.text}
            </tspan>
          ))}
        </text>
      )}
    </g>
  );
}

function BoardMockup() {
  const frames = FRAMES.map((frame, i) => ({
    frame,
    x: i * (FRAME_W + FRAME_GAP),
    ...layout(frame),
  }));
  return (
    <div
      style={{
        position: "relative",
        maxWidth: SCREEN_W,
        margin: "44px auto 0",
        border: "1px solid var(--surface)",
        borderRadius: 12,
        overflow: "hidden",
        background: "var(--bg-alt)",
        boxShadow: "0 24px 60px rgba(0,0,0,.5)",
      }}
    >
      <svg
        viewBox={`${CAMERA.x} ${CAMERA.y} ${SCREEN_W / ZOOM} ${SCREEN_H / ZOOM}`}
        style={{ display: "block", width: "100%", height: "auto", fontFamily: "var(--sans)" }}
        role="img"
        aria-label="The board: three tabs as frames, their panes as cards"
      >
        <defs>
          <pattern id="board-dots" width={GRID} height={GRID} patternUnits="userSpaceOnUse">
            <circle cx={GRID / 2} cy={GRID / 2} r={1 / ZOOM} fill="color-mix(in srgb, var(--muted) 35%, transparent)" />
          </pattern>
        </defs>
        <rect x={CAMERA.x} y={CAMERA.y} width={SCREEN_W / ZOOM} height={SCREEN_H / ZOOM} fill="url(#board-dots)" />
        {frames.map(({ frame, x, cards, h }) => (
          <g key={frame.title} transform={`translate(${x} 0)`}>
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
            {cards.map((c, i) => (
              <CardView key={i} {...c} />
            ))}
          </g>
        ))}
      </svg>
      {/* on screen, not on the board: they keep their size at any zoom */}
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
        <span>{Math.round(ZOOM * 100)}%</span>
        {["Tabs", "New frame", "Fit"].map((label) => (
          <span
            key={label}
            style={{
              borderRadius: 5,
              background: "var(--bg)",
              color: "var(--text)",
              padding: "3px 10px",
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </span>
        ))}
      </div>
    </div>
  );
}

export default function BoardShowcase() {
  return (
    <section
      id="board"
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
          BOARD
        </span>
        <h2 style={{ fontSize: 36, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>
          Every pane, on one canvas
        </h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, marginTop: 16 }}>
          Every tab becomes a frame and every pane a card. Drag the empty board to pan,
          pinch to zoom, and double-click a card’s header to fly to it at 100%. A
          frame’s + adds a pane to it (its Agent wall item opens the wall in a frame of
          its own); New frame adds a tab. Where a card sits on the board and how its tab
          is split are two separate layouts. At 100% the pointer reaches into the cards,
          and a browser page shows while its card is still, wholly in view and nothing
          (palette, panel, menu) is open over it. Any other zoom is an overview, though
          typing still goes to the focused pane.
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
            mira view board
          </span>
          <span>or</span>
          <BothKeys mac="⌘⇧O" other="Ctrl+Shift+O" />
          <span style={{ color: "var(--muted)", fontSize: 12.5 }}>
            or the sidebar’s Tabs | Board switch
          </span>
        </div>
      </div>
      <BoardMockup />
    </section>
  );
}
