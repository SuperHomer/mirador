/**
 * The app's agent chrome, as the mocks draw it: status marks, the sidebar,
 * an agent's status bar and the wall in Focus. Shared by the hero's mock
 * and the teams section, which draw the same sidebar.
 *
 * Kept honest to the app (src/sidebar/Sidebar.tsx, src/agents/StatusMark.tsx,
 * AgentStatusBar.tsx and FocusLayout.tsx, and their rules in styles.css):
 * the view switch at the top, the current tab's agents nested under it
 * with their mark, name, a grey harness mark, how long in their status and,
 * for one waiting on you, what it asks; any other tab's agents as pips; and
 * the footer's + New tab, Inbox with its count, peach while anyone waits,
 * and Commands. Status, what an agent waits on and its context are Claude
 * Code's: a Codex agent's mark is the muted ring of a status not known,
 * and its status bar says what it does not report.
 */

import TermLines, { type Line } from "./TermLines";

export type Status = "working" | "needsYou" | "idle" | null;
export type Harness = "claude" | "codex";

export type MockAgent = {
  name: string;
  harness: Harness;
  status: Status;
  /** How long in its status: "38s", "2m". */
  since?: string;
  /** What it waits on, when it needs you. */
  asking?: string;
  model: string;
  effort?: string;
  /** How full its context is, in percent; Claude Code's only. */
  context?: number;
  lines: Line[];
};

export type MockTab = {
  title: string;
  team?: boolean;
  agents: MockAgent[];
  branch?: string;
  pr?: { number: number; checks: "pass" | "pending" };
  port?: number;
  /** A notification in it not read yet. */
  unread?: boolean;
};

const PR = {
  pass: { mark: " ✓", className: "pass" },
  pending: { mark: " ●", className: "pending" },
};

export type View = "splits" | "wall" | "tasks" | "board";

const STATUS_WORD: Record<NonNullable<Status>, string> = { working: "working", needsYou: "needs you", idle: "idle" };
const STATUS_CLASS: Record<NonNullable<Status>, string> = { working: "working", needsYou: "needs-you", idle: "idle" };

/** A "!" square when it needs you, a pulsing dot while it works, a ring
 *  when idle, a muted ring when not known. */
export function Mark({ status, small }: { status: Status; small?: boolean }) {
  const label = status ? STATUS_WORD[status] : "status unknown";
  return (
    <span className={`ms-mark ${status ? STATUS_CLASS[status] : "unknown"}${small ? " small" : ""}`} role="img" aria-label={label} title={label}>
      {status === "needsYou" ? "!" : null}
    </span>
  );
}

export function Pip({ status }: { status: Status }) {
  return <span className={`ms-pip ${status ? STATUS_CLASS[status] : "unknown"}`} />;
}

/** The harnesses' marks, grey, after an agent's name. */
const HARNESS_MARK: Record<Harness, string> = { claude: "✳", codex: ">_" };
const HARNESS_NAME: Record<Harness, string> = { claude: "Claude", codex: "Codex" };

/** How it stands, in its status colour: "needs you · 2m". */
function standing(a: MockAgent) {
  if (!a.status) return null;
  return (
    <span className={`ms-standing ${STATUS_CLASS[a.status]}`}>
      {STATUS_WORD[a.status]}
      {a.since && ` · ${a.since}`}
    </span>
  );
}

/**
 * The sidebar: the view switch, the tabs — the current one's agents under
 * it — and the footer. `onView` and `onTab` make it clickable; a view not
 * in `views` is drawn but not offered.
 */
export function MockSidebar({
  tabs,
  current,
  view,
  views = ["splits", "wall", "tasks", "board"],
  onView,
  onTab,
  className = "",
}: {
  tabs: MockTab[];
  current: number;
  view: View;
  views?: View[];
  onView?: (v: View) => void;
  onTab?: (i: number) => void;
  className?: string;
}) {
  const waiting = tabs.flatMap((t) => t.agents).filter((a) => a.status === "needsYou").length;
  return (
    <div className={`ms-sidebar ${className}`}>
      <div className="ms-switch">
        {(["splits", "wall", "tasks", "board"] as const).map((v) => (
          <button
            key={v}
            className="mock-view-switch"
            aria-pressed={view === v}
            disabled={!onView || !views.includes(v)}
            onClick={() => onView?.(v)}
          >
            {v[0].toUpperCase() + v.slice(1)}
          </button>
        ))}
      </div>
      <div className="ms-tabs">
        {tabs.map((t, i) => (
          <div key={t.title} className="ms-tab-block">
            <button className="mock-tab ms-tab" aria-pressed={i === current} onClick={() => onTab?.(i)} tabIndex={onTab ? 0 : -1}>
              <span className="ms-tab-index">{i + 1}</span>
              <span className="ms-tab-text">
                <span className="ms-tab-title">
                  {t.team && <span className="ms-team">⚑</span>}
                  {t.title}
                </span>
                {t.branch && (
                  <span className="ms-tab-chips">
                    <span className="ms-branch">⎇ {t.branch}</span>
                    {t.pr && (
                      <span className={`ms-chip-pr ${PR[t.pr.checks].className}`}>
                        #{t.pr.number}
                        {PR[t.pr.checks].mark}
                      </span>
                    )}
                    {t.port && <span className="ms-chip-port">:{t.port}</span>}
                  </span>
                )}
              </span>
              {i !== current && t.agents.length > 0 && (
                <span className="ms-pips">
                  {t.agents.map((a) => (
                    <Pip key={a.name} status={a.status} />
                  ))}
                </span>
              )}
              {t.unread && <span className="ms-badge">1</span>}
            </button>
            {i === current && t.agents.length > 0 && (
              <div className="ms-tree">
                {t.agents.map((a) => (
                  <div key={a.name} className={`ms-agent${a.status === "needsYou" ? " needs-you" : a.status === "idle" ? " idle" : ""}`}>
                    <span className="ms-agent-line">
                      <span className="ms-agent-mark">
                        <Mark status={a.status} />
                      </span>
                      <span className="ms-agent-name">{a.name}</span>
                      <span className="ms-harness">{HARNESS_MARK[a.harness]}</span>
                      {standing(a)}
                    </span>
                    {a.status === "needsYou" && a.asking && <span className="ms-asking">{a.asking}</span>}
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="ms-footer">
        <span className="ms-new-tab">+ New tab</span>
        <span className={`ms-button${waiting > 0 ? " waiting" : ""}`}>Inbox {waiting}</span>
        <span className="ms-button">Commands</span>
      </div>
    </div>
  );
}

/** The foot of an agent's tile or pane: what it runs and how full its
 *  context is — or, from a harness that does not say, that it does not. */
export function StatusBar({ agent, compact }: { agent: MockAgent; compact?: boolean }) {
  const meter = agent.context !== undefined && (
    <span className="ms-context">
      <span className={`ms-meter ${agent.context < 50 ? "low" : agent.context <= 80 ? "mid" : "high"}`}>
        <span style={{ width: `${agent.context}%` }} />
      </span>
      <span className="ms-pct">{agent.context}%</span>
    </span>
  );
  if (compact) {
    return (
      <div className="ms-bar compact">
        <span className="ms-bar-model">{agent.model}</span>
        {meter}
      </div>
    );
  }
  return (
    <div className="ms-bar">
      <span className="ms-chip">{HARNESS_NAME[agent.harness]}</span>
      <span className="ms-chip">{agent.model}</span>
      {agent.effort && <span className="ms-chip outline">effort {agent.effort}</span>}
      {agent.context !== undefined ? (
        <>
          <span className="ms-bar-label">Context</span>
          {meter}
        </>
      ) : (
        <span className="ms-bar-label">Context not reported</span>
      )}
      {agent.harness === "claude" && <span className="ms-subagents">Subagents ▾</span>}
    </div>
  );
}

/**
 * A tab's wall in Focus: the agent that most wants you large, with its
 * status bar; the rest in a rail beside it, a few lines of each.
 */
export function FocusWall({ agents, large }: { agents: MockAgent[]; large: number }) {
  const lead = agents[large];
  const counts = (s: NonNullable<Status>) => agents.filter((a) => a.status === s).length;
  return (
    <div className="ms-wall">
      <div className="ms-wall-toolbar">
        <span className="ms-filter selected">All {agents.length}</span>
        {(["needsYou", "working", "idle"] as const).map((s) => (
          <span key={s} className={`ms-filter ${STATUS_CLASS[s]}`}>
            {STATUS_WORD[s][0].toUpperCase() + STATUS_WORD[s].slice(1)} {counts(s)}
          </span>
        ))}
        <span className="ms-segmented">
          <span className="selected">Focus</span>
          <span>Grid</span>
        </span>
      </div>
      <div className="ms-wall-body">
        <div className={`ms-tile large${lead.status === "needsYou" ? " needs-you" : ""}`}>
          <div className="ms-tile-head">
            <Mark status={lead.status} />
            <span className="ms-tile-name">{lead.name}</span>
            {standing(lead)}
          </div>
          {lead.status === "needsYou" && lead.asking && <div className="ms-tile-asking">{lead.asking}</div>}
          <div className="ms-tile-term mono">
            <TermLines lines={lead.lines} cursor />
          </div>
          <StatusBar agent={lead} />
        </div>
        <div className="ms-rail">
          {agents.map((a, i) =>
            i === large ? null : (
              <div key={a.name} className={`ms-tile mini${a.status === "needsYou" ? " needs-you" : ""}`}>
                <div className="ms-tile-head">
                  <Mark status={a.status} small />
                  <span className="ms-tile-name">{a.name}</span>
                  <span className="ms-harness">{HARNESS_MARK[a.harness]}</span>
                  {a.since && a.status && <span className={`ms-since ${STATUS_CLASS[a.status]}`}>{a.since}</span>}
                </div>
                <div className="ms-tile-term mono">
                  <TermLines lines={a.lines.slice(-3)} />
                </div>
                <StatusBar agent={a} compact />
              </div>
            ),
          )}
        </div>
      </div>
    </div>
  );
}
