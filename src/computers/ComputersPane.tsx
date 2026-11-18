import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Computer,
  ComputerCheck,
  addComputer,
  closePane,
  focusPane,
  listComputers,
  removeComputer,
  sshHosts,
  testComputer,
  updateComputer,
} from "../bindings";
import { useUiStore } from "../state/uiStore";

type Props = {
  paneId: string;
  focused: boolean;
};

/** The form: a saved computer's fields as edited, or a new one's
 *  (`original` null) until it is saved. */
type Form = { original: string | null; name: string; host: string; dir: string };

/** A test's answer, with the host and directory it was asked about. */
type Tested = { host: string; dir: string; check?: ComputerCheck; error?: string };

const BLANK: Form = { original: null, name: "", host: "", dir: "" };
const formOf = (c: Computer): Form => ({ original: c.name, name: c.name, host: c.host, dir: c.dir });
const same = (f: Form, c: Computer) => f.name === c.name && f.host === c.host && f.dir === c.dir;

/**
 * The computers saved in mirador.json: the list on the left, the chosen
 * one, or a new one, as a form on the right, which saves to the file,
 * removes from it, and tests what it says before it is saved. The list is
 * read again whenever the file changes on disk; a file that does not parse
 * shows why, and nothing is offered that it would refuse.
 */
export function ComputersPane({ paneId, focused }: Props) {
  const [computers, setComputers] = useState<Computer[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  // Another computer, or a new one, asked for while this one has unsaved
  // changes: it waits for Discard.
  const [pending, setPending] = useState<Form | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [testing, setTesting] = useState(false);
  const [tested, setTested] = useState<Tested | null>(null);
  const [hosts, setHosts] = useState<string[]>([]);
  // Each new draft is a form of its own, mounted afresh, so its Name takes
  // the caret even when the last form was a draft too.
  const [drafts, setDrafts] = useState(0);
  const draftAsked = useUiStore((s) => s.computerDraft);

  // Two reads in a row can come back in either order: only the latest one
  // is shown. The list shown before it, to tell a form left as it was from
  // one the person changed.
  const latest = useRef(0);
  const shown = useRef<Computer[]>([]);
  const read = useCallback(async (): Promise<Computer[] | null> => {
    const n = ++latest.current;
    try {
      const list = await listComputers();
      if (n === latest.current) {
        const before = shown.current;
        shown.current = list;
        setComputers(list);
        setListError(null);
        // A form as its computer was follows the file: changed there, it
        // shows the change; gone, the first one is chosen instead. A form
        // the person changed stays as typed.
        setForm((f) => {
          const was = f?.original != null ? before.find((c) => c.name === f.original) : undefined;
          if (!f || !was || !same(f, was)) return f;
          const now = list.find((c) => c.name === f.original);
          return now ? formOf(now) : null;
        });
      }
      return list;
    } catch (e: unknown) {
      if (n === latest.current) setListError(String(e));
      return null;
    }
  }, []);

  useEffect(() => {
    void read();
    void sshHosts().then(setHosts, () => setHosts([]));
    const unlisten = listen("config-file-changed", () => void read());
    return () => void unlisten.then((f) => f());
  }, [read]);

  const saved = form?.original != null ? computers.find((c) => c.name === form.original) : undefined;
  // A new one once anything is typed in it; a saved one once it differs.
  const dirty = !form ? false : saved ? !same(form, saved) : form.name + form.host + form.dir !== "";
  // Nothing the file would refuse is offered while it cannot be read.
  const locked = listError !== null;
  // Edited here, then renamed or removed in the file: kept as typed, and
  // saved as a new one.
  const gone = form?.original != null && !saved && !locked;

  const open = (next: Form | null) => {
    if (next?.original === null) setDrafts((n) => n + 1);
    setForm(next);
    setPending(null);
    setStatus(null);
    setConfirmRemove(false);
    setTested(null);
  };
  const go = (next: Form) => {
    if (dirty && !(form?.original != null && form.original === next.original)) setPending(next);
    else open(next);
  };

  // With nothing chosen, the first.
  useEffect(() => {
    if (!form) open(computers[0] ? formOf(computers[0]) : null);
  }, [computers, form]);

  // The palette's "Add a computer": a new draft, here.
  useEffect(() => {
    if (!draftAsked) return;
    useUiStore.getState().setComputerDraft(false);
    // `go` as it is now: it asks first about unsaved changes.
    go(BLANK);
  }, [draftAsked]);

  const change = (patch: Partial<Form>) => {
    if (!form) return;
    setForm({ ...form, ...patch });
    setStatus(null);
    // A result about another host or directory no longer describes these.
    if (patch.host !== undefined || patch.dir !== undefined) setTested(null);
  };

  const test = () => {
    if (!form) return;
    const { host, dir } = form;
    setTesting(true);
    setTested(null);
    testComputer(host, dir)
      .then((check) => setTested({ host, dir, check }))
      .catch((e: unknown) => setTested({ host, dir, error: String(e) }))
      .finally(() => setTesting(false));
  };
  // Only while it still describes what the form says.
  const result = tested && form && tested.host === form.host && tested.dir === form.dir ? tested : null;

  const save = () => {
    if (!form || locked) return;
    const { original, name, host, dir } = form;
    (original === null || !saved ? addComputer(name, host, dir) : updateComputer(original, name, host, dir))
      .then(async (c) => {
        await read();
        setForm(formOf(c));
        setStatus(null);
      })
      .catch((e: unknown) => setStatus(String(e)));
  };

  const remove = () => {
    if (form?.original == null) return;
    const at = computers.findIndex((c) => c.name === form.original);
    removeComputer(form.original)
      .then(async () => {
        const list = (await read()) ?? [];
        const next = list[Math.min(Math.max(at, 0), list.length - 1)];
        open(next ? formOf(next) : null);
      })
      .catch((e: unknown) => {
        setConfirmRemove(false);
        setStatus(String(e));
      });
  };

  const isNew = form?.original === null;

  return (
    <div
      className={`pane agent-editor${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Computers</span>
        <span className="graph-repo">mirador.json</span>
        <button className="graph-refresh" title="Read the file again" onClick={() => void read()}>
          ⟳
        </button>
        <button className="graph-close" title="Close" onClick={() => void closePane(paneId)}>
          ✕
        </button>
      </div>
      {listError && <div className="editor-bar error">{listError}</div>}
      <div className="editor-body">
        <nav className="editor-list">
          <section>
            <div className="editor-section">
              <span>Computers</span>
            </div>
            {computers.map((c) => (
              <button
                key={c.name}
                className={`editor-item${form?.original === c.name ? " selected" : ""}`}
                title={`${c.host} · ${c.dir}`}
                onClick={() => go(formOf(c))}
              >
                <span className="editor-item-row">
                  <span className="editor-item-name">{c.name}</span>
                </span>
              </button>
            ))}
            {isNew && (
              <div className="editor-item selected" aria-current="true">
                <span className="editor-item-row">
                  <span className="editor-item-name">{form.name.trim() || "New computer"}</span>
                  <span className="editor-source computer-new">new</span>
                </span>
              </div>
            )}
            <button className="editor-item" disabled={locked} onClick={() => go(BLANK)}>
              + Add a computer
            </button>
          </section>
        </nav>
        <div className="editor-main">
          {pending && (
            <div className="editor-bar warn">
              <span>Unsaved changes to {form?.original ?? "the new computer"}.</span>
              <button onClick={() => open(pending)}>Discard</button>
              <button onClick={() => setPending(null)}>Keep editing</button>
            </div>
          )}
          {!form ? (
            !listError && (
              <p className="editor-empty">
                No computers saved yet. + Add a computer: an SSH host, and the directory there where
                agents work.
              </p>
            )
          ) : (
            <>
              <div className="editor-head">
                <div className="editor-head-text">
                  <span className="editor-title">
                    {isNew ? form.name.trim() || "New computer" : form.original}
                    {isNew && <span className="editor-kind">new computer</span>}
                  </span>
                  <span className="editor-path">mirador.json · computers</span>
                </div>
                {dirty && <span className="editor-unsaved">Unsaved</span>}
                {confirmRemove ? (
                  <>
                    <button className="danger" onClick={remove}>
                      Remove {form.original}
                    </button>
                    <button onClick={() => setConfirmRemove(false)}>Cancel</button>
                  </>
                ) : (
                  <>
                    {isNew ? (
                      <button onClick={() => open(computers[0] ? formOf(computers[0]) : null)}>Cancel</button>
                    ) : (
                      saved && (
                        <>
                          {dirty && <button onClick={() => open(formOf(saved))}>Revert</button>}
                          <button disabled={locked} onClick={() => setConfirmRemove(true)}>
                            Remove…
                          </button>
                        </>
                      )
                    )}
                    <button
                      className="primary"
                      disabled={locked || !dirty}
                      title="Save (⌘S / Ctrl+S)"
                      onClick={save}
                    >
                      Save
                    </button>
                  </>
                )}
              </div>
              {gone && (
                <div className="editor-bar warn">
                  <span>
                    {form.original} is no longer in mirador.json. Save adds it again as it is here.
                  </span>
                  <button onClick={() => open(null)}>Discard</button>
                </div>
              )}
              {status && <div className="editor-bar error">{status}</div>}
              <div
                key={form.original ?? `new:${drafts}`}
                className="computer-form"
                onKeyDown={(e) => {
                  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
                    e.preventDefault();
                    e.stopPropagation();
                    save();
                  }
                }}
              >
                <label className="inspector-field">
                  <span>Name</span>
                  <input
                    value={form.name}
                    disabled={locked}
                    spellCheck={false}
                    autoFocus={isNew}
                    onChange={(e) => change({ name: e.target.value })}
                  />
                </label>
                <label className="inspector-field">
                  <span>SSH host</span>
                  <input
                    value={form.host}
                    disabled={locked}
                    spellCheck={false}
                    list="computers-ssh-hosts"
                    onChange={(e) => change({ host: e.target.value })}
                  />
                  <span className="computer-hint">
                    An alias from ~/.ssh/config, or a destination such as -p 2222 me@host
                  </span>
                </label>
                <datalist id="computers-ssh-hosts">
                  {hosts.map((h) => (
                    <option key={h} value={h} />
                  ))}
                </datalist>
                <label className="inspector-field">
                  <span>Directory</span>
                  <input
                    className="computer-dir"
                    value={form.dir}
                    disabled={locked}
                    spellCheck={false}
                    onChange={(e) => change({ dir: e.target.value })}
                  />
                  <span className="computer-hint">Where agents work there. ~ is the home there.</span>
                </label>
                <div className="computer-test">
                  <button
                    disabled={locked || testing || !form.host.trim() || !form.dir.trim()}
                    title="Connect as an agent's start would, and look around: nothing is changed there"
                    onClick={test}
                  >
                    {testing ? "Testing… (up to 30s)" : "Test connection"}
                  </button>
                  {result && <TestResult tested={result} />}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** A test's answer as three rows, straight from the check; or ssh's error
 *  when the computer was not reached. Not reached is an error; the rest
 *  only warns, since the computer can be saved all the same. */
function TestResult({ tested }: { tested: Tested }) {
  const { check, host, dir } = tested;
  if (!check) {
    return (
      <div className="computer-check" role="status">
        <CheckRow level="error" text={tested.error ?? `${host} was not reached`} />
      </div>
    );
  }
  return (
    <div className="computer-check" role="status">
      <CheckRow level="ok" text={`Reached ${host} · ${check.system}`} />
      <CheckRow
        level={check.dirFound ? "ok" : "warn"}
        text={check.dirFound ? `Directory ${dir} found` : `Directory ${dir} not found`}
      />
      <CheckRow
        level={check.agents === null ? "ok" : "warn"}
        text={check.agents === null ? "Agents can run there" : `Agents can't run there yet: ${check.agents}`}
      />
    </div>
  );
}

const MARK = { ok: "✓", warn: "⚠", error: "✕" } as const;

function CheckRow({ level, text }: { level: keyof typeof MARK; text: string }) {
  return (
    <div className={`computer-check-row ${level}`}>
      <span className="computer-check-mark" aria-hidden="true">
        {MARK[level]}
      </span>
      <span>{text}</span>
    </div>
  );
}
