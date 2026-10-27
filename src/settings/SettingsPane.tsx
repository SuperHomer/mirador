import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { SettingRow, closePane, focusPane, settingsView } from "../bindings";

type Props = {
  paneId: string;
  focused: boolean;
};

/** A value as the file has it: a string bare, anything else as JSON. */
function shown(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

/**
 * mirador.json's settings, by group: what the file sets each key to, or
 * its default, and a mark on a value the key does not take. Read on
 * mount and again whenever the file changes on disk. A file that does
 * not parse shows why, above the list.
 */
export function SettingsPane({ paneId, focused }: Props) {
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
        <span className="graph-repo">mirador.json</span>
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
              .map((r) => {
                const set = r.value !== null && r.value !== undefined;
                return (
                  <div key={r.key} className="settings-row">
                    <div className="settings-text">
                      <span className="settings-key">{r.key}</span>
                      <span className="settings-description">{r.description}</span>
                    </div>
                    <div className="settings-value">
                      {set ? (
                        <span className="settings-set" title={shown(r.value)}>
                          {shown(r.value)}
                        </span>
                      ) : (
                        <span className="settings-default">default: {r.default}</span>
                      )}
                      {!r.valid && (
                        <span className="settings-invalid" title={`${r.key} does not take this value`}>
                          invalid
                        </span>
                      )}
                    </div>
                  </div>
                );
              })}
          </section>
        ))}
      </div>
    </div>
  );
}
