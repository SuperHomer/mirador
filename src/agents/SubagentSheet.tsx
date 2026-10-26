import { useState } from "react";
import { AgentActivity } from "../bindings";
import { SubagentRow } from "./ActivityPanel";

type Filter = "running" | "finished" | null;

/**
 * An agent's subagents, slid over the right of its terminal, which stays
 * live beside it. Running and Finished narrow the list; clicked again,
 * they widen it back. Esc is left to the agent.
 */
export function SubagentSheet({
  activity,
  now,
  onClose,
}: {
  activity: AgentActivity | null;
  now: number;
  onClose: () => void;
}) {
  const [filter, setFilter] = useState<Filter>(null);
  const all = activity?.subagents ?? [];
  const running = all.filter((s) => s.state === "running");
  const finished = all.filter((s) => s.state !== "running");
  const shown = filter === "running" ? running : filter === "finished" ? finished : all;
  const chip = (id: Exclude<Filter, null>, label: string, n: number) => (
    <button
      className={`subagent-sheet-chip${filter === id ? " selected" : ""}`}
      onClick={() => setFilter(filter === id ? null : id)}
    >
      {label} {n}
    </button>
  );
  return (
    <div className="subagent-sheet">
      <div className="subagent-sheet-head">
        <span>Subagents · {all.length}</span>
        <button title="Close" onClick={onClose}>
          ×
        </button>
      </div>
      <div className="subagent-sheet-chips">
        {chip("running", "Running", running.length)}
        {chip("finished", "Finished", finished.length)}
      </div>
      <div className="subagent-sheet-list">
        {all.length === 0 ? (
          <div className="notif-empty">
            No subagents yet. They appear here as soon as it starts one.
          </div>
        ) : (
          shown.map((s) => <SubagentRow key={s.id} agent={s} now={now} compact />)
        )}
      </div>
    </div>
  );
}
