import { useEffect, useState } from "react";
import { AgentFileKind, EditorCatalog, RoleFields, agentEditorUsedBy } from "../bindings";
import { RoleFieldsEditor, RolePromptEditor } from "./RoleForm";

type Props = {
  paneId: string;
  name: string;
  form: RoleFields;
  onChange: (form: RoleFields) => void;
  cat: EditorCatalog;
  readOnly: boolean;
  refreshed: number;
  onOpen: (kind: AgentFileKind, name: string) => void;
};

/**
 * A role's page: its fields as pickers — the inspector's, the same
 * component — the teams it is in, and its prompt.
 */
export function RolePage({ paneId, name, form, onChange, cat, readOnly, refreshed, onOpen }: Props) {
  const [teams, setTeams] = useState<string[]>([]);
  useEffect(() => {
    agentEditorUsedBy(paneId, "role", name)
      .then((users) => setTeams(users.filter((u) => u.kind === "team").map((u) => u.name)))
      .catch(() => setTeams([]));
  }, [paneId, name, refreshed]);

  return (
    <div className="role-page">
      {/* A built-in is read here; its copy is what gets edited. */}
      <div className="role-fields">
        <fieldset disabled={readOnly}>
          <RoleFieldsEditor form={form} onChange={onChange} cat={cat} description noWay="none" />
        </fieldset>
        <div className="inspector-field">
          <span>Used in</span>
          <div className="inspector-chips">
            {teams.length === 0 && <span className="inspector-note">no team</span>}
            {teams.map((t) => (
              <button key={t} className="inspector-chip link" onClick={() => onOpen("team", t)}>
                {t}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="role-prompt">
        <div className="team-label">PROMPT</div>
        {readOnly ? (
          <textarea className="inspector-prompt" readOnly value={form.prompt ?? ""} />
        ) : (
          <RolePromptEditor form={form} onChange={onChange} />
        )}
      </div>
    </div>
  );
}
