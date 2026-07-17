import { useEffect, useRef, useState } from "react";
import {
  browserHistory,
  browserNavigate,
  focusPane,
  setBrowserBounds,
} from "../bindings";

interface Props {
  paneId: string;
  url: string;
  focused: boolean;
}

/**
 * Host-side shell of a browser pane: the chrome (URL bar, nav buttons) is
 * ours; the page itself is a native child webview that Rust positions over
 * the placeholder div, tracked via ResizeObserver.
 */
export function BrowserPane({ paneId, url, focused }: Props) {
  const placeholderRef = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState(url);
  const [editing, setEditing] = useState(false);

  // The authoritative URL follows navigation events unless the user is
  // mid-edit in the URL bar.
  useEffect(() => {
    if (!editing) setDraft(url);
  }, [url, editing]);

  useEffect(() => {
    const el = placeholderRef.current;
    if (!el) return;

    const report = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width < 2 || rect.height < 2) return;
      void setBrowserBounds(paneId, rect.x, rect.y, rect.width, rect.height);
    };

    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    // Rect changes that don't resize the element (divider drags move
    // siblings, tab switches) — a slow poll catches strays cheaply.
    const poll = setInterval(report, 1000);
    return () => {
      observer.disconnect();
      clearInterval(poll);
    };
  }, [paneId]);

  const commit = () => {
    setEditing(false);
    if (draft.trim() && draft !== url) {
      void browserNavigate(paneId, draft.trim());
    }
  };

  return (
    <div
      className={`pane browser-pane${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="browser-chrome">
        <button onClick={() => void browserHistory(paneId, "back")}>‹</button>
        <button onClick={() => void browserHistory(paneId, "forward")}>
          ›
        </button>
        <button onClick={() => void browserHistory(paneId, "reload")}>
          ⟳
        </button>
        <input
          className="browser-url"
          value={draft}
          spellCheck={false}
          onFocus={() => setEditing(true)}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              commit();
              (e.target as HTMLInputElement).blur();
            }
            if (e.key === "Escape") {
              setDraft(url);
              setEditing(false);
            }
            e.stopPropagation();
          }}
        />
      </div>
      <div className="browser-placeholder" ref={placeholderRef} />
    </div>
  );
}
