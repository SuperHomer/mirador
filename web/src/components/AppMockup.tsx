/**
 * The hero's Mirador, as a mock a visitor can click around: three tabs in
 * the sidebar, each with its own panes. No shell runs behind it.
 *
 * Kept honest to the app: a tab row draws what the sidebar's does (index,
 * ⚑ for a team, cwd or the last notification, ⎇ branch, PR and port chips,
 * ✳ agent count, unread badge — src/sidebar/Sidebar.tsx), a focused pane
 * has the brighter edge and an agent that wants you the accent ring
 * (.pane.focused, .pane.unread in the app's styles.css), and focusing that
 * pane clears its ring, as it does in the app. The omakase tab's columns
 * are the teams section's, line for line.
 *
 * At phone width three columns would each be a few characters wide, so
 * the team's tab shows its focused member alone, as pane zoom does, and the
 * agents row under the tabs picks which, as the sidebar's agent list does.
 *
 * The sidebar's Tabs | Board switch shows the same tabs as frames on the
 * board, fitted as the app's Fit fits them (cmux-core's board::fit) — on a
 * phone, where Fit would draw them too small to read, centred on the
 * current tab's frame instead (board::centre_on) — and
 * a click on a card flies back to its pane, as double-clicking a card's
 * header does in the app.
 *
 * Under it, a line types real `mira` commands and the mock does what the
 * app would: `team start` opens the team's tab after the active one and
 * focuses its lead (cmux-core's new_team_tab), its frame going right of
 * the others on the board (board::next_frame_origin); `agent send` from a
 * shell outside any pane finds the worker in the focused pane's tab, types
 * the message without a "[message from …]" line, since no agent sent it,
 * and moves no focus; `view board` and `view tabs` flip the view. A click
 * or a key on the mock stops it for good, short of replay.
 */

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import BoardCanvas, { FRAME_GAP, FRAME_W, layout, type Card, type Frame } from "./BoardCanvas";
import { MARK, MEMBERS } from "./TeamsShowcase";
import TermLines, { type Line } from "./TermLines";

type Pane = {
  flex?: number;
  lines?: Line[];
  browser?: string;
  attention?: boolean;
  agent?: (typeof MEMBERS)[number];
  /** Its card's title on the board, when not the agent's or the tab's. */
  title?: string;
  status?: Card["status"];
};
type Tab = {
  title: string;
  team?: boolean;
  cwd: string;
  /** Shown in place of the cwd while the tab has an unread pane. */
  notification?: string;
  branch: string;
  pr?: { number: number; checks: "pass" | "pending" };
  port?: number;
  agents?: number;
  panes: Pane[];
  /** The pane focused when the page loads. */
  focus: number;
  /** Its frame's place on the board, counted from the left. */
  slot: number;
};

/** How far the demo has brought the team: not started yet, started, the
    worker sent its step, or as the teams section shows it. */
type Team = "absent" | "started" | "sent" | "full";

const PR = {
  pass: { mark: " ✓", color: "var(--green)", bg: "rgba(166,227,161,.25)" },
  pending: { mark: " ●", color: "var(--yellow)", bg: "rgba(249,226,175,.25)" },
};

const TABS: Tab[] = [
  {
    title: "api",
    cwd: "~/work/api",
    notification: "Claude needs your permission",
    branch: "feat/auth",
    pr: { number: 42, checks: "pass" },
    port: 3000,
    agents: 1,
    focus: 1,
    slot: 0,
    panes: [
      {
        attention: true,
        title: "claude",
        status: "needsYou",
        lines: [
          { text: "> wire up the refresh flow", color: "var(--text)" },
          { text: "⏺ Read(src/auth/refresh.ts)" },
          { text: "⏺ Update(src/auth/refresh.ts)" },
          { text: "  ⎿ Added 18 lines, removed 3", color: "var(--muted)" },
          { text: "⏺ Bash(npm run migrate)" },
          { text: "Do you want to proceed?", color: "var(--text)" },
          { text: "❯ 1. Yes", color: "var(--accent)" },
          { text: "  2. No, and tell Claude what to do differently" },
        ],
      },
      {
        lines: [
          { text: "➜ api mira run --wait npm test", color: "var(--green)" },
          { text: "  PASS  auth.spec.ts (1.2s)" },
          { text: "  PASS  token.spec.ts (0.4s)" },
          { text: "  PASS  refresh.spec.ts (0.9s)" },
          { text: "  42 passing", color: "var(--green)" },
          { text: "— exit 0 —", color: "var(--muted)" },
          { text: "➜ api ", color: "var(--green)" },
        ],
      },
    ],
  },
  {
    title: "omakase",
    team: true,
    cwd: "~/work/api",
    branch: "feat/oauth",
    agents: MEMBERS.length,
    focus: 0,
    // Started last, so its frame went right of the others.
    slot: 2,
    panes: MEMBERS.map((m, i) => ({ lines: m.lines, agent: m, status: i === 0 ? "working" : "idle" })),
  },
  {
    title: "web",
    cwd: "~/work/web",
    branch: "main",
    pr: { number: 17, checks: "pending" },
    port: 5173,
    focus: 1,
    slot: 1,
    panes: [
      {
        flex: 0.85,
        lines: [
          { text: "➜ web npm run dev", color: "var(--green)" },
          { text: "  VITE v8.3.1  ready in 212 ms", color: "var(--green)" },
          { text: "  ➜  Local:   http://localhost:5173/", color: "var(--text)" },
          { text: "  ➜  press h + enter to show help", color: "var(--muted)" },
          { text: "10:42:07 [vite] hmr update /src/Hero.tsx" },
          { text: "10:42:31 [vite] hmr update /src/Hero.tsx" },
        ],
      },
      { flex: 1.15, browser: "localhost:5173" },
    ],
  },
];

const TEAM = TABS.findIndex((t) => t.team);
const TASK = "rotate refresh tokens";
const STEP = "Step 2: rotate the refresh token on use";

/** The team's panes just after `team start`: the lead has the task. */
const STARTED: Line[][] = [
  [
    { text: `> ${TASK}`, color: "var(--text)" },
    { text: "✶ Planning…", color: "var(--accent)" },
  ],
  [{ text: "> ", color: "var(--muted)" }],
  [{ text: "› ", color: "var(--muted)" }],
];
/** The worker once a person sent it the step, and it did it. */
const SENT: Line[] = [{ text: `> ${STEP}`, color: "var(--text)" }, ...MEMBERS[1].lines.slice(2)];

function teamLines(team: Team, pane: number): Line[] {
  if (team === "full" || team === "absent") return MEMBERS[pane].lines;
  return team === "sent" && pane === 1 ? SENT : STARTED[pane];
}

/** Per character, give or take half of JITTER; then the pauses, in ms. */
const TYPE_MS = 45;
const JITTER = 30;
const AFTER_COMMAND = 1600;
const AFTER_SCRIPT = 4000;

export default function AppMockup() {
  const reduced = useState(() => matchMedia("(prefers-reduced-motion: reduce)").matches)[0];
  const [current, setCurrent] = useState(0);
  const [focus, setFocus] = useState(TABS.map((t) => t.focus));
  // The one pane with an attention ring loses it once it is focused.
  const [attended, setAttended] = useState(false);
  const [view, setView] = useState<"tabs" | "board">("tabs");
  // The demo starts before the team does; without it, the team is there.
  const [team, setTeam] = useState<Team>(reduced ? "full" : "absent");
  const tab = TABS[current];
  // TABS' indices, of the tabs the sidebar shows.
  const shown = TABS.flatMap((t, i) => (t.team && team === "absent" ? [] : [i]));

  const script = [
    {
      command: `mira team start omakase "${TASK}"`,
      run: () => {
        setTeam("started");
        setCurrent(TEAM);
        setFocus((f) => f.map((p, i) => (i === TEAM ? 0 : p)));
      },
    },
    { command: `mira agent send worker "${STEP}" --wait`, run: () => setTeam("sent") },
    { command: "mira view board", run: () => setView("board") },
    { command: "mira view tabs", run: () => setView("tabs") },
  ];
  const reset = () => {
    setCurrent(0);
    setFocus(TABS.map((t) => t.focus));
    setAttended(false);
    setView("tabs");
    setTeam("absent");
    setAnnounced("");
  };

  // Where the demo is, or null once it stopped (or, under reduced motion,
  // never started).
  const [demo, setDemo] = useState<{ step: number; typed: number } | null>(reduced ? null : { step: 0, typed: 0 });
  const [announced, setAnnounced] = useState("");
  const [inView, setInView] = useState(false);
  const [pageShown, setPageShown] = useState(!document.hidden);
  const mockRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const observer = new IntersectionObserver(([entry]) => setInView(entry.isIntersecting), { threshold: 0.3 });
    observer.observe(mockRef.current!);
    const onVisibility = () => setPageShown(!document.hidden);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  // One timer at a time: the next character, the pause after a command,
  // or the pause before starting over. None while the mock is out of view.
  useEffect(() => {
    if (!demo || !inView || !pageShown) return;
    const { command, run } = script[demo.step];
    let next: () => void;
    let delay: number;
    if (demo.typed < command.length) {
      delay = TYPE_MS + (Math.random() - 0.5) * JITTER;
      next = () => {
        if (demo.typed + 1 === command.length) {
          run();
          setAnnounced(command);
        }
        setDemo({ ...demo, typed: demo.typed + 1 });
      };
    } else if (demo.step < script.length - 1) {
      delay = AFTER_COMMAND;
      next = () => setDemo({ step: demo.step + 1, typed: 0 });
    } else {
      delay = AFTER_SCRIPT;
      next = () => {
        reset();
        setDemo({ step: 0, typed: 0 });
      };
    }
    const timer = setTimeout(next, delay);
    return () => clearTimeout(timer);
  }, [demo, inView, pageShown]);

  // The visitor's click or key takes the mock over.
  const takeOver = () => demo && setDemo(null);

  // From the board, back to tabs on that tab, with that pane focused.
  const fly = (t: number, pane?: number) => {
    setCurrent(t);
    if (pane !== undefined) {
      setFocus(focus.map((f, i) => (i === t ? pane : f)));
      if (TABS[t].panes[pane].attention) setAttended(true);
    }
    setView("tabs");
  };

  const frames: Frame[] = shown.map((ti) => {
    const t = TABS[ti];
    return {
      title: t.title,
      current: ti === current,
      x: t.slot * (FRAME_W + FRAME_GAP),
      cards: t.panes.map((pane, pi) => ({
        title: pane.title ?? pane.agent?.name ?? (pane.browser ? `http://${pane.browser}/` : t.title),
        h: CARD_H,
        focused: ti === current && focus[ti] === pi,
        status: pane.status,
        url: pane.browser && `http://${pane.browser}/`,
        lines: t.team ? teamLines(team, pi) : pane.lines,
      })),
    };
  });

  return (
    <div ref={mockRef} style={{ marginTop: 56, textAlign: "left" }}>
      <div
        onPointerDownCapture={takeOver}
        onKeyDownCapture={takeOver}
        style={{
          maxWidth: 1000,
          margin: "0 auto",
          border: "1px solid var(--surface)",
          borderRadius: 12,
          overflow: "hidden",
          background: "var(--bg)",
          boxShadow: "0 24px 60px rgba(0,0,0,.5)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "9px 14px",
            background: "var(--bg-alt)",
            borderBottom: "1px solid var(--surface)",
          }}
        >
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--red)" }} />
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--yellow)" }} />
          <span style={{ width: 11, height: 11, borderRadius: "50%", background: "var(--green)" }} />
          <span className="mono" style={{ marginLeft: 12, fontSize: 12, color: "var(--muted)" }}>
            mirador — {tab.cwd}
          </span>
        </div>
        <div className="mock-body" style={{ display: "flex", height: 396 }}>
          <div
            className="mock-sidebar"
            style={{
              width: 216,
              flexShrink: 0,
              background: "var(--bg-alt)",
              borderRight: "1px solid var(--surface)",
              display: "flex",
              flexDirection: "column",
              padding: 6,
              gap: 2,
            }}
          >
            {shown.map((i, n) => {
              const t = TABS[i];
              const unread = !!t.notification && !attended;
              return (
                <button
                  key={t.title}
                  className="mock-tab"
                  aria-pressed={i === current}
                  onClick={() => setCurrent(i)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    padding: "6px 8px",
                    borderRadius: 6,
                    minHeight: 40,
                    border: "none",
                    textAlign: "left",
                    color: "var(--text)",
                    background: i === current ? "var(--surface)" : "transparent",
                  }}
                >
                  <span className="mock-tab-index" style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>{n + 1}</span>
                  <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 1 }}>
                    <span style={{ fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", color: unread ? "var(--accent)" : undefined }}>
                      {t.team && <span style={{ color: "var(--accent)", marginRight: 5, fontSize: 11 }}>⚑</span>}
                      {t.title}
                    </span>
                    <span
                      className="mock-tab-detail"
                      style={{ fontSize: 10.5, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", color: unread ? "var(--accent)" : "var(--muted)" }}
                    >
                      {unread ? t.notification : t.cwd}
                    </span>
                    <span className="mock-tab-detail" style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 2 }}>
                      <span style={{ fontSize: 10, color: "var(--subtext)", whiteSpace: "nowrap" }}>⎇ {t.branch}</span>
                      {t.pr && (
                        <span style={chip(PR[t.pr.checks].bg, PR[t.pr.checks].color)}>
                          #{t.pr.number}
                          {PR[t.pr.checks].mark}
                        </span>
                      )}
                      {t.port && <span style={chip("var(--surface)", "var(--accent)")}>:{t.port}</span>}
                    </span>
                  </span>
                  {t.agents && (
                    <span
                      className="mock-tab-detail"
                      style={{
                        fontSize: 10.5,
                        lineHeight: "16px",
                        padding: "0 6px",
                        borderRadius: 8,
                        whiteSpace: "nowrap",
                        border: "1px solid rgba(217,119,87,.4)",
                        ...(unread
                          ? { background: "#d97757", color: "var(--bg-alt)", fontWeight: 700 }
                          : { color: "#d97757" }),
                      }}
                    >
                      ✳ {t.agents}
                    </span>
                  )}
                  {unread && (
                    <span style={{ background: "var(--accent)", color: "var(--bg-alt)", fontSize: 10, fontWeight: 700, borderRadius: 8, minWidth: 16, height: 16, lineHeight: "16px", textAlign: "center" }}>
                      1
                    </span>
                  )}
                </button>
              );
            })}
            <div className="mock-sidebar-footer" style={{ marginTop: "auto", display: "flex", alignItems: "center" }}>
              <span className="mock-new-tab" style={{ flex: 1, padding: "7px 8px", fontSize: 12, color: "var(--subtext)" }}>
                + New Tab
              </span>
              <div style={{ display: "flex", border: "1px solid var(--surface)", borderRadius: 6, overflow: "hidden" }}>
                {(["tabs", "board"] as const).map((v) => (
                  <button
                    key={v}
                    className="mock-view-switch"
                    aria-pressed={view === v}
                    onClick={() => setView(v)}
                    style={{
                      border: "none",
                      fontSize: 11,
                      padding: "3px 8px",
                      background: view === v ? "var(--surface)" : "transparent",
                      color: view === v ? "var(--text)" : "var(--subtext)",
                    }}
                  >
                    {v === "tabs" ? "Tabs" : "Board"}
                  </button>
                ))}
              </div>
            </div>
          </div>
          {/* keyed by the view, so each switch replays the view's entrance */}
          <div key={view} className="mock-view" style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
            {view === "board" ? (
              <BoardView frames={frames} onCard={(f, c) => fly(shown[f], c)} onTabs={() => setView("tabs")} />
            ) : (
            <>
              {tab.team && (
                <div className="mock-agents" style={{ display: "none", gap: 4, padding: "6px 6px 0" }}>
                  {tab.panes.map((pane, i) =>
                    pane.agent ? (
                      <button
                        key={pane.agent.name}
                        className="mock-tab"
                        aria-pressed={focus[current] === i}
                        onClick={() => setFocus(focus.map((f, t) => (t === current ? i : f)))}
                        style={{
                          flex: 1,
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                          gap: 6,
                          padding: "5px 8px",
                          borderRadius: 6,
                          border: "none",
                          fontSize: 12.5,
                          color: "var(--text)",
                          background: focus[current] === i ? "var(--surface)" : "transparent",
                        }}
                      >
                        <span className="mono" style={{ fontSize: 11, color: MARK[pane.agent.harness].color }}>
                          {MARK[pane.agent.harness].text}
                        </span>
                        {pane.agent.name}
                      </button>
                    ) : null,
                  )}
                </div>
              )}
              <div
                className={`mock-panes${tab.team ? " mock-zoomable" : ""}`}
                style={{ flex: 1, display: "flex", gap: 6, padding: 6, minWidth: 0, minHeight: 0 }}
              >
                {tab.panes.map((pane, i) => {
                  const focused = focus[current] === i;
                  const ring = pane.attention && !attended;
                  return (
                    <div
                      key={`${current}-${i}`}
                      className={`mock-pane${focused ? " focused" : ""}`}
                      tabIndex={0}
                      onFocus={() => {
                        setFocus(focus.map((f, t) => (t === current ? i : f)));
                        if (pane.attention) setAttended(true);
                      }}
                      style={{
                        flex: pane.flex ?? 1,
                        minWidth: 0,
                        minHeight: 0,
                        borderRadius: 5,
                        overflow: "hidden",
                        border: `1px solid ${ring ? "var(--accent)" : focused ? "var(--muted)" : "var(--surface)"}`,
                        boxShadow: ring ? "0 0 0 1px var(--accent), 0 0 16px rgba(137,180,250,.45)" : undefined,
                        background: pane.browser ? "var(--bg-alt)" : "var(--crust)",
                        display: "flex",
                        flexDirection: "column",
                      }}
                    >
                      {pane.browser ? (
                        <BrowserPane url={pane.browser} />
                      ) : (
                        <div className="mono" style={{ padding: "8px 10px", fontSize: 11, lineHeight: 1.6 }}>
                          <TermLines lines={tab.team ? teamLines(team, i) : (pane.lines ?? [])} cursor={focused} />
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
            )}
          </div>
        </div>
      </div>
      {/* two lines' room, so a command that wraps on a phone moves nothing */}
      <p className="mono" style={{ marginTop: 14, minHeight: "3.2em", lineHeight: 1.6, textAlign: "center", fontSize: 12.5, color: "var(--muted)" }}>
        {demo ? (
          <span style={{ color: "var(--text)" }}>
            <span style={{ color: "var(--muted)" }}>$ </span>
            {script[demo.step].command.slice(0, demo.typed)}
            <span className="cursor" style={{ width: 7, height: 14, verticalAlign: "middle", marginLeft: 1 }} />
          </span>
        ) : reduced ? (
          <span style={{ color: "var(--text)" }}>
            <span style={{ color: "var(--muted)" }}>$ </span>
            {script[0].command}
          </span>
        ) : (
          <>
            {view === "tabs" ? "↑ click a tab or a pane, or flip to Board" : "↑ click a card to fly back to it"}
            {" · "}
            <button
              className="mock-replay"
              onClick={() => {
                reset();
                setDemo({ step: 0, typed: 0 });
              }}
              style={{ border: "none", background: "none", padding: 0, font: "inherit", color: "var(--muted)" }}
            >
              replay ↺
            </button>
          </>
        )}
      </p>
      <span className="visually-hidden" aria-live="polite">
        {announced}
      </span>
    </div>
  );
}

/** A card's height on the board, in board units. */
const CARD_H = 180;
/** Room left around the frames when fitted, in pixels (board::FIT_MARGIN). */
const FIT_MARGIN = 40;
/** Narrower than this, in pixels, the board opens on the current frame. */
const PHONE_W = 600;
/** How much of each neighbouring frame shows then, in board units. */
const PEEK = 40;

/** The board, fitted to the mock as the app's Fit would fit it. */
function BoardView({ frames, onCard, onTabs }: { frames: Frame[]; onCard: (frame: number, card?: number) => void; onTabs: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [screen, setScreen] = useState<{ w: number; h: number } | null>(null);
  useLayoutEffect(() => {
    const el = ref.current!;
    const measure = () => setScreen({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const xs = frames.map((f, i) => f.x ?? i * (FRAME_W + FRAME_GAP));
  const bounds = {
    w: Math.max(...xs) + FRAME_W,
    h: Math.max(...frames.map((f) => layout(f).h)),
  };
  // Never past 100% nor under the board's 10%.
  const clamp = (zoom: number) => Math.min(1, Math.max(0.1, zoom));
  let view: { zoom: number; centre: { x: number; y: number } } | null = null;
  if (screen && screen.w < PHONE_W) {
    // The current frame across the screen, its gaps and a sliver of each
    // neighbour beside it, so the board reads as one you could pan.
    const i = Math.max(0, frames.findIndex((f) => f.current));
    view = {
      zoom: clamp(screen.w / (FRAME_W + 2 * (FRAME_GAP + PEEK))),
      centre: { x: xs[i] + FRAME_W / 2, y: layout(frames[i]).h / 2 },
    };
  } else if (screen) {
    // Every frame in view.
    view = {
      zoom: clamp(Math.min((screen.w - 2 * FIT_MARGIN) / bounds.w, (screen.h - 2 * FIT_MARGIN) / bounds.h)),
      centre: { x: bounds.w / 2, y: bounds.h / 2 },
    };
  }
  return (
    <div ref={ref} style={{ position: "relative", flex: 1, minHeight: 0, overflow: "hidden", background: "var(--bg-alt)" }}>
      {screen && view && (
        <BoardCanvas
          frames={frames}
          zoom={view.zoom}
          camera={{ x: view.centre.x - screen.w / view.zoom / 2, y: view.centre.y - screen.h / view.zoom / 2 }}
          screen={screen}
          dotsId="hero-board-dots"
          label="The board: the same three tabs as frames, their panes as cards"
          onCard={onCard}
          onTabs={onTabs}
        />
      )}
    </div>
  );
}

const chip = (bg: string, color: string): React.CSSProperties => ({
  fontSize: 9.5,
  fontWeight: 600,
  borderRadius: 4,
  padding: "1px 5px",
  whiteSpace: "nowrap",
  background: bg,
  color,
});

/** A browser pane on the dev server's page. The page is someone's app, not
    Mirador, so it keeps its own light colors rather than the theme's. */
function BrowserPane({ url }: { url: string }) {
  return (
    <>
      <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px", borderBottom: "1px solid var(--surface)" }}>
        <span style={{ color: "var(--muted)", fontSize: 13 }}>‹ ›</span>
        <div className="mono" style={{ flex: 1, minWidth: 0, background: "var(--crust)", border: "1px solid var(--surface)", borderRadius: 6, fontSize: 10.5, color: "var(--subtext)", padding: "3px 9px" }}>
          {url}
        </div>
      </div>
      <div style={{ flex: 1, background: "#f7f7fb", color: "#1e1e2e", padding: "14px 18px", overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 14, fontSize: 11, color: "#6c6f85" }}>
          <span style={{ fontWeight: 700, color: "#1e1e2e" }}>◆ acme</span>
          <span style={{ marginLeft: "auto" }}>docs</span>
          <span>pricing</span>
        </div>
        <div style={{ fontSize: 22, fontWeight: 750, letterSpacing: "-.02em", marginTop: 26, lineHeight: 1.15 }}>
          Ship on Friday.
        </div>
        <div style={{ fontSize: 12, color: "#5c5f77", marginTop: 8, lineHeight: 1.5 }}>
          Deploys you can undo, from the branch you are on.
        </div>
        <span style={{ display: "inline-block", marginTop: 16, background: "#1e66f5", color: "#fff", fontSize: 11.5, fontWeight: 600, padding: "6px 12px", borderRadius: 6 }}>
          Get started
        </span>
      </div>
    </>
  );
}
