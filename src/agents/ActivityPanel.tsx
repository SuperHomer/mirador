import { useEffect, useState } from "react";
import {
  AgentActivity,
  HARNESS_LABEL,
  SubagentInfo,
  SubagentState,
  TaskProgress,
  getAgentActivity,
  zoomPane,
} from "../bindings";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";

/** How often the open panel reads the transcripts again. */
const POLL_MS = 2000;

/** Each subagent type gets one of these, the same one every time. */
const TYPE_COLORS = ["#fab387", "#a6e3a1", "#89b4fa", "#f9e2af", "#cba6f7", "#94e2d5", "#f38ba8"];

export function typeColor(type: string): string {
  let h = 0;
  for (const c of type) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return TYPE_COLORS[h % TYPE_COLORS.length];
}

export function tokens(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${Math.round(n / 1000)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** m:ss, or h:mm:ss past the hour. */
function duration(fromMs: number | null, toMs: number | null): string {
  if (fromMs == null || toMs == null) return "–";
  const s = Math.max(0, Math.floor((toMs - fromMs) / 1000));
  const mm = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  if (s >= 3600) return `${Math.floor(s / 3600)}:${String(mm).padStart(2, "0")}:${ss}`;
  return `${mm}:${ss}`;
}

/** `claude-opus-5-5` → `Opus 5.5`; anything else as it is. */
function modelName(model: string | null): string | null {
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
 * `pane`'s activity, read again every [`POLL_MS`] while `enabled`. Null
 * until the first read, and whenever the agent has none to report (not
 * Claude Code, or no hook has said where its transcript is): the error
 * says which.
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
    const load = () =>
      getAgentActivity(pane).then(
        (a) => {
          if (cancelled) return;
          setActivity(a);
          setError(null);
          setNow(Date.now());
        },
        (e) => {
          if (cancelled) return;
          setActivity(null);
          setError(String(e));
        },
      );
    void load();
    const timer = window.setInterval(load, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [pane, enabled]);

  return { activity, error, now };
}

/**
 * A Claude Code agent's activity, opened by right-clicking it in the
 * sidebar or on the wall: its own task list, and every subagent it started
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
  const { activity, error, now } = useAgentActivity(pane);

  useEffect(() => {
    if (!pane) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pane, close]);

  if (!pane) return null;

  const name = agent ? (agent.role ?? agent.topic ?? HARNESS_LABEL[agent.harness]) : "Agent";
  const session = activity?.session;
  const subagents = activity?.subagents ?? [];
  const totalTokens =
    (session?.tokens ?? 0) + subagents.reduce((sum, s) => sum + s.tokens, 0);
  return (
    <div className="activity-panel">
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
        ) : error ? (
          <div className="notif-empty">{error}</div>
        ) : !session ? (
          <div className="notif-empty">Reading its transcripts…</div>
        ) : (
          <>
            <div className="activity-stats">
              <Stat label="Tokens" value={tokens(totalTokens)} />
              <Stat label="Context" value={tokens(session.context)} />
              <Stat
                label="Time"
                value={duration(session.startedMs, agent?.status === "working" ? now : session.lastMs)}
              />
            </div>
            <SubagentList activity={activity} now={now} />
            {subagents.length === 0 && (
              <div className="notif-empty">
                No subagents yet. They appear here as soon as it starts one.
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="activity-stat">
      <span className="activity-stat-label">{label}</span>
      <span className="activity-stat-value">{value}</span>
    </div>
  );
}

/**
 * The agent's own task progress, then its subagents, running ones first:
 * the panel's list, and the side of each agent's tile on the wall
 * (`compact`, which is narrower and keeps finished ones folded away while
 * any are running).
 */
export function SubagentList({
  activity,
  now,
  compact = false,
}: {
  activity: AgentActivity;
  now: number;
  compact?: boolean;
}) {
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
      <Section title="Running" items={running} now={now} compact={compact} />
      <Section
        title="Finished"
        items={finished}
        now={now}
        compact={compact}
        defaultOpen={!compact || running.length === 0}
      />
    </>
  );
}

function Section({
  title,
  items,
  now,
  compact,
  defaultOpen = true,
}: {
  title: string;
  items: SubagentInfo[];
  now: number;
  compact: boolean;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  if (items.length === 0) return null;
  return (
    <div className="activity-section">
      <div className="activity-section-head" onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} {title} · {items.length}
      </div>
      {open && items.map((s) => <SubagentRow key={s.id} agent={s} now={now} compact={compact} />)}
    </div>
  );
}

const STATE_MARK: Record<SubagentState, string> = {
  running: "●",
  done: "✓",
  stopped: "■",
};

function SubagentRow({
  agent,
  now,
  compact,
}: {
  agent: SubagentInfo;
  now: number;
  compact: boolean;
}) {
  const color = typeColor(agent.agentType ?? "agent");
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
          {tokens(agent.tokens)} · {duration(agent.startedMs, end)}
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
