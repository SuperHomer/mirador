import { AgentActivity, AgentInfo, HARNESS_LABEL } from "../bindings";
import { duration, modelName, tokens } from "./ActivityPanel";

/** Green under half full, yellow to 80%, red past it. */
export function fullness(pct: number): string {
  return pct < 50 ? "low" : pct <= 80 ? "mid" : "high";
}

/**
 * The foot of an agent's tile: what it runs (harness, model, effort), how
 * full its context is, what it has used, and — for a Claude Code agent —
 * the way to its subagents. Says "not reported" where the harness gives
 * nothing to read.
 */
export function AgentStatusBar({
  agent,
  activity,
  now,
  sheetOpen,
  onToggleSheet,
  compact = false,
}: {
  agent: AgentInfo;
  activity: AgentActivity | null;
  now: number;
  sheetOpen: boolean;
  onToggleSheet: () => void;
  /** A grid tile's footer: the model, the context and the subagents,
   *  without the chips, the effort or the usage. */
  compact?: boolean;
}) {
  const session = activity?.session ?? null;
  const model = modelName(session?.model ?? null) ?? modelName(agent.model);
  const used = session?.context ?? 0;
  const window = session?.contextWindow ?? null;
  const pct = window ? Math.min(100, Math.round((used / window) * 100)) : null;
  const end = agent.status === "working" ? now : (session?.lastMs ?? null);
  const subagents = agent.harness === "claude" && agent.hasActivity;
  const pill = subagents && (
    <button
      className={`agent-bar-subagents${sheetOpen ? " open" : ""}`}
      onClick={onToggleSheet}
    >
      Subagents {activity?.subagents.length ?? 0} {sheetOpen ? "▾" : "▸"}
    </button>
  );
  if (compact) {
    return (
      <div className="agent-bar compact">
        {model && <span className="agent-bar-model">{model}</span>}
        {pct !== null && (
          <>
            <span className={`agent-bar-meter ${fullness(pct)}`}>
              <span style={{ "--fill": `${pct}%` } as React.CSSProperties} />
            </span>
            <span className="agent-bar-pct">{pct}%</span>
          </>
        )}
        {pill}
      </div>
    );
  }
  return (
    <div className="agent-bar">
      <span className="agent-bar-chips">
        <span className="agent-bar-chip">{HARNESS_LABEL[agent.harness]}</span>
        {model && <span className="agent-bar-chip">{model}</span>}
        {agent.effort && (
          <span className="agent-bar-chip outline">effort {agent.effort}</span>
        )}
      </span>
      {!agent.hasActivity ? (
        <span className="agent-bar-context">Context not reported</span>
      ) : (
        session && (
          <>
            <span className="agent-bar-context">
              Context
              {pct !== null && (
                <>
                  <span className={`agent-bar-meter ${fullness(pct)}`}>
                    {/* The fill's width is the one value that cannot be a class. */}
                    <span style={{ "--fill": `${pct}%` } as React.CSSProperties} />
                  </span>
                  <span className="agent-bar-pct">{pct}%</span>
                </>
              )}
              <span>
                {tokens(used)}
                {window ? ` / ${tokens(window)}` : ""}
              </span>
            </span>
            <span className="agent-bar-usage">
              {tokens(session.tokens)} tokens · {duration(session.startedMs, end)}
            </span>
          </>
        )
      )}
      {pill}
    </div>
  );
}
