import { useEffect, useState } from "react";
import {
  AgentRole,
  agentRoles,
  openAgent,
  openAgentWall,
  openBrowser,
  openDiff,
  openGraph,
  roleHint,
  splitPane,
} from "../bindings";

/**
 * What a frame's "+" adds to its tab: what the tabs view can create, by
 * the same calls, from the frame's focused pane — a split of it, whose
 * card the board places in the frame. The agent wall opens as it always
 * does, in a tab of its own.
 */
export function FrameMenu({ pane, onClose }: { pane: string; onClose: () => void }) {
  const [agents, setAgents] = useState(false);
  const [roles, setRoles] = useState<AgentRole[]>([]);

  // The roles of the frame's focused pane's directory, as "New Agent…" in
  // the palette lists them.
  useEffect(() => {
    if (!agents) return;
    let live = true;
    agentRoles(pane)
      .then((c) => live && setRoles(c.roles))
      .catch(() => live && setRoles([]));
    return () => {
      live = false;
    };
  }, [agents, pane]);

  const run = (create: () => Promise<unknown>) => () => {
    onClose();
    create().catch((e) => console.error(`mirador: ${e}`));
  };

  return (
    <div
      className="frame-menu"
      onMouseLeave={onClose}
      // The frame's title bar drags the frame; its menu must not.
      onPointerDown={(e) => e.stopPropagation()}
    >
      {agents ? (
        <>
          <button onClick={() => setAgents(false)}>‹ Back</button>
          <button onClick={run(() => openAgent(null, null, pane, false))}>
            <span>Claude</span>
            <span className="frame-menu-hint">default model</span>
          </button>
          {roles.map((r) => (
            <button
              key={r.name}
              disabled={!!r.problem}
              title={r.problem ?? r.description ?? undefined}
              onClick={run(() => openAgent(r.name, null, pane, false))}
            >
              <span>{r.name}</span>
              <span className="frame-menu-hint">{roleHint(r)}</span>
            </button>
          ))}
        </>
      ) : (
        <>
          <button onClick={run(() => splitPane(pane, "row"))}>Terminal</button>
          {/* As the palette's "New Browser Pane": a blank page, its URL
              typed in the pane's own bar. */}
          <button onClick={run(() => openBrowser(pane, false, "about:blank"))}>
            Browser
          </button>
          <button onClick={() => setAgents(true)}>Agent…</button>
          <button onClick={run(() => openDiff(pane, false, null))}>Diff</button>
          <button onClick={run(() => openGraph(pane, false))}>
            Commit graph
          </button>
          <button onClick={run(() => openAgentWall(pane))}>Agent wall</button>
        </>
      )}
    </div>
  );
}
