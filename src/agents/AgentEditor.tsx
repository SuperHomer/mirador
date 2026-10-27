import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AgentFileKind,
  EditorCatalog,
  FileFields,
  RoleSource,
  agentEditorCatalog,
  agentEditorUsedBy,
  agentFileCheck,
  agentFileDelete,
  agentFileRead,
  agentFileSave,
  agentFileSaveFields,
  agentFileTemplate,
  closePane,
  focusPane,
  openAgent,
  startTeam,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { MemberInspector } from "./MemberInspector";
import { RolePage } from "./RolePage";
import { roleFieldsOf } from "./RoleForm";
import { WayPage } from "./WayPage";
import { TeamPage } from "./TeamPage";
import { Confirm, RestartConfirm, restart, teamRuns } from "./teamRestart";
import { useDiskCheck } from "./useDiskCheck";

type Props = {
  paneId: string;
  cwd: string | null;
  focused: boolean;
};

/** One file in the list, or one being created. */
interface Item {
  kind: AgentFileKind;
  name: string;
  source: RoleSource;
  /** Null for an `agentRoles` entry, and for a file not yet saved. */
  path: string | null;
  description: string | null;
  problem: string | null;
  /** A file hidden by one of the same name that comes first. */
  shadowedBy?: { source: RoleSource; path: string | null };
}

const SECTIONS: { kind: AgentFileKind; title: string }[] = [
  { kind: "team", title: "Teams" },
  { kind: "role", title: "Roles" },
  { kind: "way", title: "Ways of working" },
];

const SOURCE_LABEL: Record<RoleSource, string> = {
  repo: "repo",
  global: "yours",
  config: "mirador.json",
  builtin: "built-in",
};

/** What the primary action does after saving, by kind; for a file that
 *  cannot be saved here, the same without the save. */
const PRIMARY: Record<AgentFileKind, string> = {
  team: "Save & restart team…",
  role: "Save & start ▸",
  way: "Save & restart users…",
};
const PRIMARY_READ_ONLY: Record<AgentFileKind, string> = {
  team: "Restart team…",
  role: "Start ▸",
  way: "Restart users…",
};

function items(cat: EditorCatalog | null): Item[] {
  if (!cat) return [];
  const { roles, ways, teams, problems, shadowed } = cat.catalog;
  // A way has no problem of its own on the catalog; its file's line does.
  const problemOf = (path: string) =>
    problems.find((p) => p.startsWith(`${path}: `))?.slice(path.length + 2) ??
    null;
  return [
    ...roles.map((r) => ({
      kind: "role" as const,
      name: r.name,
      source: r.source,
      path: r.path,
      description: r.description,
      problem: r.problem,
    })),
    ...ways.map((w) => ({
      kind: "way" as const,
      name: w.name,
      source: w.source,
      path: w.path,
      description: w.description,
      problem: problemOf(w.path),
    })),
    ...teams.map((t) => ({
      kind: "team" as const,
      name: t.name,
      source: t.source,
      path: t.path,
      description: t.description,
      problem: t.problem,
    })),
    ...shadowed.map((f) => ({
      kind: f.kind,
      name: f.name,
      source: f.source,
      path: f.path,
      description: null,
      problem: null,
      shadowedBy: { source: f.by, path: f.byPath },
    })),
  ];
}

/** `item` as its page's form edits it, from what the catalog read of its
 *  file — none for a file the catalog does not hold as one in use (a
 *  shadowed one, one not saved yet) or for an `agentRoles` entry. */
function formOf(cat: EditorCatalog | null, item: Item): FileFields | null {
  if (!cat || !item.path || item.shadowedBy) return null;
  const mine = <T extends { name: string; source: RoleSource }>(list: T[]) =>
    list.find((x) => x.name === item.name && x.source === item.source);
  switch (item.kind) {
    case "team": {
      const t = mine(cat.catalog.teams);
      return t
        ? { kind: "team", description: t.description, way: t.way, layout: t.layout, members: t.members, charter: t.prompt }
        : null;
    }
    case "role": {
      const r = mine(cat.catalog.roles);
      return r ? { kind: "role", ...roleFieldsOf(r) } : null;
    }
    case "way": {
      const w = mine(cat.catalog.ways);
      return w ? { kind: "way", description: w.description, prompt: w.prompt } : null;
    }
  }
}

const same = (a: Item | null, b: Item | null) =>
  !!a && !!b && a.kind === b.kind && a.name === b.name && a.source === b.source;

/**
 * The role, way-of-working and team files the pane's directory sees —
 * the repository's `.mirador`, and yours — to read, edit, create and
 * delete. Rust decides what may be touched and whether a file parses; this
 * lists, edits text, and asks.
 */
export const AgentEditor = memo(function AgentEditor({
  paneId,
  cwd,
  focused,
}: Props) {
  const [cat, setCat] = useState<EditorCatalog | null>(null);
  const [sel, setSel] = useState<Item | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [text, setText] = useState("");
  const [saved, setSaved] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [invalid, setInvalid] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [creating, setCreating] = useState<{
    kind: AgentFileKind;
    name: string;
    source: RoleSource;
  } | null>(null);
  const [pending, setPending] = useState<Item | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  // A team is edited as a form; its file as text only when asked for.
  const [form, setForm] = useState<FileFields | null>(null);
  const [formSaved, setFormSaved] = useState("");
  const [raw, setRaw] = useState(false);
  const [pendingRaw, setPendingRaw] = useState<boolean | null>(null);
  const [member, setMember] = useState<number | null>(null);
  // Whether Save is asking before it writes over a file changed on disk.
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const [refreshed, setRefreshed] = useState(0);
  // The selected member's role has unsaved edits in the inspector.
  const [roleDirty, setRoleDirty] = useState(false);
  // What waits on the member's unsaved role edits: what to say, and what
  // to do once they are discarded.
  const [pendingRole, setPendingRole] = useState<{ message: string; then: () => void } | null>(null);
  // Bumped to drop the inspector's role edits: it starts again.
  const [roleEpoch, setRoleEpoch] = useState(0);
  const charterRef = useRef<HTMLTextAreaElement>(null);

  const formMode = form !== null && !raw;
  const dirty = formMode ? JSON.stringify(form) !== formSaved : text !== saved;
  // mirador.json's entries are edited there; a built-in ships with the
  // app — both are read here, and a built-in can be copied to change it.
  const readOnly = sel?.source === "config" || sel?.source === "builtin";
  const list = useMemo(() => items(cat), [cat]);

  const refresh = useCallback(() => {
    agentEditorCatalog(paneId)
      .then((c) => {
        setCat(c);
        setRefreshed((n) => n + 1);
      })
      .catch((e: unknown) => setStatus(String(e)));
  }, [paneId]);
  useEffect(() => refresh(), [refresh]);
  // Files change outside the app too: coming back to the pane re-reads.
  const wasFocused = useRef(focused);
  useEffect(() => {
    if (focused && !wasFocused.current) refresh();
    wasFocused.current = focused;
  }, [focused, refresh]);

  // The open file changed on disk: taken as it is when nothing is
  // unsaved — the form and the text alike — else the edits stay and the
  // header says so.
  const [stale, setStale] = useDiskCheck(
    paneId,
    sel && !isNew && sel.source !== "config" ? sel.path : null,
    refreshed,
    saved,
    dirty,
    (t) => {
      setText(t);
      setSaved(t);
      if (sel) resetForm(formOf(cat, sel));
    },
    (e) => setStatus(String(e)),
  );

  // Checked as you type: why it cannot be saved, or what is ignored.
  useEffect(() => {
    if (!sel || readOnly) return;
    const t = setTimeout(() => {
      agentFileCheck(sel.kind, sel.name, text)
        .then((w) => {
          setWarnings(w);
          setInvalid(null);
        })
        .catch((e: unknown) => {
          setWarnings([]);
          setInvalid(String(e));
        });
    }, 300);
    return () => clearTimeout(t);
  }, [sel, text, readOnly]);

  const resetForm = (f: FileFields | null) => {
    setForm(f);
    setFormSaved(f ? JSON.stringify(f) : "");
  };

  const open = (item: Item, catalog = cat) => {
    setPending(null);
    setPendingRaw(null);
    setConfirmDelete(false);
    setMenuOpen(false);
    setConfirm(null);
    setStatus(null);
    setIsNew(false);
    setSel(item);
    setRaw(false);
    setMember(null);
    setPendingRole(null);
    setRoleDirty(false);
    setStale(false);
    setConfirmOverwrite(false);
    resetForm(formOf(catalog, item));
    if (!item.path) {
      // An `agentRoles` entry, shown as the catalog has it — unless it is
      // shadowed, when the catalog has only the one used instead.
      const role = item.shadowedBy
        ? undefined
        : cat?.catalog.roles.find((r) => r.name === item.name);
      const body = role ? JSON.stringify(role, null, 2) : "";
      setText(body);
      setSaved(body);
      return;
    }
    agentFileRead(paneId, item.path)
      .then((t) => {
        setText(t);
        setSaved(t);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  // A built-in's text as a file of the same name — which, being yours or
  // the repository's, replaces the built-in from then on.
  const copyTo = (to: RoleSource) => {
    if (!sel) return;
    const copy = { ...sel, source: to };
    agentFileSave(paneId, sel.kind, to, sel.name, text, true)
      .then((next) => {
        setCat(next);
        const now = items(next).find((i) => same(i, copy));
        if (now) open(now, next);
        setStatus(`Copied to ${SOURCE_LABEL[to]}: edit away`);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  // Leaving unsaved text asks first.
  const choose = (item: Item) => {
    if ((dirty || roleDirty) && !same(item, sel)) setPending(item);
    else open(item);
  };

  // The page of the team, role or way `name` in use, as choosing it in
  // the list opens it.
  const openNamed = (kind: AgentFileKind, name: string) => {
    const item = list.find((i) => i.kind === kind && i.name === name && !i.shadowedBy);
    if (item) choose(item);
  };

  // Another member drops the role edits of this one: asked first.
  const selectMember = (i: number | null) => {
    if (roleDirty && i !== member) {
      setPendingRole({ message: "Unsaved changes to this member's role.", then: () => setMember(i) });
    } else setMember(i);
  };
  const removeMember = (i: number) => {
    const remove = () => {
      setForm((f) => (f?.kind === "team" ? { ...f, members: f.members.filter((_, j) => j !== i) } : f));
      setMember(null);
    };
    if (roleDirty) {
      setPendingRole({ message: "Unsaved changes to this member's role: removing the member drops them.", then: remove });
    } else remove();
  };

  /** Saves what is unsaved; whether there is nothing left unsaved. Over a
   *  file changed on disk only when `overwrite`: else it asks first. */
  const save = (overwrite = false): Promise<boolean> => {
    if (!sel || readOnly || !dirty) return Promise.resolve(true);
    if (stale && !overwrite) {
      setConfirmOverwrite(true);
      return Promise.resolve(false);
    }
    const written = () => {
      setStale(false);
      setConfirmOverwrite(false);
    };
    if (formMode && sel.path) {
      const path = sel.path;
      return agentFileSaveFields(paneId, sel.source, sel.name, path, form!, false)
        .then((next) => {
          setCat(next);
          setFormSaved(JSON.stringify(form));
          written();
          setStatus("Saved");
          // The file as written, for its raw view.
          return agentFileRead(paneId, path).then((t) => {
            setText(t);
            setSaved(t);
            return true;
          });
        })
        .catch((e: unknown) => {
          setStatus(String(e));
          return false;
        });
    }
    if (invalid) return Promise.resolve(false);
    return agentFileSave(paneId, sel.kind, sel.source, sel.name, text, isNew)
      .then((next) => {
        setCat(next);
        setSaved(text);
        written();
        setIsNew(false);
        const now = items(next).find((i) => same(i, sel));
        // A file just created opens as any other, on its form.
        if (now) isNew ? open(now, next) : setSel(now);
        setStatus("Saved");
        return true;
      })
      .catch((e: unknown) => {
        setStatus(String(e));
        return false;
      });
  };

  // Between the form and the file's text: what is unsaved in the one is
  // dropped (after asking) — the other starts from the file as saved.
  const showRaw = (toRaw: boolean) => {
    setPendingRaw(null);
    if (toRaw) {
      if (formSaved) setForm(JSON.parse(formSaved) as FileFields);
    } else {
      setText(saved);
      if (sel) resetForm(formOf(cat, sel));
    }
    setRaw(toRaw);
  };
  const switchView = (toRaw: boolean) => (dirty ? setPendingRaw(toRaw) : showRaw(toRaw));

  const remove = () => {
    if (!sel?.path) return;
    agentFileDelete(paneId, sel.path)
      .then((next) => {
        setCat(next);
        setSel(null);
        setText("");
        setSaved("");
        setConfirmDelete(false);
        setStatus(`Deleted ${sel.name}`);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  const create = () => {
    if (!creating) return;
    const name = creating.name.trim();
    if (!name) return;
    const { kind, source } = creating;
    agentFileTemplate(kind)
      .then((t) => {
        setCreating(null);
        setPending(null);
        setSel({
          kind,
          name,
          source,
          path: null,
          description: null,
          problem: null,
        });
        setIsNew(true);
        setText(t);
        setSaved("");
        setStatus(null);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  // Save, then what the page is for: start the role; start the team, or
  // restart what of it can be; restart the teams following the way. A
  // restart is confirmed in the header first.
  const primary = async (roleChecked = false) => {
    if (!sel) return;
    // The role is saved apart from the team: starting with its edits
    // unsaved would start it as it was.
    if (sel.kind === "team" && roleDirty && !roleChecked) {
      setPendingRole({
        message:
          "This member's role has unsaved edits, which the team's Save does not write: Save role first, or discard them and go on.",
        then: () => void primary(true),
      });
      return;
    }
    if (!(await save())) return;
    const fail = (e: unknown) => setStatus(String(e));
    const snapshot = useWorkspaceStore.getState().snapshot;
    if (sel.kind === "role") {
      openAgent(sel.name, null, paneId, false)
        .then(() => setStatus(`Started ${sel.name}`))
        .catch(fail);
    } else if (sel.kind === "team") {
      const runs = teamRuns(snapshot, (team) => team === sel.name);
      if (runs.length === 0) {
        startTeam(sel.name, null, paneId)
          .then(() => setStatus(`Started team ${sel.name}`))
          .catch(fail);
      } else {
        setConfirm({ runs, keep: runs.flatMap((r) => r.running) });
      }
    } else {
      const users = await agentEditorUsedBy(paneId, "way", sel.name).catch(
        (e: unknown) => {
          fail(e);
          return null;
        },
      );
      if (!users) return;
      // Every member of a team that follows it, and any member whose role
      // does, whatever its team's own way.
      const teams = users.filter((u) => u.kind === "team").map((u) => u.name);
      const roles = users.filter((u) => u.kind === "role").map((u) => u.name);
      const runs = teamRuns(snapshot, (team, m) => teams.includes(team) || roles.includes(m.role));
      // Agents started with a role that follows it, outside a team.
      const alone = (snapshot?.agents ?? [])
        .filter((a) => !a.member && a.role && roles.includes(a.role))
        .map((a) => a.name ?? a.role!);
      setConfirm({ runs, keep: [...runs.flatMap((r) => r.running), ...alone] });
    }
  };

  // What the header says under the title: where the file is, and whether
  // it is shared, hides one of the same name, or is hidden by one.
  const where = (s: Item) => {
    const base =
      s.source === "config"
        ? "agentRoles in mirador.json — read only here"
        : s.source === "builtin"
          ? "ships with Mirador — copy it to change it; your copy replaces it"
          : (s.path ?? `new ${s.kind}, in ${SOURCE_LABEL[s.source]}`);
    if (s.shadowedBy) {
      return `${base} · shadowed by ${SOURCE_LABEL[s.shadowedBy.source]}'s, which is used instead`;
    }
    const hides = (cat?.catalog.shadowed ?? [])
      .filter((f) => f.kind === s.kind && f.name === s.name && f.by === s.source)
      .map((f) => SOURCE_LABEL[f.source]);
    return [
      base,
      s.source === "repo" && !isNew ? "shared through the repository" : null,
      hides.length ? `shadows ${hides.join(", ")}` : null,
    ]
      .filter(Boolean)
      .join(" · ");
  };

  return (
    <div
      className={`pane agent-editor${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Agents</span>
        <span className="graph-repo" title={cwd ?? undefined}>
          {cwd?.split("/").pop() ?? "roles, ways and teams"}
        </span>
        <button
          className="graph-refresh"
          title="Read the files again"
          onClick={refresh}
        >
          ⟳
        </button>
        <button
          className="graph-close"
          title="Close"
          onClick={() => void closePane(paneId)}
        >
          ✕
        </button>
      </div>
      <div className="editor-body">
        <nav className="editor-list">
          {SECTIONS.map(({ kind, title }) => (
            <section key={kind}>
              <div className="editor-section">
                <span>{title}</span>
                <button
                  title={`New ${kind}`}
                  onClick={() =>
                    setCreating({
                      kind,
                      name: "",
                      source: cat?.repoDir ? "repo" : "global",
                    })
                  }
                >
                  +
                </button>
              </div>
              {creating?.kind === kind && (
                <div className="editor-create">
                  <input
                    autoFocus
                    placeholder="name"
                    value={creating.name}
                    onChange={(e) =>
                      setCreating({ ...creating, name: e.target.value })
                    }
                    onKeyDown={(e) => {
                      if (e.key === "Enter") create();
                      if (e.key === "Escape") setCreating(null);
                    }}
                  />
                  <select
                    value={creating.source}
                    onChange={(e) =>
                      setCreating({
                        ...creating,
                        source: e.target.value as RoleSource,
                      })
                    }
                  >
                    <option value="repo" disabled={!cat?.repoDir}>
                      repo
                    </option>
                    <option value="global">yours</option>
                  </select>
                  <button onClick={create}>Create</button>
                </div>
              )}
              {list
                .filter((i) => i.kind === kind)
                .map((i) => (
                  <button
                    key={`${i.source}:${i.name}`}
                    className={`editor-item${same(i, sel) ? " selected" : ""}${i.shadowedBy ? " shadowed" : ""}`}
                    title={
                      i.shadowedBy
                        ? `Not used: ${SOURCE_LABEL[i.shadowedBy.source]}'s ${i.name} comes first${i.shadowedBy.path ? ` (${i.shadowedBy.path})` : ""}`
                        : (i.description ?? i.path ?? undefined)
                    }
                    onClick={() => choose(i)}
                  >
                    <span className="editor-item-row">
                      <span className="editor-item-name">{i.name}</span>
                      <span className={`editor-source ${i.source}`}>
                        {SOURCE_LABEL[i.source]}
                        {i.shadowedBy && " · shadowed"}
                      </span>
                    </span>
                    {i.problem && (
                      <span className="editor-item-problem">{i.problem}</span>
                    )}
                  </button>
                ))}
            </section>
          ))}
          <p
            className="editor-where"
            title={
              cat
                ? `repo: ${cat.repoDir ?? "not in a repository"}\nyours: ${cat.globalDir}`
                : undefined
            }
          >
            Same name: repo › yours › config › built-in
          </p>
        </nav>
        <div className="editor-main">
          {pending && (
            <div className="editor-bar warn">
              <span>Unsaved changes to {sel?.name}.</span>
              <button onClick={() => open(pending)}>Discard</button>
              <button onClick={() => setPending(null)}>Keep editing</button>
            </div>
          )}
          {pendingRole && (
            <div className="editor-bar warn">
              <span>{pendingRole.message}</span>
              <button
                onClick={() => {
                  const { then } = pendingRole;
                  setPendingRole(null);
                  setRoleEpoch((n) => n + 1);
                  setRoleDirty(false);
                  then();
                }}
              >
                Discard
              </button>
              <button onClick={() => setPendingRole(null)}>Keep editing</button>
            </div>
          )}
          {pendingRaw !== null && (
            <div className="editor-bar warn">
              <span>Unsaved changes to {sel?.name}.</span>
              <button onClick={() => showRaw(pendingRaw)}>Discard</button>
              <button onClick={() => setPendingRaw(null)}>Keep editing</button>
            </div>
          )}
          {!sel ? (
            <p className="editor-empty">
              Pick a team, role or way of working on the left, or + to create
              one.
            </p>
          ) : (
            <>
              <div className="editor-head">
                <div className="editor-head-text">
                  <span className="editor-title">
                    {sel.name}
                    <span className="editor-kind">{sel.kind}</span>
                  </span>
                  <span className="editor-path" title={sel.path ?? undefined}>
                    {where(sel)}
                  </span>
                </div>
                {stale && (
                  <>
                    <span className="editor-unsaved" title="Edited outside the app since it was opened">
                      Changed on disk
                    </span>
                    <button title="Discard your edits and read the file again" onClick={() => open(sel)}>
                      Reload
                    </button>
                  </>
                )}
                {dirty && !readOnly && !stale && (
                  <span className="editor-unsaved">Unsaved</span>
                )}
                {form && (
                  <button
                    className="editor-link"
                    title={raw ? "Back to the form" : "The file as text, frontmatter and all"}
                    onClick={() => switchView(!raw)}
                  >
                    {raw ? "Edit as form" : "Edit raw file"}
                  </button>
                )}
                {!readOnly && !isNew && !confirmDelete && (
                  <span className="editor-menu-anchor">
                    <button title="More" onClick={() => setMenuOpen((o) => !o)}>
                      ⋯
                    </button>
                    {menuOpen && (
                      <div className="editor-menu">
                        <button
                          disabled={!dirty}
                          onClick={() => {
                            setMenuOpen(false);
                            if (formMode) setForm(JSON.parse(formSaved) as FileFields);
                            else setText(saved);
                          }}
                        >
                          Revert
                        </button>
                        {sel.path && (
                          <button
                            className="danger"
                            onClick={() => {
                              setMenuOpen(false);
                              setConfirmDelete(true);
                            }}
                          >
                            Delete…
                          </button>
                        )}
                      </div>
                    )}
                  </span>
                )}
                {confirmDelete && (
                  <>
                    <button className="danger" onClick={remove}>
                      Delete {sel.name}
                    </button>
                    <button onClick={() => setConfirmDelete(false)}>
                      Cancel
                    </button>
                  </>
                )}
                {sel.source === "builtin" &&
                  (["repo", "global"] as const).map((to) => (
                    <button
                      key={to}
                      disabled={to === "repo" && !cat?.repoDir}
                      title={
                        to === "repo" && !cat?.repoDir
                          ? "Not in a repository"
                          : `A file of the same name, in ${SOURCE_LABEL[to]}, which replaces the built-in`
                      }
                      onClick={() => copyTo(to)}
                    >
                      Save a copy in {SOURCE_LABEL[to]}
                    </button>
                  ))}
                {!readOnly && (
                  <button
                    disabled={!dirty || !!invalid}
                    title={invalid ?? "Save (⌘S / Ctrl+S)"}
                    onClick={() => void save()}
                  >
                    Save
                  </button>
                )}
                {!sel.shadowedBy && (
                  <button
                    className="primary"
                    disabled={!!invalid || (!dirty && !!sel.problem)}
                    title={invalid ?? (!dirty ? sel.problem : null) ?? undefined}
                    onClick={() => void primary()}
                  >
                    {(readOnly ? PRIMARY_READ_ONLY : PRIMARY)[sel.kind]}
                  </button>
                )}
              </div>
              {confirmOverwrite && (
                <div className="editor-bar confirm">
                  <span>
                    {sel.name} changed on disk since it was opened. Write your edits over it?
                  </span>
                  <button className="primary" onClick={() => void save(true)}>
                    Overwrite
                  </button>
                  <button onClick={() => setConfirmOverwrite(false)}>Cancel</button>
                </div>
              )}
              {confirm && (
                <RestartConfirm
                  confirm={confirm}
                  onRestart={() => {
                    const c = confirm;
                    setConfirm(null);
                    restart(c)
                      .then(setStatus)
                      .catch((e: unknown) => setStatus(String(e)));
                  }}
                  onCancel={() => setConfirm(null)}
                />
              )}
              {!formMode && invalid && <div className="editor-bar error">{invalid}</div>}
              {!invalid && sel.problem && !dirty && (
                <div className="editor-bar warn">{sel.problem}</div>
              )}
              {!formMode &&
                warnings.map((w) => (
                  <div key={w} className="editor-bar warn">
                    {w}
                  </div>
                ))}
              {formMode && cat ? (
                <div
                  className="editor-form"
                  onKeyDown={(e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
                      e.preventDefault();
                      e.stopPropagation();
                      void save();
                    }
                  }}
                >
                  {form!.kind === "team" ? (
                    <div className="team-layout">
                      <TeamPage
                        name={sel.name}
                        fields={form!}
                        onChange={(f) => {
                          setForm({ ...f, kind: "team" });
                          setStatus(null);
                        }}
                        catalog={cat.catalog}
                        readOnly={readOnly}
                        selected={member}
                        onSelect={selectMember}
                        charterRef={charterRef}
                      />
                      {member !== null && form!.members[member] ? (
                        <MemberInspector
                          key={`${member}:${form!.members[member].role}:${roleEpoch}`}
                          paneId={paneId}
                          cat={cat}
                          refreshed={refreshed}
                          team={sel.name}
                          fields={form!}
                          onFields={(f) => {
                            setForm({ ...f, kind: "team" });
                            setStatus(null);
                          }}
                          index={member}
                          teamReadOnly={readOnly}
                          teamDirty={dirty}
                          onCatalog={setCat}
                          onRoleDirty={setRoleDirty}
                          onRemove={() => removeMember(member)}
                          onOpenWay={(way) => openNamed("way", way)}
                          onEditCharter={() => charterRef.current?.focus()}
                        />
                      ) : (
                        <aside className="inspector empty">
                          <p className="inspector-note">Pick a member to see its settings, its role and what it is told.</p>
                        </aside>
                      )}
                    </div>
                  ) : form!.kind === "role" ? (
                    <RolePage
                      paneId={paneId}
                      name={sel.name}
                      form={form!}
                      onChange={(f) => {
                        setForm({ ...f, kind: "role" });
                        setStatus(null);
                      }}
                      cat={cat}
                      readOnly={readOnly}
                      refreshed={refreshed}
                      onOpen={openNamed}
                    />
                  ) : (
                    <WayPage
                      paneId={paneId}
                      name={sel.name}
                      form={form!}
                      onChange={(f) => {
                        setForm({ ...f, kind: "way" });
                        setStatus(null);
                      }}
                      readOnly={readOnly}
                      refreshed={refreshed}
                      onOpen={openNamed}
                    />
                  )}
                </div>
              ) : (
                <textarea
                  className="editor-text"
                  spellCheck={false}
                  readOnly={readOnly}
                  value={text}
                  onChange={(e) => {
                    setText(e.target.value);
                    setStatus(null);
                  }}
                  onKeyDown={(e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
                      e.preventDefault();
                      e.stopPropagation();
                      void save();
                    }
                  }}
                />
              )}
            </>
          )}
          {status && <div className="editor-status">{status}</div>}
        </div>
      </div>
    </div>
  );
});
