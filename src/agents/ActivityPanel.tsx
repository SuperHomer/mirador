import { useEffect, useRef, useState } from "react";
import {
  AgentActivity,
  agentLabel,
  SubagentInfo,
  SubagentState,
  TaskProgress,
  getAgentActivity,
  zoomPane,
} from "../bindings";
import { useConfigStore } from "../state/configStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { SPENT_MEANS, spentLabel, spentOf } from "./teamWeight";

/** How often the open panel reads the transcripts again. */
const POLL_MS = 2000;

/** Each subagent type gets one of seven of the theme's hues, the same one
 *  every time: these places in it, so builtin keeps the colours it always
 *  gave (peach, green, blue, yellow, mauve, teal, red). */
const TYPE_HUES = [6, 1, 0, 2, 4, 5, 3];

export function typeColor(type: string, hues: string[]): string {
  let h = 0;
  for (const c of type) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return hues[TYPE_HUES[h % TYPE_HUES.length]] ?? "var(--accent)";
}

export function tokens(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** m:ss, or h:mm:ss past the hour. */
export function duration(fromMs: number | null, toMs: number | null): string {
  if (fromMs == null || toMs == null) return "–";
  const s = Math.max(0, Math.floor((toMs - fromMs) / 1000));
  const mm = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  if (s >= 3600) return `${Math.floor(s / 3600)}:${String(mm).padStart(2, "0")}:${ss}`;
  return `${mm}:${ss}`;
}

/** `claude-opus-5-5` → `Opus 5.5`; anything else as it is. */
export function modelName(model: string | null): string | null {
  if (!model) return null;
  const m = /^claude-([a-z]+)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?$/.exec(model);
  if (!m) return model;
  const family = m[1][0].toUpperCase() + m[1].slice(1);
  return m[3] ? `${family} ${m[2]}.${m[3]}` : `${family} ${m[2]}`;
}

export function percent(tasks: TaskProgress): number {
  return tasks.total === 0 ? 0 : Math.round((tasks.done / tasks.total) * 100);
}

/**
 * `pane`'s activity, read again [`POLL_MS`] after each read returns, while
 * `enabled` — callers pass the agent's `hasActivity`. One read at a time,
 * so a slow one is never overtaken and the bars never jump back; and a
 * failed read keeps the last good one, so a view does not blink out and
 * resize the terminal beside it. Null until the first read succeeds.
 */
export function useAgentActivity(pane: string | null, enabled = true) {
  const [activity, setActivity] = useState<AgentActivity | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    setActivity(null);
    setError(null);
  }, [pane]);

  useEffect(() => {
    if (!pane || !enabled) return;
    let cancelled = false;
    let timer: number | undefined;
    const load = () => {
      void getAgentActivity(pane)
        .then(
          (a) => {
            if (cancelled) return;
            setActivity(a);
            setError(null);
            setNow(Date.now());
          },
          (e) => {
            if (!cancelled) setError(String(e));
          },
        )
        .finally(() => {
          if (!cancelled) timer = window.setTimeout(load, POLL_MS);
        });
    };
    load();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [pane, enabled]);

  return { activity, error, now };
}

/**
 * An agent's activity, opened from the palette's Agents: Show Activity for the
 * focused agent: its own task list, and every subagent it started
 * with what each is on, how far through, and what it used. All of it is
 * read from the transcripts (`cmux_core::transcript`), so it says only
 * what they record — no cost, which would be an estimate.
 */
export function ActivityPanel() {
  const pane = useUiStore((s) => s.activityPane);
  const close = useUiStore((s) => s.closeActivity);
  const agent = useWorkspaceStore((s) =>
    s.snapshot?.agents.find((a) => a.paneId === pane),
  );
  const { activity, error, now } = useAgentActivity(pane, !!agent?.hasActivity);
  const panelRef = useRef<HTMLDivElement>(null);

  // Esc closes the panel only when the panel has focus (it takes it on
  // opening) or nothing does. With a terminal focused, Esc is the
  // agent's: it interrupts Claude, and must not also close this.
  useEffect(() => {
    if (!pane) return;
    panelRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const focused = document.activeElement;
      if (focused === document.body || panelRef.current?.contains(focused)) {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pane, close]);

  if (!pane) return null;

  const name = agent ? agentLabel(agent) : "Agent";
  const session = activity?.session;
  const subagents = activity?.subagents ?? [];
  const totalTokens =
    (session?.tokens ?? 0) + subagents.reduce((sum, s) => sum + s.tokens, 0);
  const spent = spentOf(session ? [session, ...subagents] : []);
  return (
    <div className="activity-panel" ref={panelRef} tabIndex={-1}>
      <div className="notif-header">
        <span>Activity</span>
        <div className="notif-header-actions">
          <button onClick={close} title="Close (Esc)">
            ×
          </button>
        </div>
      </div>
      <div className="activity-body">
        <div
          className="activity-title"
          title="Show this agent in its tab"
          onClick={() => void zoomPane(pane, true)}
        >
          {name}
        </div>
        {!agent ? (
          <div className="notif-empty">This agent has exited.</div>
        ) : !agent.hasActivity ? (
          <div className="notif-empty">
            Only Claude Code and Codex agents have activity to show: Claude
            Code once its hooks have reported its transcript (the palette's App:
            Set Up Claude Code Integration installs them), Codex once its
            session file is found.
          </div>
        ) : !session ? (
          <div className="notif-empty">{error ?? "Reading its transcripts…"}</div>
        ) : (
          <>
            <div className="activity-stats">
              <Stat label="Tokens" value={tokens(totalTokens)} />
              <Stat
                label="Cost"
                value={spentLabel(spent)}
                title={
                  spent.unpriced
                    ? `${SPENT_MEANS} Some replies had no known price and are not counted.`
                    : SPENT_MEANS
                }
              />
              <Stat label="Context" value={tokens(session.context)} />
              <Stat
                label="Time"
                value={duration(session.startedMs, agent?.status === "working" ? now : session.lastMs)}
              />
            </div>
            <SubagentList activity={activity} now={now} />
            {subagents.length === 0 && (
              <div className="notif-empty">
                {agent.harness === "codex"
                  ? "Codex reports no tasks or subagents."
                  : "No subagents yet. They appear here as soon as it starts one."}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function Stat({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div className="activity-stat" title={title}>
      <span className="activity-stat-label">{label}</span>
      <span className="activity-stat-value">{value}</span>
    </div>
  );
}

/** The agent's own task progress, then its subagents, running ones
 *  first: the panel's list. */
export function SubagentList({ activity, now }: { activity: AgentActivity; now: number }) {
  const tasks = activity.session.tasks;
  const running = activity.subagents.filter((s) => s.state === "running");
  const finished = activity.subagents.filter((s) => s.state !== "running");
  return (
    <>
      {tasks && (
        <div className="activity-session">
          <Progress tasks={tasks} color="var(--accent)" />
          {tasks.current && <div className="activity-current">{tasks.current}</div>}
        </div>
      )}
      <Section title="Running" items={running} now={now} />
      <Section title="Finished" items={finished} now={now} />
    </>
  );
}

function Section({ title, items, now }: { title: string; items: SubagentInfo[]; now: number }) {
  const [open, setOpen] = useState(true);
  if (items.length === 0) return null;
  return (
    <div className="activity-section">
      <div className="activity-section-head" onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} {title} · {items.length}
      </div>
      {open && items.map((s) => <SubagentRow key={s.id} agent={s} now={now} compact={false} />)}
    </div>
  );
}

const STATE_MARK: Record<SubagentState, string> = {
  running: "●",
  done: "✓",
  stopped: "■",
};

export function SubagentRow({
  agent,
  now,
  compact,
}: {
  agent: SubagentInfo;
  now: number;
  compact: boolean;
}) {
  const hues = useConfigStore((s) => s.config?.colors.ui.categorical) ?? [];
  const color = typeColor(agent.agentType ?? "agent", hues);
  const end = agent.state === "running" ? now : agent.lastMs;
  return (
    <div className={`activity-agent ${agent.state}`} style={{ paddingLeft: (agent.depth - 1) * 12 }}>
      <div className="activity-agent-top">
        <span className="activity-agent-title">{agent.description ?? agent.agentType ?? "Subagent"}</span>
        <span className={`activity-mark ${agent.state}`} title={agent.state}>
          {STATE_MARK[agent.state]}
        </span>
      </div>
      <div className="activity-agent-meta">
        <span style={{ color }}>{agent.agentType ?? "agent"}</span>
        {modelName(agent.model) && <span> {modelName(agent.model)}</span>}
      </div>
      <div className="activity-agent-numbers">
        <span>
          {agent.tasks
            ? `${agent.tasks.done}/${agent.tasks.total} · ${percent(agent.tasks)}%`
            : `${agent.toolCalls} tools`}
        </span>
        <span>
          {compact ? "" : `ctx ${tokens(agent.context)} · `}
          {tokens(agent.tokens)}
          {agent.costUsd !== null ? ` · ${spentLabel(spentOf([agent]))}` : ""} · {duration(agent.startedMs, end)}
        </span>
      </div>
      <Bar tasks={agent.tasks} state={agent.state} color={color} />
      {agent.state === "running" && agent.tasks?.current && (
        <div className="activity-current">{agent.tasks.current}</div>
      )}
    </div>
  );
}

function Progress({ tasks, color }: { tasks: TaskProgress; color: string }) {
  return (
    <>
      <div className="activity-agent-numbers">
        <span>
          Tasks {tasks.done}/{tasks.total}
        </span>
        <span>{percent(tasks)}%</span>
      </div>
      <Bar tasks={tasks} state="running" color={color} />
    </>
  );
}

/**
 * How far through its tasks. Without a task list there is no fraction to
 * show: a running one gets a moving bar, a finished one a full one.
 */
function Bar({
  tasks,
  state,
  color,
}: {
  tasks: TaskProgress | null;
  state: SubagentState;
  color: string;
}) {
  const indeterminate = !tasks && state === "running";
  const width = tasks ? percent(tasks) : state === "done" ? 100 : 0;
  return (
    <div className="activity-bar">
      <div
        className={`activity-bar-fill${indeterminate ? " indeterminate" : ""}`}
        style={{ width: indeterminate ? "30%" : `${width}%`, background: color }}
      />
    </div>
  );
}
