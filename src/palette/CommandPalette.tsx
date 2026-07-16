import { useEffect, useMemo, useRef, useState } from "react";
import { newTab, splitPane } from "../bindings";
import { actions } from "../keymap/actions";
import { useConfigStore } from "../state/configStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";

interface Entry {
  id: string;
  title: string;
  hint?: string;
  run: () => void;
}

/** Subsequence fuzzy match; higher is better, null = no match. */
function fuzzyScore(query: string, text: string): number | null {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let qi = 0;
  let score = 0;
  let streak = 0;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      streak += 1;
      score += streak + (ti === 0 || t[ti - 1] === " " ? 5 : 0);
      qi++;
    } else {
      streak = 0;
    }
  }
  return qi === q.length ? score : null;
}

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const close = useUiStore((s) => s.closePalette);
  const customCommands = useConfigStore(
    (s) => s.config?.customCommands ?? [],
  );
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const entries = useMemo<Entry[]>(() => {
    const base: Entry[] = actions
      .filter((a) => a.id !== "command_palette")
      .map((a) => ({ id: a.id, title: a.title, run: a.run }));
    const custom: Entry[] = customCommands.map((c) => ({
      id: `custom:${c.name}`,
      title: c.name,
      hint: c.command,
      run: () => {
        if (c.target === "tab") {
          void newTab(c.command);
        } else {
          const { snapshot } = useWorkspaceStore.getState();
          const pane = activeTab(snapshot)?.focusedPane;
          if (pane) void splitPane(pane, "row", c.command);
        }
      },
    }));
    return [...custom, ...base];
  }, [customCommands]);

  const filtered = useMemo(() => {
    return entries
      .map((e) => ({ e, score: fuzzyScore(query, e.title) }))
      .filter((x): x is { e: Entry; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((x) => x.e);
  }, [entries, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelected(0);
      // Focus after the overlay renders.
      requestAnimationFrame(() => inputRef.current?.focus());
    } else {
      // Hand focus back to the focused terminal.
      const textarea = document.querySelector<HTMLTextAreaElement>(
        ".pane.focused textarea",
      );
      textarea?.focus();
    }
  }, [open]);

  useEffect(() => setSelected(0), [query]);

  if (!open) return null;

  const runEntry = (entry: Entry | undefined) => {
    close();
    entry?.run();
  };

  return (
    <div className="palette-overlay" onMouseDown={close}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="palette-input"
          placeholder="Type a command…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              close();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelected((s) => Math.min(s + 1, filtered.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelected((s) => Math.max(s - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              runEntry(filtered[selected]);
            }
            e.stopPropagation();
          }}
        />
        <div className="palette-list">
          {filtered.map((entry, i) => (
            <div
              key={entry.id}
              className={`palette-item${i === selected ? " selected" : ""}`}
              onMouseEnter={() => setSelected(i)}
              onClick={() => runEntry(entry)}
            >
              <span className="palette-title">{entry.title}</span>
              {entry.hint && (
                <span className="palette-hint">{entry.hint}</span>
              )}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="palette-empty">No matching commands</div>
          )}
        </div>
      </div>
    </div>
  );
}
