import { useEffect, useMemo, useRef, useState } from "react";
import {
  AgentRole,
  AgentTeam,
  agentRoles,
  startTeam,
  newTab,
  splitPane,
  openSsh,
  sshHosts,
  openAgent,
  roleHint,
} from "../bindings";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { actions } from "../keymap/actions";
import { useConfigStore } from "../state/configStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";

interface Entry {
  id: string;
  title: string;
  hint?: string;
  /** Already formatted for this platform; absent when nothing is bound. */
  accel?: string;
  run: () => void;
}

/** Subsequence fuzzy match; higher is better, null = no match. */
function fuzzyScore(query: string, text: string): number | null {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let qi = 0;
  let score = 0;
  let streak = 0;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      streak += 1;
      score += streak + (ti === 0 || t[ti - 1] === " " ? 5 : 0);
      qi++;
    } else {
      streak = 0;
    }
  }
  return qi === q.length ? score : null;
}

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const close = useUiStore((s) => s.closePalette);
  const customCommands = useConfigStore((s) => s.config?.customCommands ?? []);
  const keybindings = useConfigStore((s) => s.config?.keybindings);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [hosts, setHosts] = useState<string[]>([]);
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [teams, setTeams] = useState<AgentTeam[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  const entries = useMemo<Entry[]>(() => {
    // Shown so the palette teaches its own shortcuts: the keys a command
    // already has are the reason to stop opening the palette for it.
    const accels = acceleratorsByAction(keybindings);
    const base: Entry[] = actions
      .filter((a) => a.id !== "command_palette")
      .map((a) => {
        const accel = accels.get(a.id);
        return {
          id: a.id,
          title: a.title,
          accel: accel ? formatAccel(accel) : undefined,
          run: a.run,
        };
      });
    const custom: Entry[] = customCommands.map((c) => ({
      id: `custom:${c.name}`,
      title: c.name,
      hint: c.command,
      run: () => {
        if (c.target === "tab") {
          void newTab(c.command);
        } else {
          const { snapshot } = useWorkspaceStore.getState();
          const pane = activeTab(snapshot)?.focusedPane;
          if (pane) void splitPane(pane, "row", c.command);
        }
      },
    }));
    // ~/.ssh/config aliases become "SSH: <host>" entries.
    const ssh: Entry[] = hosts.map((host) => ({
      id: `ssh:${host}`,
      title: `SSH: ${host}`,
      hint: "open remote pane",
      run: () => void openSsh(null, false, host),
    }));
    // The roles the focused pane's directory sees — its repository's, then
    // yours — become "New Agent: <role>" entries, each splitting the
    // focused pane: the agent joins the tab you are working in.
    const agents: Entry[] = roles.map((r) => ({
      id: `agent:${r.name}`,
      title: `New Agent: ${r.name}`,
      hint: roleHint(r),
      run: () => {
        const { snapshot } = useWorkspaceStore.getState();
        const pane = activeTab(snapshot)?.focusedPane ?? null;
        openAgent(r.name, null, pane, false).catch((e) =>
          console.error(`mirador: role ${r.name}: ${e}`),
        );
      },
    }));
    // Teams the same way: "Start Team: <team>", in a tab of its own.
    const teamEntries: Entry[] = teams.map((t) => ({
      id: `team:${t.name}`,
      title: `Start Team: ${t.name}`,
      hint: t.problem
        ? `can't start: ${t.problem}`
        : [
            t.members
              .map((m) => (m.count > 1 ? `${m.role}×${m.count}` : m.role))
              .join(", "),
            t.source === "repo" ? "repo" : null,
          ]
            .filter(Boolean)
            .join(" · "),
      run: () => {
        const { snapshot } = useWorkspaceStore.getState();
        const pane = activeTab(snapshot)?.focusedPane ?? null;
        startTeam(t.name, null, pane).catch((e) =>
          console.error(`mirador: team ${t.name}: ${e}`),
        );
      },
    }));
    return [...custom, ...agents, ...teamEntries, ...ssh, ...base];
  }, [customCommands, roles, teams, hosts, keybindings]);

  const filtered = useMemo(() => {
    return entries
      .map((e) => ({ e, score: fuzzyScore(query, e.title) }))
      .filter((x): x is { e: Entry; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((x) => x.e);
  }, [entries, query]);

  useEffect(() => {
    if (open) {
      void sshHosts().then(setHosts);
      const pane =
        activeTab(useWorkspaceStore.getState().snapshot)?.focusedPane ?? null;
      agentRoles(pane)
        .then((c) => {
          setRoles(c.roles);
          setTeams(c.teams);
        })
        .catch(() => {
          setRoles([]);
          setTeams([]);
        });
      setQuery("");
      setSelected(0);
      // Focus after the overlay renders.
      requestAnimationFrame(() => inputRef.current?.focus());
    } else {
      // Hand focus back to the focused terminal.
      const textarea = document.querySelector<HTMLTextAreaElement>(
        ".pane.focused textarea",
      );
      textarea?.focus();
    }
  }, [open]);

  useEffect(() => setSelected(0), [query]);

  if (!open) return null;

  const runEntry = (entry: Entry | undefined) => {
    close();
    entry?.run();
  };

  return (
    <div className="palette-overlay" onMouseDown={close}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="palette-input"
          placeholder="Type a command…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              close();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelected((s) => Math.min(s + 1, filtered.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelected((s) => Math.max(s - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              runEntry(filtered[selected]);
            }
            e.stopPropagation();
          }}
        />
        <div className="palette-list">
          {filtered.map((entry, i) => (
            <div
              key={entry.id}
              className={`palette-item${i === selected ? " selected" : ""}`}
              onMouseEnter={() => setSelected(i)}
              onClick={() => runEntry(entry)}
            >
              <span className="palette-title">{entry.title}</span>
              {entry.hint && <span className="palette-hint">{entry.hint}</span>}
              {entry.accel && (
                <kbd className="palette-accel">{entry.accel}</kbd>
              )}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="palette-empty">No matching commands</div>
          )}
        </div>
      </div>
    </div>
  );
}
