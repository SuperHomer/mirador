import { useEffect, useState } from "react";
import { AgentFileKind, UsedBy, WayFields, agentEditorUsedBy } from "../bindings";

type Props = {
  paneId: string;
  name: string;
  form: WayFields;
  onChange: (form: WayFields) => void;
  readOnly: boolean;
  refreshed: number;
  onOpen: (kind: AgentFileKind, name: string) => void;
};

/**
 * A way of working's page (17d): its text in full width, its description,
 * and beside them who uses it — the teams that follow it as theirs, the
 * roles that do — and whether each is running.
 */
export function WayPage({ paneId, name, form, onChange, readOnly, refreshed, onOpen }: Props) {
  const [users, setUsers] = useState<UsedBy[] | null>(null);
  useEffect(() => {
    agentEditorUsedBy(paneId, "way", name)
      .then(setUsers)
      .catch(() => setUsers([]));
  }, [paneId, name, refreshed]);

  return (
    <div className="way-page">
      <div className="way-main">
        <label className="inspector-field">
          <span>Description</span>
          <input
            readOnly={readOnly}
            value={form.description ?? ""}
            onChange={(e) => onChange({ ...form, description: e.target.value || null })}
          />
        </label>
        <textarea
          className="team-charter"
          spellCheck={false}
          readOnly={readOnly}
          value={form.prompt}
          onChange={(e) => onChange({ ...form, prompt: e.target.value })}
        />
      </div>
      <aside className="way-users">
        <div className="team-label">USED BY</div>
        {users?.length === 0 && <p className="inspector-note">No team or role follows it.</p>}
        {users?.map((u) => (
          <button key={`${u.kind}:${u.name}`} className={`way-user${u.running ? " running" : ""}`} onClick={() => onOpen(u.kind, u.name)}>
            <span className="way-user-name">{u.name}</span>
            <span className="way-user-kind">{u.kind === "team" ? "team default" : "role"}</span>
            <span className="way-user-state">{u.running ? "running" : "not running"}</span>
          </button>
        ))}
        <p className="inspector-note">
          Saving changes the file only. Running agents keep the prompt they started with until restarted.
        </p>
      </aside>
    </div>
  );
}
