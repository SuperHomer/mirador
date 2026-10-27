/**
 * The board, as a static mock: the board's own drawing (BoardCanvas), on
 * a camera of its own.
 */

import BoardCanvas, { type Frame } from "./BoardCanvas";
import { BothKeys } from "./Keys";

const ZOOM = 0.6;
/** The mock's screen, in pixels. */
const SCREEN_W = 940;
const SCREEN_H = 500;
const CAMERA = { x: -55, y: -60 };

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

function BoardMockup() {
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
      <BoardCanvas
        frames={FRAMES}
        zoom={ZOOM}
        camera={CAMERA}
        screen={{ w: SCREEN_W, h: SCREEN_H }}
        dotsId="board-dots"
        label="The board: three tabs as frames, their panes as cards"
      />
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
          Every tab becomes a frame and every pane a card — or, for a tab showing its
          wall, its frame shows the wall: its agents’ live tiles, or its task board.
          Each frame’s title carries its agents’ status and its view; a frame where
          someone waits on you has a peach edge, and the corner counts them and flies
          you to each in turn. Drag the empty board to pan,
          pinch to zoom, and double-click a card’s header to fly to it at 100%. A
          frame’s + adds a pane to it; New frame adds a tab. Where a card sits on the board and how its tab
          is split are two separate layouts. From 50% up the pointer reaches into the
          cards — click, select text, use the mouse in a program — and at 100% a browser
          page shows while its card is still, wholly in view and nothing (palette, panel,
          menu) is open over it. Away from that, a card shows a recent picture of its
          page, refreshed after it navigates and every few seconds while the card and the
          window are visible — on macOS;
          Windows tries, keeping the last picture or the URL when it can’t, and elsewhere
          the card shows the URL. Below 50% is an overview, where a click focuses a card,
          though typing still goes to the focused pane. At any zoom the wheel scrolls the
          focused card’s content, unless that is a browser away from 100%, and pans the
          board anywhere else, other cards included; hold <BothKeys mac="⌥" other="Alt" /> to
          pan over the focused card too — all but a browser page at 100%, which keeps its
          own scroll.
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
            or Board in the sidebar’s switch
          </span>
        </div>
      </div>
      <BoardMockup />
    </section>
  );
}
