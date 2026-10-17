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

/**
 * An entry that opens a second step rather than running: "New Agent…"
 * lists the roles, "Start Team…" the teams, "SSH…" the hosts. A palette
 * with every role, team and host at the top buried its commands once
 * there were a dozen of each.
 */
interface Step {
  id: string;
  title: string;
  hint?: string;
  /** Shown before the query once inside: "New Agent". */
  crumb: string;
  placeholder: string;
  /** What to say when the step has nothing in it. */
  empty: string;
  entries: Entry[];
}

type Row = { kind: "entry"; entry: Entry } | { kind: "step"; step: Step };

const rowId = (r: Row) =>
  r.kind === "entry" ? r.entry.id : `step:${r.step.id}`;

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

  // The step open, if any: its entries replace the top level.
  const [stepId, setStepId] = useState<string | null>(null);

  const focusedPane = () =>
    activeTab(useWorkspaceStore.getState().snapshot)?.focusedPane ?? null;

  const { entries, steps } = useMemo(() => {
    // Shown so the palette teaches its own shortcuts: the keys a command
    // already has are the reason to stop opening the palette for it.
    const accels = acceleratorsByAction(keybindings);
    const accelOf = (id: string) => {
      const accel = accels.get(id);
      return accel ? formatAccel(accel) : undefined;
    };
    const base: Entry[] = actions
      // "New Agent…" is a step now, whose first entry is this action.
      .filter((a) => a.id !== "command_palette" && a.id !== "new_agent")
      .map((a) => ({
        id: a.id,
        title: a.title,
        accel: accelOf(a.id),
        run: a.run,
      }));
    const custom: Entry[] = customCommands.map((c) => ({
      id: `custom:${c.name}`,
      title: c.name,
      hint: c.command,
      run: () => {
        if (c.target === "tab") {
          void newTab(c.command);
        } else {
          const pane = focusedPane();
          if (pane) void splitPane(pane, "row", c.command);
        }
      },
    }));
    const plain = actions.find((a) => a.id === "new_agent");
    // The roles the focused pane's directory sees — its repository's, then
    // yours — each splitting the focused pane: the agent joins the tab you
    // are working in.
    const agents: Step = {
      id: "agent",
      title: "New Agent…",
      hint: roles.length
        ? `Claude Code, or one of ${roles.length} ${roles.length === 1 ? "role" : "roles"}`
        : "Claude Code",
      crumb: "New Agent",
      placeholder: "Search roles…",
      empty: "No matching role",
      entries: [
        {
          id: "agent:",
          title: "Claude Code",
          hint: "no role",
          accel: accelOf("new_agent"),
          run: () => plain?.run(),
        },
        ...roles.map((r) => ({
          id: `agent:${r.name}`,
          title: r.name,
          hint: roleHint(r),
          run: () => {
            openAgent(r.name, null, focusedPane(), false).catch((e) =>
              console.error(`mirador: role ${r.name}: ${e}`),
            );
          },
        })),
      ],
    };
    // Teams the same way, each in a tab of its own.
    const teamStep: Step = {
      id: "team",
      title: "Start Team…",
      hint: `${teams.length} ${teams.length === 1 ? "team" : "teams"}`,
      crumb: "Start Team",
      placeholder: "Search teams…",
      empty: "No matching team",
      entries: teams.map((t) => ({
        id: `team:${t.name}`,
        title: t.name,
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
          startTeam(t.name, null, focusedPane()).catch((e) =>
            console.error(`mirador: team ${t.name}: ${e}`),
          );
        },
      })),
    };
    // ~/.ssh/config aliases.
    const ssh: Step = {
      id: "ssh",
      title: "SSH…",
      hint: `${hosts.length} ${hosts.length === 1 ? "host" : "hosts"} in ~/.ssh/config`,
      crumb: "SSH",
      placeholder: "Search hosts…",
      empty: "No matching host",
      entries: hosts.map((host) => ({
        id: `ssh:${host}`,
        title: host,
        hint: "open remote pane",
        run: () => void openSsh(null, false, host),
      })),
    };
    // A step with nothing in it is not offered; "New Agent…" always has
    // plain Claude Code.
    const steps = [agents, teamStep, ssh].filter((st) => st.entries.length > 0);
    return { entries: [...custom, ...base], steps };
  }, [customCommands, roles, teams, hosts, keybindings]);

  const step = steps.find((st) => st.id === stepId) ?? null;

  const filtered = useMemo<Row[]>(() => {
    const rank = <T,>(items: T[], text: (t: T) => string) =>
      items
        .map((item) => ({ item, score: fuzzyScore(query, text(item)) }))
        .filter((x): x is { item: T; score: number } => x.score !== null)
        .sort((a, b) => b.score - a.score)
        .map((x) => x.item);
    if (step) {
      return rank(step.entries, (e) => e.title).map((entry) => ({
        kind: "entry",
        entry,
      }));
    }
    const top: Row[] = rank<Row>(
      [
        ...entries.map((entry) => ({ kind: "entry" as const, entry })),
        ...steps.map((st) => ({ kind: "step" as const, step: st })),
      ],
      (r) => (r.kind === "entry" ? r.entry.title : r.step.title),
    );
    if (!query) return top;
    // What a step holds is still found from the top by its name — a
    // `reviewer` you know needs no second step — below the commands, so a
    // dozen roles never crowd them out.
    const deep: Row[] = steps.flatMap((st) =>
      rank(st.entries, (e) => e.title).map((e) => ({
        kind: "entry" as const,
        entry: { ...e, id: `deep:${e.id}`, title: `${st.crumb} › ${e.title}` },
      })),
    );
    return [...top, ...deep];
  }, [entries, steps, step, query]);

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
      setStepId(null);
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

  useEffect(() => setSelected(0), [query, stepId]);

  if (!open) return null;

  const enter = (st: Step) => {
    setStepId(st.id);
    setQuery("");
    inputRef.current?.focus();
  };
  const back = () => {
    setStepId(null);
    setQuery("");
  };
  const choose = (row: Row | undefined) => {
    if (!row) return;
    if (row.kind === "step") {
      enter(row.step);
      return;
    }
    close();
    row.entry.run();
  };

  return (
    <div className="palette-overlay" onMouseDown={close}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette-field">
          {step && (
            <button
              className="palette-crumb"
              title="Back (Esc)"
              onClick={() => {
                back();
                inputRef.current?.focus();
              }}
            >
              {step.crumb} ›
            </button>
          )}
          <input
            ref={inputRef}
            className="palette-input"
            placeholder={step ? step.placeholder : "Type a command…"}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                // Inside a step, back to the top first.
                if (step) back();
                else close();
              } else if (e.key === "Backspace" && step && query === "") {
                e.preventDefault();
                back();
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                setSelected((s) => Math.min(s + 1, filtered.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSelected((s) => Math.max(s - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                choose(filtered[selected]);
              }
              e.stopPropagation();
            }}
          />
        </div>
        <div className="palette-list">
          {filtered.map((row, i) => {
            const title =
              row.kind === "entry" ? row.entry.title : row.step.title;
            const hint = row.kind === "entry" ? row.entry.hint : row.step.hint;
            const accel = row.kind === "entry" ? row.entry.accel : undefined;
            return (
              <div
                key={rowId(row)}
                className={`palette-item${i === selected ? " selected" : ""}`}
                onMouseEnter={() => setSelected(i)}
                onClick={() => choose(row)}
              >
                <span className="palette-title">{title}</span>
                {hint && <span className="palette-hint">{hint}</span>}
                {accel && <kbd className="palette-accel">{accel}</kbd>}
                {row.kind === "step" && <span className="palette-more">›</span>}
              </div>
            );
          })}
          {filtered.length === 0 && (
            <div className="palette-empty">
              {step ? step.empty : "No matching commands"}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
