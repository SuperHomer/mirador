import { useEffect, useState } from "react";
import {
  AgentRole,
  EditorCatalog,
  PromptLayer,
  RoleFields,
  RoleSource,
  TeamFields,
  agentEditorTold,
  agentEditorUsedBy,
  agentFileRead,
  agentFileSaveFields,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { RoleFieldsEditor, RolePromptEditor, roleFieldsOf } from "./RoleForm";
import { Confirm, RestartConfirm, restart, teamRuns } from "./teamRestart";
import { useDiskCheck } from "./useDiskCheck";

type Props = {
  paneId: string;
  cat: EditorCatalog;
  refreshed: number;
  team: string;
  fields: TeamFields;
  onFields: (fields: TeamFields) => void;
  index: number;
  /** The team file cannot be saved here (a built-in). */
  teamReadOnly: boolean;
  /** The team form has unsaved edits. */
  teamDirty: boolean;
  onCatalog: (cat: EditorCatalog) => void;
  onRoleDirty: (dirty: boolean) => void;
  /** Asks the editor to remove this member: it asks first when the
   *  role has unsaved edits, which removing would drop. */
  onRemove: () => void;
  onOpenWay: (way: string) => void;
  onEditCharter: () => void;
};

type Tab = "settings" | "prompt" | "told";

const SOURCE_LABEL: Record<RoleSource, string> = {
  repo: "repo",
  global: "yours",
  config: "mirador.json",
  builtin: "built-in",
};


/** A member's seat names, as `teams::seats` gives them. */
const seatsOf = (base: string, count: number) =>
  count > 1 ? Array.from({ length: count }, (_, i) => `${base}-${i + 1}`) : [base];


/**
 * The selected member of a team page (17b, 17c): its place in the team,
 * which edits the team's form, and its role, which is a file of its own —
 * edited and saved here, apart from the team — then the role's prompt,
 * and what the member is told, layer by layer.
 */
export function MemberInspector(props: Props) {
  const { paneId, cat, refreshed, team, fields, onFields, index, teamReadOnly } = props;
  const m = fields.members[index];
  const role = cat.catalog.roles.find((r) => r.name === m.role);
  const [tab, setTab] = useState<Tab>("settings");
  const [form, setForm] = useState<RoleFields | null>(role ? roleFieldsOf(role) : null);
  const [formSaved, setFormSaved] = useState(form ? JSON.stringify(form) : "");
  // The role file's text as last loaded or saved, to tell a change on disk.
  const [text, setText] = useState("");
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [others, setOthers] = useState<string[]>([]);
  const [told, setTold] = useState<{ layers: PromptLayer[]; prompt: string | null } | null>(null);
  const [toldError, setToldError] = useState<string | null>(null);

  const dirty = !!form && JSON.stringify(form) !== formSaved;
  const path = role && role.source !== "config" ? role.path : null;
  const fail = (e: unknown) => setStatus(String(e));

  const resetFrom = (r: AgentRole | undefined) => {
    const f = r ? roleFieldsOf(r) : null;
    setForm(f);
    setFormSaved(f ? JSON.stringify(f) : "");
  };
  const load = (p: string | null) => {
    if (p) agentFileRead(paneId, p).then(setText).catch(fail);
  };
  useEffect(() => load(path), []);
  useEffect(() => props.onRoleDirty(dirty), [dirty]);
  useEffect(() => () => props.onRoleDirty(false), []);

  const [stale, setStale] = useDiskCheck(
    paneId,
    path,
    refreshed,
    text,
    dirty,
    (t) => {
      setText(t);
      resetFrom(role);
    },
    fail,
  );

  // The other teams with this role: what saving it changes besides.
  useEffect(() => {
    agentEditorUsedBy(paneId, "role", m.role)
      .then((users) => setOthers(users.filter((u) => u.kind === "team" && u.name !== team).map((u) => u.name)))
      .catch(() => setOthers([]));
  }, [refreshed, m.role]);

  // What the member's first seat is told, from the saved files.
  const seats = seatsOf(m.name ?? m.role, m.count);
  useEffect(() => {
    if (tab !== "told") return;
    agentEditorTold(paneId, team, seats[0])
      .then((t) => {
        setTold(t);
        setToldError(null);
      })
      .catch((e: unknown) => setToldError(String(e)));
  }, [tab, refreshed, cat, seats[0]]);

  const save = (to: RoleSource, overwrite = false) => {
    if (!role || !form) return;
    if (stale && !overwrite) {
      setConfirmOverwrite(true);
      return;
    }
    agentFileSaveFields(paneId, to, role.name, role.path, { kind: "role", ...form }, to !== role.source)
      .then((next) => {
        props.onCatalog(next);
        setFormSaved(JSON.stringify(form));
        setStale(false);
        setConfirmOverwrite(false);
        setStatus(to === role.source ? "Role saved" : `Role copied to ${SOURCE_LABEL[to]}`);
        load(next.catalog.roles.find((r) => r.name === role.name)?.path ?? null);
      })
      .catch(fail);
  };

  const askRestart = () => {
    const snapshot = useWorkspaceStore.getState().snapshot;
    const runs = teamRuns(snapshot, (t, s) => t === team && seats.includes(s.name));
    if (runs.length === 0) {
      setStatus(`Team ${team} is not running`);
      return;
    }
    setConfirm({ runs, keep: runs.flatMap((r) => r.running) });
  };

  const setMember = (patch: Partial<TeamFields["members"][number]>) =>
    onFields({ ...fields, members: fields.members.map((x, i) => (i === index ? { ...x, ...patch } : x)) });

  const warning = others.length > 0 && (
    <div className="inspector-warning">
      This role is also used by {others.length === 1 ? "team" : "teams"} {others.join(", ")}. Changing it affects{" "}
      {others.length === 1 ? "both" : "all of them"}.
    </div>
  );
  const editRole = !!form && role?.source !== "config";

  return (
    <aside className="inspector">
      <div className="inspector-head">
        <b>{m.name ?? m.role}</b> · member · role {m.role}
        {role && ` (${SOURCE_LABEL[role.source]})`}
      </div>
      <div className="inspector-tabs">
        {(
          [
            ["settings", "Settings"],
            ["prompt", "Role prompt"],
            ["told", "What it's told"],
          ] as const
        ).map(([id, label]) => (
          <button key={id} className={tab === id ? "selected" : ""} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </div>
      <div className="inspector-body">
        {stale && (
          <div className="editor-bar warn">
            <span>The role file changed on disk.</span>
            <button
              onClick={() => {
                setStale(false);
                setConfirmOverwrite(false);
                resetFrom(role);
                load(path);
              }}
            >
              Reload
            </button>
          </div>
        )}
        {confirmOverwrite && role && (
          <div className="editor-bar confirm">
            <span>{role.name} changed on disk. Write your edits over it?</span>
            <button className="primary" onClick={() => save(role.source, true)}>
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
              restart(c).then(setStatus).catch(fail);
            }}
            onCancel={() => setConfirm(null)}
          />
        )}

        {tab === "settings" && (
          <>
            <div className="inspector-label">IN THE TEAM</div>
            <label className="inspector-field">
              <span>Name in team</span>
              <input
                readOnly={teamReadOnly}
                placeholder={m.role}
                value={m.name ?? ""}
                onChange={(e) => setMember({ name: e.target.value.trim() ? e.target.value : null })}
              />
            </label>
            <label className="inspector-field">
              <span>Count</span>
              <input
                type="number"
                min={1}
                max={12}
                readOnly={teamReadOnly}
                value={m.count}
                onChange={(e) => setMember({ count: Math.max(1, Math.min(12, Number(e.target.value) || 1)) })}
              />
            </label>
            <label className="inspector-check">
              <input
                type="checkbox"
                disabled={teamReadOnly}
                checked={m.lead}
                onChange={(e) =>
                  // One lead: marking this one unmarks the others.
                  onFields({
                    ...fields,
                    members: fields.members.map((x, i) => ({ ...x, lead: i === index ? e.target.checked : e.target.checked ? false : x.lead })),
                  })
                }
              />
              Lead — the team's task goes to it
            </label>
            {!teamReadOnly && (
              <button
                className="inspector-remove"
                onClick={props.onRemove}
              >
                Remove member
              </button>
            )}

            <div className="inspector-label">ROLE {m.role.toUpperCase()}</div>
            {!role ? (
              <p className="inspector-note problem">No role `{m.role}` in this directory's roles.</p>
            ) : !form || role.source === "config" ? (
              <p className="inspector-note">An agentRoles entry: it is edited in mirador.json.</p>
            ) : (
              <>
                <RoleFieldsEditor form={form} onChange={setForm} cat={cat} />
                {warning}
              </>
            )}
          </>
        )}

        {tab === "prompt" &&
          (editRole && form ? (
            <>
              <RolePromptEditor form={form} onChange={setForm} />
              {warning}
              <p className="inspector-note">
                {form.way || fields.way
                  ? `After this come the ${form.way ?? fields.way} way of working and the team section.`
                  : "After this comes the team section."}
              </p>
            </>
          ) : (
            <p className="inspector-note">{role ? "An agentRoles entry: it is edited in mirador.json." : `No role \`${m.role}\`.`}</p>
          ))}

        {tab === "told" && (
          <>
            {(dirty || props.teamDirty) && (
              <p className="inspector-note">The saved version: unsaved edits are not in it yet.</p>
            )}
            {toldError ? (
              // A member added since the team was saved is not in the file.
              /no member/.test(toldError) ? (
                <p className="inspector-note">Save the team to see what this member is told.</p>
              ) : (
                <p className="inspector-note problem">{toldError}</p>
              )
            ) : (
              told?.layers.map((l, i) => {
                const edit =
                  l.kind === "role"
                    ? () => setTab("prompt")
                    : l.kind === "roleWay" && role?.way
                      ? () => props.onOpenWay(role.way!)
                      : l.kind === "teamWay"
                        ? () => {
                            const way = cat.catalog.teams.find((t) => t.name === team)?.way;
                            if (way) props.onOpenWay(way);
                          }
                        : l.kind === "charter"
                          ? props.onEditCharter
                          : null;
                return (
                  <div key={i} className="told-layer">
                    <div className="told-layer-head">
                      <span className="told-layer-number">{i + 1}</span>
                      <span className="told-layer-source" title={l.source}>
                        {l.source}
                      </span>
                      {edit && (
                        <button className="told-layer-edit" onClick={edit}>
                          Edit
                        </button>
                      )}
                    </div>
                    <pre className="told-layer-text">
                      {l.text.replace(/^\n+/, "").split("\n").slice(0, 4).join("\n")}
                    </pre>
                  </div>
                );
              })
            )}
            {told?.prompt && (
              <div className="told-foot">
                <span title="An estimate: about four characters a token">
                  ≈ {Math.round(told.prompt.length / 4)} tokens
                </span>
                {" · "}
                <button
                  onClick={() =>
                    void navigator.clipboard
                      .writeText(told.prompt!)
                      .then(() => setStatus("Copied the full prompt"))
                      .catch(fail)
                  }
                >
                  Copy full prompt
                </button>
              </div>
            )}
          </>
        )}
      </div>
      <div className="inspector-foot">
        {status && <span className="inspector-status">{status}</span>}
        {dirty && <span className="editor-unsaved">Unsaved role</span>}
        {role?.source === "builtin"
          ? (["repo", "global"] as const).map((to) => (
              <button
                key={to}
                disabled={!dirty || (to === "repo" && !cat.repoDir)}
                title={to === "repo" && !cat.repoDir ? "Not in a repository" : `A copy of the role, in ${SOURCE_LABEL[to]}`}
                onClick={() => save(to)}
              >
                Save a copy in {SOURCE_LABEL[to]}
              </button>
            ))
          : editRole && (
              <button className="primary" disabled={!dirty} onClick={() => role && save(role.source)}>
                Save role
              </button>
            )}
        <button onClick={askRestart}>Restart {m.name ?? m.role}</button>
      </div>
    </aside>
  );
}
