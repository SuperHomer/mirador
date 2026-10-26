import { useEffect, useRef, useState } from "react";
import {
  AgentRole,
  agentRoles,
  Harness,
  HARNESS_LABEL,
  openAgent,
  openAgentEditor,
  RoleCatalog,
} from "../bindings";
import { useUiStore } from "../state/uiStore";

/** "Claude Code · opus · high": what a role starts. */
export function roleLine(r: AgentRole): string {
  return [HARNESS_LABEL[r.harness], r.model ?? "default model", r.effort].filter(Boolean).join(" · ");
}

/** The roles and teams `pane`'s directory sees, read when asked for. */
export function useRoleCatalog(pane: string | null): RoleCatalog | null {
  const [catalog, setCatalog] = useState<RoleCatalog | null>(null);
  useEffect(() => {
    if (!pane) return;
    let live = true;
    agentRoles(pane)
      .then((c) => live && setCatalog(c))
      .catch(() => live && setCatalog(null));
    return () => {
      live = false;
    };
  }, [pane]);
  return catalog;
}

/** The palette's team step: which team, and its task, are chosen there. */
export function startTeamStep() {
  useUiStore.getState().openPalette("team");
}

/**
 * The wall's + Agent popover (21c): an optional first message, which also
 * becomes the new agent's card, then a role of this repository, or plain
 * Claude Code or Codex. The agent joins the tab, splitting `pane` under
 * the wall. Closes on a click outside, or Esc while it has the keyboard —
 * an Esc typed into a terminal is the agent's.
 */
export function AgentMenu({ pane, onClose }: { pane: string; onClose: () => void }) {
  const catalog = useRoleCatalog(pane);
  const [message, setMessage] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);

  // Whatever had the keyboard when the menu opened — read while rendering,
  // before the input's autofocus takes it — gets it back when the menu
  // closes having started nothing, unless something else took it since: a
  // click outside on another terminal keeps that one.
  const [before] = useState(() => document.activeElement as HTMLElement | null);
  const acted = useRef(false);
  useEffect(
    () => () => {
      if (acted.current) return;
      requestAnimationFrame(() => {
        const now = document.activeElement;
        if (before?.isConnected && (!now || now === document.body)) before.focus();
      });
    },
    [before],
  );

  // Outside the menu and its button: the button toggles it itself.
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (!menuRef.current?.parentElement?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", down);
    return () => document.removeEventListener("mousedown", down);
  }, [onClose]);

  const start = (role: string | null, harness: Harness | null) => {
    acted.current = true;
    onClose();
    openAgent(role, message.trim() || null, pane, false, harness).catch((e) =>
      console.error(`mirador: ${role ?? harness}: ${e}`),
    );
  };
  const edit = () => {
    acted.current = true;
    onClose();
    void openAgentEditor(pane, false);
  };
  const roles = catalog?.roles ?? [];

  return (
    <div
      className="agent-menu"
      ref={menuRef}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <label className="agent-menu-label" htmlFor="agent-menu-message">
        First message (optional)
      </label>
      <input
        id="agent-menu-message"
        className="agent-menu-input"
        value={message}
        autoFocus
        placeholder="What it should do first"
        onChange={(e) => setMessage(e.target.value)}
      />
      <div className="agent-menu-hint">It also becomes a card for the new agent.</div>

      <div className="agent-menu-label">Roles in this repo</div>
      {roles.length === 0 && (
        <div className="agent-menu-hint">
          None here yet: <code>.mirador/roles/&lt;name&gt;.md</code> gives an agent its own model,
          prompt and way of working.
        </div>
      )}
      {roles.map((r) => (
        <div key={r.name} className={`agent-menu-row${r.problem ? " problem" : ""}`}>
          <span className="agent-menu-name">{r.name}</span>
          <span className="agent-menu-meta">{r.problem ?? roleLine(r)}</span>
          {r.problem ? (
            <button className="agent-menu-link" onClick={edit}>
              Fix
            </button>
          ) : (
            <button className="agent-menu-start" onClick={() => start(r.name, null)}>
              Start
            </button>
          )}
        </div>
      ))}

      <div className="agent-menu-label">No role</div>
      {(["claude", "codex"] as Harness[]).map((h) => (
        <div key={h} className="agent-menu-row">
          <span className="agent-menu-name">{HARNESS_LABEL[h]}</span>
          <span className="agent-menu-meta">default model</span>
          <button className="agent-menu-start" onClick={() => start(null, h)}>
            Start
          </button>
        </div>
      ))}

      <div className="agent-menu-foot">
        <button
          className="agent-menu-link"
          onClick={() => {
            acted.current = true;
            onClose();
            startTeamStep();
          }}
        >
          Start a team…
        </button>
        <button className="agent-menu-link" onClick={edit}>
          Edit roles…
        </button>
        <span className="agent-menu-joins">Joins this tab</span>
      </div>
    </div>
  );
}
