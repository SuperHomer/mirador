import { useState } from "react";
import { AgentRole, EditorCatalog, HARNESS_LABEL, Harness, RoleFields } from "../bindings";

/** A role as its form edits it. */
export const roleFieldsOf = (r: AgentRole): RoleFields => ({
  harness: r.harness,
  model: r.model,
  effort: r.effort,
  way: r.way,
  description: r.description,
  deny: r.tools?.deny ?? [],
  prompt: r.prompt,
});

/** `list` with `value` in it, for a picker that must show the file's own. */
export const withValue = (list: string[], value: string | null | undefined) =>
  value && !list.includes(value) ? [...list, value] : list;

/**
 * A role's fields as pickers, so a value the launch would reject cannot
 * be typed: harness, model and effort from the catalog's choices for the
 * harness (and the file's own value), its way of working, the tools it
 * is denied — and, where asked, its description. The inspector and the
 * role page both use it.
 */
export function RoleFieldsEditor({
  form,
  onChange,
  cat,
  description = false,
  noWay = "team default",
}: {
  form: RoleFields;
  onChange: (form: RoleFields) => void;
  cat: EditorCatalog;
  description?: boolean;
  /** What following no way of its own is called: a team's way, in a team. */
  noWay?: string;
}) {
  const [adding, setAdding] = useState("");
  const choices = cat.choices.find((c) => c.harness === form.harness);
  const ways = cat.catalog.ways.map((w) => w.name);
  return (
    <>
      {description && (
        <label className="inspector-field">
          <span>Description</span>
          <input
            value={form.description ?? ""}
            onChange={(e) => onChange({ ...form, description: e.target.value || null })}
          />
        </label>
      )}
      <label className="inspector-field">
        <span>Harness</span>
        <select
          value={form.harness}
          onChange={(e) => onChange({ ...form, harness: e.target.value as Harness })}
        >
          {withValue(
            cat.choices.map((c) => c.harness),
            form.harness,
          ).map((h) => (
            <option key={h} value={h}>
              {HARNESS_LABEL[h as Harness]}
            </option>
          ))}
        </select>
      </label>
      <label className="inspector-field">
        <span>Model</span>
        <select
          value={form.model ?? ""}
          onChange={(e) => onChange({ ...form, model: e.target.value || null })}
        >
          <option value="">the harness's default</option>
          {withValue(choices?.models ?? [], form.model).map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
      </label>
      <div className="inspector-field">
        <span>Effort</span>
        <div className="inspector-segments">
          {withValue(choices?.efforts ?? [], form.effort).map((x) => (
            <button
              key={x}
              className={form.effort === x ? "selected" : ""}
              title={form.effort === x ? "Again for the default" : undefined}
              onClick={() => onChange({ ...form, effort: form.effort === x ? null : x })}
            >
              {x}
            </button>
          ))}
        </div>
      </div>
      <label className="inspector-field">
        <span>Way of working</span>
        <select value={form.way ?? ""} onChange={(e) => onChange({ ...form, way: e.target.value || null })}>
          <option value="">{noWay}</option>
          {withValue(ways, form.way).map((w) => (
            <option key={w} value={w}>
              {w}
            </option>
          ))}
        </select>
      </label>
      <div className="inspector-field">
        <span>Tools denied</span>
        <div className="inspector-chips">
          {form.deny.map((rule) => (
            <span key={rule} className="inspector-chip">
              {rule}
              <button
                title={`Allow ${rule} again`}
                onClick={() => onChange({ ...form, deny: form.deny.filter((d) => d !== rule) })}
              >
                ×
              </button>
            </span>
          ))}
          <input
            placeholder="Bash(git push *)"
            value={adding}
            onChange={(e) => setAdding(e.target.value)}
            onKeyDown={(e) => {
              const rule = adding.trim();
              if (e.key !== "Enter" || !rule) return;
              if (!form.deny.includes(rule)) onChange({ ...form, deny: [...form.deny, rule] });
              setAdding("");
            }}
          />
        </div>
      </div>
    </>
  );
}

/** A role's prompt, its body, in a monospace editor. */
export function RolePromptEditor({ form, onChange }: { form: RoleFields; onChange: (form: RoleFields) => void }) {
  return (
    <textarea
      className="inspector-prompt"
      spellCheck={false}
      value={form.prompt ?? ""}
      onChange={(e) => onChange({ ...form, prompt: e.target.value || null })}
    />
  );
}
