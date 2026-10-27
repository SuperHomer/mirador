import { AgentInfo, AgentStatus } from "../bindings";

export const STATUS_LABEL: Record<AgentStatus, string> = {
  working: "working",
  needsYou: "needs you",
  idle: "idle",
};

const CLASS: Record<AgentStatus, string> = {
  working: "working",
  needsYou: "needs-you",
  idle: "idle",
};

type Props = { status: AgentStatus | null | undefined };

/** An agent's status: a "!" square when it needs you, a pulsing dot while
 *  it works, a ring when idle, a muted ring when it is not known. */
export function StatusMark({ status, small }: Props & { small?: boolean }) {
  const label = status ? STATUS_LABEL[status] : "status unknown";
  return (
    <span
      className={`status-mark ${status ? CLASS[status] : "unknown"}${small ? " small" : ""}`}
      role="img"
      aria-label={label}
      title={label}
    >
      {status === "needsYou" ? "!" : null}
    </span>
  );
}

/** A pip per agent, in order: a tab's agents at a glance, in the sidebar
 *  and on its board frame. Nothing for none. */
export function StatusPips({ agents }: { agents: AgentInfo[] }) {
  if (agents.length === 0) return null;
  return (
    <span className="tab-pips">
      {agents.map((a) => (
        <StatusPip key={a.paneId} status={a.status} />
      ))}
    </span>
  );
}

/** The 7px form of `StatusMark`, for a row listing several agents. */
export function StatusPip({ status }: Props) {
  const label = status ? STATUS_LABEL[status] : "status unknown";
  return (
    <span
      className={`status-pip ${status ? CLASS[status] : "unknown"}`}
      role="img"
      aria-label={label}
      title={label}
    />
  );
}
