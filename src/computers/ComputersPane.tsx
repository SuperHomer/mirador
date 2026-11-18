import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Computer, closePane, focusPane, listComputers } from "../bindings";

type Props = {
  paneId: string;
  focused: boolean;
};

/**
 * The computers saved in mirador.json: each one's name on the left, the
 * chosen one's host and directory on the right. Read again whenever the
 * file changes on disk, so the pane shows what the file holds; a file that
 * does not parse shows why, not an empty list.
 */
export function ComputersPane({ paneId, focused }: Props) {
  const [computers, setComputers] = useState<Computer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);

  // Two reads in a row can come back in either order: only the latest one
  // is shown.
  const latest = useRef(0);
  const read = useCallback(() => {
    const n = ++latest.current;
    void listComputers()
      .then((list) => {
        if (n !== latest.current) return;
        setComputers(list);
        setError(null);
      })
      .catch((e: unknown) => {
        if (n === latest.current) setError(String(e));
      });
  }, []);

  useEffect(() => {
    read();
    const unlisten = listen("config-file-changed", read);
    return () => void unlisten.then((f) => f());
  }, [read]);

  // The chosen one, or the first while none is: renamed or removed, the
  // choice falls back to it.
  const sel = computers.find((c) => c.name === chosen) ?? computers[0];

  return (
    <div
      className={`pane agent-editor${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Computers</span>
        <span className="graph-repo">mirador.json</span>
        <button className="graph-refresh" title="Read the file again" onClick={read}>
          ⟳
        </button>
        <button className="graph-close" title="Close" onClick={() => void closePane(paneId)}>
          ✕
        </button>
      </div>
      {error && <div className="editor-bar error">{error}</div>}
      <div className="editor-body">
        <nav className="editor-list">
          <section>
            <div className="editor-section">
              <span>Computers</span>
            </div>
            {computers.map((c) => (
              <button
                key={c.name}
                className={`editor-item${c.name === sel?.name ? " selected" : ""}`}
                title={`${c.host} · ${c.dir}`}
                onClick={() => setChosen(c.name)}
              >
                <span className="editor-item-row">
                  <span className="editor-item-name">{c.name}</span>
                </span>
              </button>
            ))}
          </section>
        </nav>
        <div className="editor-main">
          {!sel ? (
            !error && (
              <p className="editor-empty">
                No computers saved yet. Add one with the palette's Add a computer…, or
                with mira computer add &lt;name&gt; --host &lt;host&gt; --dir &lt;dir&gt;.
              </p>
            )
          ) : (
            <div className="editor-head">
              <div className="editor-head-text">
                <span className="editor-title">{sel.name}</span>
                <span className="editor-path" title={sel.host}>
                  host: {sel.host}
                </span>
                <span className="editor-path" title={sel.dir}>
                  directory: {sel.dir}
                </span>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
