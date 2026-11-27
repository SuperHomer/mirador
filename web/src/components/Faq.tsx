import { GITHUB_URL } from "../config";

// Every answer here restates README.md, docs/AGENTS.md or CLAUDE.md. A
// claim with no source there is a promise the app may not keep, so an
// answer changes when those do. Native <details> keeps it static: no state,
// and every answer is in the page for search and for find-in-page.

function Code({ children }: { children: React.ReactNode }) {
  return (
    <span className="mono" style={{ color: "var(--text)", fontSize: "0.92em" }}>
      {children}
    </span>
  );
}

type Item = { q: string; a: React.ReactNode };

const ITEMS: Item[] = [
  {
    q: "Will macOS let me open it?",
    a: (
      <>
        Yes. Release builds are signed with a Developer ID and notarized by Apple, so the first launch only asks
        you to confirm opening an app downloaded from the internet — click <b>Open</b>. The download is for Apple
        silicon. A build you make yourself is only ad-hoc signed, and macOS won't stop it, since a local build is
        never quarantined.
      </>
    ),
  },
  {
    q: "Does it run on Windows?",
    a: (
      <>
        Yes, as a beta. The installer sets it up per user, so there is no admin prompt, and on Windows 10 it fetches
        WebView2. Windows is built by CI but has not yet been driven by hand. Notifications only appear once the app
        is installed, only Claude Code's hooks make an agent there (other harnesses aren't spotted from the process
        table), and agents on a remote computer are not done yet.
      </>
    ),
  },
  {
    q: "What about Linux?",
    a: (
      <>
        There is no packaged build yet, but it should build from source — <a href="#contribute">here's how</a>.
        WebKitGTK rendering and the browser pane's placement have not been verified there.
      </>
    ),
  },
  {
    q: "What happens to my terminals when I quit?",
    a: (
      <>
        They keep running — shells, builds, dev servers and agents — and the next launch reattaches to them, with
        what they printed meanwhile. To end them, close the pane, or quit with <i>App: Quit and End All Sessions</i>{" "}
        (<Code>mira quit --end-sessions</Code>). After a reboot there is nothing to reattach: layout, directories and
        scrollback come back, and command and SSH panes return idle until you press a key. Set{" "}
        <Code>"persistSessions": false</Code> in <Code>mirador.json</Code> to have quitting end everything.
      </>
    ),
  },
  {
    q: "Which coding agents does it work with?",
    a: (
      <>
        Claude Code gets the most: tabs that light up when it needs you, per-pane session resume, the{" "}
        <Code>/mira-diff</Code> and <Code>mira</Code> skills, and your plan's limits. Codex, Pi and OpenCode light
        their tabs when a turn ends once you run <Code>mira hooks setup</Code>. Gemini, Amp, Cursor Agent, Copilot,
        Aider, Goose, Qwen, Crush and Droid are recognized from the pane's process table, on macOS and Linux.
      </>
    ),
  },
  {
    q: "What does running a team cost?",
    a: (
      <>
        Mirador itself is free; the agents use your own Claude or Codex login. Before you start a team,{" "}
        <Code>mira team cost &lt;team&gt;</Code> estimates what the prompts Mirador gives its members cost at
        Anthropic's list prices — a lower bound, since the conversation and output aren't counted, and a member on Codex or on a model with no known price is left out. It warns above{" "}
        <Code>teamCostWarnUsd</Code> ($0.50 by default). On a Claude subscription, the costs shown are what the work
        would have cost at API prices, not what you are billed.
      </>
    ),
  },
  {
    q: "Can I work on a remote machine?",
    a: (
      <>
        Yes, over the system <Code>ssh</Code>, so ssh-agent auth, 2FA and ProxyJump all work. <Code>mira ssh open
        &lt;host&gt;</Code> opens a pane there, a split is another shell on it, and saved computers let agents and
        whole teams run there (a Linux computer, x86_64 or aarch64, for now) with their tabs here. Port forwarding rides ssh's ControlMaster; Windows' OpenSSH has
        none, so a password-based host asks you to log in again for each forward and split.
      </>
    ),
  },
  {
    q: "Are browser panes safe?",
    a: (
      <>
        A page in a browser pane gets no access to the app's commands, and a pane never loads the app's own pages,
        whether typed, opened by the page or reached by a redirect. Panes share one cookie store of their own, so a
        sign-in lasts across panes and relaunches until the cookie expires; a session cookie ends with the app.
        Mirador's own pages never render remote content as HTML, and their links, only <Code>http(s)</Code> ones,
        open in your real browser.
      </>
    ),
  },
  {
    q: "Is it free?",
    a: (
      <>
        Yes, and open source under the <a href={`${GITHUB_URL}/blob/main/LICENSE`}>MIT license</a>. It is an independent implementation
        inspired by cmux, using none of its code.
      </>
    ),
  },
];

export default function Faq() {
  return (
    <section id="faq" style={{ padding: "66px 48px", borderTop: "1px solid var(--surface)" }}>
      <div style={{ textAlign: "center", marginBottom: 34 }}>
        <span className="mono" style={{ fontSize: 12, letterSpacing: ".14em", color: "var(--accent)" }}>FAQ</span>
        <h2 style={{ fontSize: 34, fontWeight: 750, letterSpacing: "-.02em", marginTop: 10 }}>Before you ask</h2>
        <p style={{ color: "var(--subtext)", fontSize: 15, lineHeight: 1.65, maxWidth: 560, margin: "14px auto 0", textWrap: "pretty" }}>
          What people want to know before they install — and the first time they quit.
        </p>
      </div>

      <div style={{ maxWidth: 760, margin: "0 auto", display: "flex", flexDirection: "column", gap: 10 }}>
        {ITEMS.map((item) => (
          <details
            key={item.q}
            className="faq-item"
            style={{ background: "var(--bg-alt)", border: "1px solid var(--surface)", borderRadius: 12 }}
          >
            <summary style={{ padding: "15px 20px", fontSize: 15, fontWeight: 650, cursor: "pointer" }}>
              {item.q}
            </summary>
            <p style={{ padding: "0 20px 17px", fontSize: 14, lineHeight: 1.65, color: "var(--subtext)", textWrap: "pretty" }}>
              {item.a}
            </p>
          </details>
        ))}
      </div>
    </section>
  );
}
