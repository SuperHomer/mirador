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
import { opensInTab, useConfigStore } from "../state/configStore";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { runAction } from "../keymap/actions";

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
 * The tab bar's + New popover: a new tab; then an agent — an optional
 * first message, which also becomes the new agent's card, then a role of
 * this repository, or plain Claude Code or Codex, joining the tab by
 * splitting `pane` (under the wall, if it shows); then the panes the
 * palette and the keys open, through the same actions. Closes on a click
 * outside, or Esc while it has the keyboard — an Esc typed into a
 * terminal is the terminal's.
 */
export function NewMenu({ pane, onClose }: { pane: string; onClose: () => void }) {
  const catalog = useRoleCatalog(pane);
  const [message, setMessage] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const accels = acceleratorsByAction(useConfigStore((s) => s.config?.keybindings));

  // Browser pages are native views over the window, and would cover the
  // menu: they are hidden while it is open (`App`).
  useEffect(() => {
    useUiStore.getState().setNewMenuOpen(true);
    return () => useUiStore.getState().setNewMenuOpen(false);
  }, []);

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
    void openAgentEditor(pane, opensInTab("editor"));
  };
  const roles = catalog?.roles ?? [];
  /** A row running the action `id`, as its key or the palette would. */
  const item = (id: string, label: string) => {
    const accel = accels.get(id);
    return (
      <button
        className="agent-menu-item"
        onClick={() => {
          acted.current = true;
          onClose();
          runAction(id);
        }}
      >
        {label}
        {accel && <span className="agent-menu-accel">{formatAccel(accel)}</span>}
      </button>
    );
  };

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
      {item("new_tab", "New tab")}
      <div className="agent-menu-sep" />

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

      <div className="agent-menu-sep" />
      <div className="agent-menu-label">Terminal</div>
      {item("split_right", "Split right")}
      {item("split_down", "Split down")}
      <div className="agent-menu-label">Git</div>
      {item("new_diff_pane", "Diff")}
      {item("commit_graph", "Commit graph")}
      <div className="agent-menu-sep" />
      {item("new_browser_pane", "Browser")}
    </div>
  );
}
