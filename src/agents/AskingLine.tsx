import { AgentInfo } from "../bindings";

/**
 * What a waiting agent asks for, on one line: the tool call it wants
 * permission for when its transcript says (the tool, then what it is
 * about), else its notification's message. Nothing when it has neither.
 */
export function AskingLine({ agent, className }: { agent: AgentInfo; className: string }) {
  const asking = agent.asking;
  if (asking) {
    return (
      <div className={`asking-line ${className}`} title={`${asking.tool}: ${asking.summary}`}>
        <span className="asking-tool">{asking.tool}</span>{" "}
        <span className="asking-summary">{asking.summary}</span>
      </div>
    );
  }
  if (!agent.message) return null;
  return (
    <div className={`asking-line ${className}`} title={agent.message}>
      {agent.message}
    </div>
  );
}
