import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AgentFileKind,
  EditorCatalog,
  RoleSource,
  agentEditorCatalog,
  agentFileCheck,
  agentFileDelete,
  agentFileRead,
  agentFileSave,
  agentFileTemplate,
  closePane,
  focusPane,
  openAgent,
  startTeam,
} from "../bindings";

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
}

const SECTIONS: { kind: AgentFileKind; title: string }[] = [
  { kind: "role", title: "Roles" },
  { kind: "way", title: "Ways of working" },
  { kind: "team", title: "Teams" },
];

const SOURCE_LABEL: Record<RoleSource, string> = {
  repo: "repo",
  global: "yours",
  config: "mirador.json",
  builtin: "built-in",
};

function items(cat: EditorCatalog | null): Item[] {
  if (!cat) return [];
  const { roles, ways, teams, problems } = cat.catalog;
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
  ];
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

  const dirty = text !== saved;
  // mirador.json's entries are edited there; a built-in ships with the
  // app — both are read here, and a built-in can be copied to change it.
  const readOnly = sel?.source === "config" || sel?.source === "builtin";
  const list = useMemo(() => items(cat), [cat]);

  const refresh = useCallback(() => {
    agentEditorCatalog(paneId)
      .then(setCat)
      .catch((e: unknown) => setStatus(String(e)));
  }, [paneId]);
  useEffect(() => refresh(), [refresh]);
  // Files change outside the app too: coming back to the pane re-reads.
  const wasFocused = useRef(focused);
  useEffect(() => {
    if (focused && !wasFocused.current) refresh();
    wasFocused.current = focused;
  }, [focused, refresh]);

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

  const open = (item: Item) => {
    setPending(null);
    setConfirmDelete(false);
    setStatus(null);
    setIsNew(false);
    setSel(item);
    if (!item.path) {
      const role = cat?.catalog.roles.find((r) => r.name === item.name);
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
        if (now) open(now);
        setStatus(`Copied to ${SOURCE_LABEL[to]}: edit away`);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  // Leaving unsaved text asks first.
  const choose = (item: Item) => {
    if (dirty && !same(item, sel)) setPending(item);
    else open(item);
  };

  const save = () => {
    if (!sel || readOnly || !dirty || invalid) return;
    agentFileSave(paneId, sel.kind, sel.source, sel.name, text, isNew)
      .then((next) => {
        setCat(next);
        setSaved(text);
        setIsNew(false);
        const now = items(next).find((i) => same(i, sel));
        if (now) setSel(now);
        setStatus("Saved");
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

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

  const start = () => {
    if (!sel || dirty) return;
    const run =
      sel.kind === "role"
        ? openAgent(sel.name, null, paneId, false)
        : startTeam(sel.name, null, paneId);
    run.then(() => setStatus(null)).catch((e: unknown) => setStatus(String(e)));
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
                    className={`editor-item${same(i, sel) ? " selected" : ""}`}
                    title={i.problem ?? i.description ?? i.path ?? undefined}
                    onClick={() => choose(i)}
                  >
                    <span className="editor-item-name">{i.name}</span>
                    {i.problem && (
                      <span className="editor-item-problem">⚠</span>
                    )}
                    <span className={`editor-source ${i.source}`}>
                      {SOURCE_LABEL[i.source]}
                    </span>
                  </button>
                ))}
            </section>
          ))}
          {cat && (
            <p className="editor-where">
              repo: {cat.repoDir ?? "not in a repository"}
              <br />
              yours: {cat.globalDir}
            </p>
          )}
        </nav>
        <div className="editor-main">
          {pending && (
            <div className="editor-bar warn">
              <span>Unsaved changes to {sel?.name}.</span>
              <button onClick={() => open(pending)}>Discard</button>
              <button onClick={() => setPending(null)}>Keep editing</button>
            </div>
          )}
          {!sel ? (
            <p className="editor-empty">
              Pick a role, way of working or team on the left, or + to create
              one.
            </p>
          ) : (
            <>
              <div className="editor-head">
                <span className="editor-title">
                  {sel.name}
                  {dirty && " •"}
                </span>
                <span className="editor-path" title={sel.path ?? undefined}>
                  {sel.source === "config"
                    ? "agentRoles in mirador.json — read only here"
                    : sel.source === "builtin"
                      ? "ships with Mirador — copy it to change it; your copy replaces it"
                      : (sel.path ??
                        `new ${sel.kind}, in ${SOURCE_LABEL[sel.source]}`)}
                </span>
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
                      Copy to {SOURCE_LABEL[to]}
                    </button>
                  ))}
                {(sel.kind === "role" || sel.kind === "team") && !isNew && (
                  <button
                    disabled={dirty || !!sel.problem}
                    title={
                      dirty
                        ? "Save first"
                        : (sel.problem ?? `Start this ${sel.kind}`)
                    }
                    onClick={start}
                  >
                    Start
                  </button>
                )}
                {!readOnly && (
                  <button
                    className="primary"
                    disabled={!dirty || !!invalid}
                    title={invalid ?? "Save (⌘S / Ctrl+S)"}
                    onClick={save}
                  >
                    Save
                  </button>
                )}
                {!readOnly && dirty && !isNew && (
                  <button onClick={() => setText(saved)}>Revert</button>
                )}
                {sel.path && !readOnly && !confirmDelete && (
                  <button onClick={() => setConfirmDelete(true)}>Delete</button>
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
              </div>
              {invalid && <div className="editor-bar error">{invalid}</div>}
              {!invalid && sel.problem && !dirty && (
                <div className="editor-bar warn">{sel.problem}</div>
              )}
              {warnings.map((w) => (
                <div key={w} className="editor-bar warn">
                  {w}
                </div>
              ))}
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
                    save();
                  }
                }}
              />
            </>
          )}
          {status && <div className="editor-status">{status}</div>}
        </div>
      </div>
    </div>
  );
});
