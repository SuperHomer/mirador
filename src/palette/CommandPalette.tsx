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
  setActiveTab,
} from "../bindings";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { ActionGroup, actions } from "../keymap/actions";
import { useConfigStore } from "../state/configStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";

interface Entry {
  id: string;
  title: string;
  hint?: string;
  /** Where an empty query lists it; a step's own entries have none. */
  group?: ActionGroup;
  /** A custom command's hint is the command it runs. */
  command?: boolean;
  /** Already formatted for this platform; absent when nothing is bound. */
  accel?: string;
  run: () => void;
}

/**
 * An entry that opens a second step rather than running: "Agents: New
 * Agent…" lists the roles, "Agents: Start Team…" the teams, "Pane: SSH…"
 * the hosts. A palette with every role, team and host at the top buried
 * its commands once there were a dozen of each.
 */
interface Step {
  id: string;
  title: string;
  /** What it opens: "pick a role ›". */
  hint: string;
  group: ActionGroup;
  /** Shown before the query once inside: "New Agent". */
  crumb: string;
  placeholder: string;
  /** What to say when the step has nothing in it. */
  empty: string;
  entries: Entry[];
}

/** `key` tells apart a row shown twice: once in RECENT, once in its group. */
type Row = { kind: "entry"; entry: Entry; key?: string } | { kind: "step"; step: Step };

/** The commands last run from the palette, newest first, by entry id. */
const RECENT_KEY = "mirador.palette.recent";
const RECENT_MAX = 5;

// The webview's storage can throw (or be empty): the palette works without.
function readRecent(): string[] {
  try {
    const ids: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(ids) ? ids.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

function rememberRecent(id: string) {
  try {
    const ids = [id, ...readRecent().filter((r) => r !== id)].slice(0, RECENT_MAX);
    localStorage.setItem(RECENT_KEY, JSON.stringify(ids));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

/** The groups an empty query lists, in order. */
const GROUPS: [ActionGroup, string][] = [
  ["agents", "AGENTS"],
  ["git", "GIT"],
  ["pane", "PANE"],
  ["view", "VIEW"],
  ["app", "APP"],
  ["tabs", "TABS"],
];

/** Rows under a group label; `label` is null for a ranked list. */
interface Section {
  label: string | null;
  rows: Row[];
}

const rowId = (r: Row) =>
  r.kind === "entry" ? (r.key ?? r.entry.id) : `step:${r.step.id}`;

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
  const tabs = useWorkspaceStore((s) => s.snapshot?.tabs);
  const [query, setQuery] = useState("");
  // The selected row, by id: rows can arrive or go while the palette is
  // open (roles read, a tab closed), and an index would then point at
  // another command. Its index is kept for when the row itself goes.
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const lastIndex = useRef(0);
  const [hosts, setHosts] = useState<string[]>([]);
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [teams, setTeams] = useState<AgentTeam[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
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
      // "Agents: New Agent…" is a step now, whose first entry is this action.
      .filter((a) => a.id !== "command_palette" && a.id !== "new_agent")
      .map((a) => ({
        id: a.id,
        title: a.title,
        hint: a.hint,
        group: a.group,
        accel: accelOf(a.id),
        run: a.run,
      }));
    // Every open tab, by the number its shortcut jumps to.
    const tabEntries: Entry[] = (tabs ?? []).map((t, i) => ({
      id: `tab:${t.id}`,
      title: `Tab: ${t.title}`,
      group: "tabs",
      accel: i < 9 ? accelOf(`tab_${i + 1}`) : undefined,
      run: () => void setActiveTab(t.id),
    }));
    const custom: Entry[] = customCommands.map((c) => ({
      id: `custom:${c.name}`,
      title: c.name,
      hint: c.command,
      group: "pane",
      command: true,
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
      title: "Agents: New Agent…",
      hint: "pick a role ›",
      group: "agents",
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
      title: "Agents: Start Team…",
      hint: "pick a team ›",
      group: "agents",
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
              t.source === "repo"
                ? "repo"
                : t.source === "builtin"
                  ? "built-in"
                  : null,
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
      title: "Pane: SSH…",
      hint: "pick a host ›",
      group: "pane",
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
    // A step with nothing in it is not offered; "Agents: New Agent…" always
    // has plain Claude Code.
    const steps = [agents, teamStep, ssh].filter((st) => st.entries.length > 0);
    return { entries: [...base, ...custom, ...tabEntries], steps };
  }, [customCommands, roles, teams, hosts, keybindings, tabs]);

  const step = steps.find((st) => st.id === stepId) ?? null;

  const sections = useMemo<Section[]>(() => {
    const rank = <T,>(items: T[], text: (t: T) => string) =>
      items
        .map((item) => ({ item, score: fuzzyScore(query, text(item)) }))
        .filter((x): x is { item: T; score: number } => x.score !== null)
        .sort((a, b) => b.score - a.score)
        .map((x) => x.item);
    if (step) {
      const rows = rank(step.entries, (e) => e.title).map((entry) => ({ kind: "entry" as const, entry }));
      return [{ label: null, rows }];
    }
    if (!query) {
      // What was run last, when it can still run: a step's entry by the
      // name it has from the top ("New Agent › reviewer").
      const runnable = new Map<string, Entry>(entries.map((e) => [e.id, e]));
      for (const st of steps) {
        for (const e of st.entries) runnable.set(e.id, { ...e, title: `${st.crumb} › ${e.title}` });
      }
      const recentRows: Row[] = recent.flatMap((id) => {
        const entry = runnable.get(id);
        return entry ? [{ kind: "entry" as const, entry, key: `recent:${id}` }] : [];
      });
      // Then each group's steps and commands, in the actions' order.
      const groups = GROUPS.map(([group, label]) => ({
        label,
        rows: [
          ...steps.filter((st) => st.group === group).map((st) => ({ kind: "step" as const, step: st })),
          ...entries.filter((e) => e.group === group).map((entry) => ({ kind: "entry" as const, entry })),
        ],
      }));
      return [{ label: "RECENT", rows: recentRows }, ...groups].filter((sec) => sec.rows.length > 0);
    }
    const top: Row[] = rank<Row>(
      [
        ...entries.map((entry) => ({ kind: "entry" as const, entry })),
        ...steps.map((st) => ({ kind: "step" as const, step: st })),
      ],
      (r) => (r.kind === "entry" ? r.entry.title : r.step.title),
    );
    // What a step holds is still found from the top by its name — a
    // `reviewer` you know needs no second step — below the commands, so a
    // dozen roles never crowd them out.
    const deep: Row[] = steps.flatMap((st) =>
      rank(st.entries, (e) => e.title).map((e) => ({
        kind: "entry" as const,
        entry: { ...e, id: `deep:${e.id}`, title: `${st.crumb} › ${e.title}` },
      })),
    );
    return [{ label: null, rows: [...top, ...deep] }];
  }, [entries, steps, step, query, recent]);
  // The rows in the order shown, which the arrow keys walk.
  const filtered = useMemo(() => sections.flatMap((sec) => sec.rows), [sections]);
  const found = filtered.findIndex((r) => rowId(r) === selectedId);
  const selected = found >= 0 ? found : Math.min(lastIndex.current, filtered.length - 1);
  lastIndex.current = Math.max(selected, 0);
  const select = (i: number) => {
    lastIndex.current = i;
    setSelectedId(filtered[i] ? rowId(filtered[i]) : null);
  };

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
      setRecent(readRecent());
      lastIndex.current = 0;
      setSelectedId(null);
      // Opened on a step (the wall's "Start a team…"), else the top level.
      // A step with no entries yet is shown once they are read.
      setStepId(useUiStore.getState().paletteStep);
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

  useEffect(() => {
    lastIndex.current = 0;
    setSelectedId(null);
  }, [query, stepId]);

  // Keep the selected row in view as the arrows walk past the list's edge.
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    listRef.current?.querySelector(".palette-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [selected, sections]);

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
    // By the id it has inside its step, so a deep entry and the same
    // entry picked in its step are one recent command.
    rememberRecent(row.entry.id.replace(/^deep:/, ""));
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
                select(Math.min(selected + 1, filtered.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                select(Math.max(selected - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                choose(filtered[selected]);
              }
              e.stopPropagation();
            }}
          />
        </div>
        <div className="palette-list" ref={listRef}>
          {sections.map((sec, si) => {
            const offset = sections.slice(0, si).reduce((n, x) => n + x.rows.length, 0);
            return (
              <div key={sec.label ?? "ranked"}>
                {sec.label && <div className="palette-group">{sec.label}</div>}
                {sec.rows.map((row, ri) => {
                  const i = offset + ri;
                  const title = row.kind === "entry" ? row.entry.title : row.step.title;
                  const hint = row.kind === "entry" ? row.entry.hint : row.step.hint;
                  const accel = row.kind === "entry" ? row.entry.accel : undefined;
                  const command = row.kind === "entry" && row.entry.command;
                  return (
                    <div
                      key={rowId(row)}
                      className={`palette-item${i === selected ? " selected" : ""}`}
                      onMouseEnter={() => select(i)}
                      onClick={() => choose(row)}
                    >
                      <span className="palette-title">{title}</span>
                      <span className={`palette-hint${command ? " command" : ""}`}>{hint}</span>
                      {accel && <kbd className="palette-accel">{accel}</kbd>}
                    </div>
                  );
                })}
              </div>
            );
          })}
          {filtered.length === 0 && (
            <div className="palette-empty">
              {step ? step.empty : "No matching commands"}
            </div>
          )}
        </div>
        <div className="palette-foot">
          Groups: <span>Agents</span> · <span>Git</span> · <span>Pane</span> · <span>View</span> · <span>App</span>
        </div>
      </div>
    </div>
  );
}
