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
 */

import { useState } from "react";
import { MARK, MEMBERS } from "./TeamsShowcase";
import TermLines, { type Line } from "./TermLines";

type Pane = { flex?: number; lines?: Line[]; browser?: string; attention?: boolean; agent?: (typeof MEMBERS)[number] };
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
};

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
    panes: [
      {
        attention: true,
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
    panes: MEMBERS.map((m) => ({ lines: m.lines, agent: m })),
  },
  {
    title: "web",
    cwd: "~/work/web",
    branch: "main",
    pr: { number: 17, checks: "pending" },
    port: 5173,
    focus: 1,
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

export default function AppMockup() {
  const [current, setCurrent] = useState(0);
  const [focus, setFocus] = useState(TABS.map((t) => t.focus));
  // The one pane with an attention ring loses it once it is focused.
  const [attended, setAttended] = useState(false);
  const tab = TABS[current];

  return (
    <div style={{ marginTop: 56, textAlign: "left" }}>
      <div
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
            {TABS.map((t, i) => {
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
                  <span style={{ fontSize: 10, color: "var(--muted)", width: 12, textAlign: "center" }}>{i + 1}</span>
                  <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 1 }}>
                    <span style={{ fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", color: unread ? "var(--accent)" : undefined }}>
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
            <div className="mock-new-tab" style={{ marginTop: 4, padding: "7px 8px", fontSize: 12, color: "var(--muted)" }}>
              + new tab
            </div>
          </div>
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
                      <TermLines lines={pane.lines ?? []} cursor={focused} />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
      <p className="mono" style={{ marginTop: 14, textAlign: "center", fontSize: 12, color: "var(--muted)" }}>
        ↑ click a tab, or a pane to focus it
      </p>
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
