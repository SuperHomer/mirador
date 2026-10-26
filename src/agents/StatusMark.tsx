import { AgentStatus } from "../bindings";

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
