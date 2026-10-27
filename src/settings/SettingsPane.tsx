import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { SettingRow, closePane, focusPane, setConfigValue, settingsView } from "../bindings";

type Props = {
  paneId: string;
  /** The mirador.json it shows. */
  path: string;
  focused: boolean;
};

/** A value as the file has it: a string bare, anything else as JSON. */
function shown(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

/**
 * mirador.json's settings, by group: what the file sets each key to, or
 * its default, and a mark on a value the key does not take. Each is
 * edited with the control its kind calls for. A write never patches the
 * rows: they are read again when the file changes on disk, so the file
 * stays what the pane shows. A file that does not parse shows why,
 * above the list.
 */
export function SettingsPane({ paneId, path, focused }: Props) {
  const [rows, setRows] = useState<SettingRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  const read = useCallback(() => {
    void settingsView()
      .then((r) => {
        setRows(r);
        setError(null);
      })
      .catch((e: unknown) => setError(String(e)));
  }, []);

  useEffect(() => {
    read();
    const unlisten = listen("config-file-changed", read);
    return () => void unlisten.then((f) => f());
  }, [read]);

  const groups = [...new Set(rows.map((r) => r.group))];

  return (
    <div
      className={`pane settings-pane${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Settings</span>
        <span className="graph-repo" title={path}>
          mirador.json
        </span>
        <button className="graph-refresh" title="Read the file again" onClick={read}>
          ⟳
        </button>
        <button className="graph-close" title="Close" onClick={() => void closePane(paneId)}>
          ✕
        </button>
      </div>
      {error && <div className="editor-bar error">{error}</div>}
      <div className="settings-body">
        {groups.map((group) => (
          <section key={group}>
            <div className="editor-section">
              <span>{group}</span>
            </div>
            {rows
              .filter((r) => r.group === group)
              .map((r) => (
                <Setting key={r.key} row={r} path={path} />
              ))}
          </section>
        ))}
      </div>
    </div>
  );
}

/** One setting: its key and description, its control, and why the last
 *  write of it failed. */
function Setting({ row, path }: { row: SettingRow; path: string }) {
  const [failed, setFailed] = useState<string | null>(null);
  const set = row.set;
  const write = (value: unknown) =>
    void setConfigValue(row.key, value)
      .then(() => setFailed(null))
      .catch((e: unknown) => setFailed(String(e)));

  return (
    <div className="settings-row">
      <div className="settings-text">
        <span className="settings-key">{row.key}</span>
        <span className="settings-description">{row.description}</span>
        {row.kind.kind === "readOnly" && (
          <span className="settings-where">Edit in mirador.json: {path}</span>
        )}
        {failed && <span className="settings-error">{failed}</span>}
      </div>
      <div className="settings-value">
        {!row.valid && (
          <span className="settings-invalid" title={`${row.key} does not take this value`}>
            invalid
          </span>
        )}
        <Control row={row} set={set} write={write} />
        {set && row.kind.kind !== "readOnly" && (
          <button className="settings-reset" title="Remove it from mirador.json" onClick={() => write(null)}>
            Reset
          </button>
        )}
      </div>
    </div>
  );
}

/** The control a setting's kind calls for. */
function Control({ row, set, write }: { row: SettingRow; set: boolean; write: (value: unknown) => void }) {
  const kind = row.kind;
  switch (kind.kind) {
    case "choice": {
      // Unset, the default is what shows chosen; the key stays unset
      // until another is picked. A value the file has that is not a
      // choice shows as itself.
      const at = set
        ? kind.values.findIndex((v) => v === row.value)
        : kind.values.findIndex((v) => String(v) === row.default);
      return (
        <select
          className={`settings-select${set ? "" : " unset"}`}
          value={at}
          onChange={(e) => write(kind.values[Number(e.target.value)])}
        >
          {at === -1 && (
            <option value={-1} disabled>
              {set ? shown(row.value) : row.default}
            </option>
          )}
          {kind.values.map((v, i) => (
            <option key={i} value={i}>
              {String(v)}
            </option>
          ))}
        </select>
      );
    }
    case "bool":
      return (
        <input
          type="checkbox"
          className={`settings-toggle${set ? "" : " unset"}`}
          checked={set ? row.value === true : row.default === "true"}
          onChange={(e) => write(e.target.checked)}
        />
      );
    case "number":
    case "text":
      return <Field row={row} set={set} write={write} />;
    case "readOnly":
      return set ? (
        <span className="settings-set" title={shown(row.value)}>
          {shown(row.value)}
        </span>
      ) : (
        <span className="settings-default">default: {row.default}</span>
      );
  }
}

/** A number or text input, written on Enter or blur rather than on each
 *  keystroke; left empty, it removes the key. Escape puts back what the
 *  file has. After a failed write the typed text stays. */
function Field({ row, set, write }: { row: SettingRow; set: boolean; write: (value: unknown) => void }) {
  const number = row.kind.kind === "number";
  const initial = set ? shown(row.value) : "";
  const [draft, setDraft] = useState(initial);
  useEffect(() => setDraft(initial), [initial]);

  const commit = () => {
    if (draft === initial) return;
    if (draft.trim() === "") return write(null);
    write(number ? Number(draft) : draft);
  };

  return (
    <input
      className="settings-input"
      type={number ? "number" : "text"}
      step={number ? "any" : undefined}
      min={row.kind.kind === "number" ? row.kind.min : undefined}
      max={row.kind.kind === "number" ? row.kind.max : undefined}
      value={draft}
      placeholder={row.default}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        else if (e.key === "Escape") setDraft(initial);
      }}
    />
  );
}
