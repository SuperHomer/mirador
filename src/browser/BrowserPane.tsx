import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  browserHistory,
  browserNavigate,
  closePane,
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

    // getBoundingClientRect is in viewport coordinates, but a child webview
    // is positioned against the window frame — which on macOS includes the
    // titlebar. Passing the rect straight through drew every browser page a
    // titlebar-height too high, hiding the chrome row above it. Measure the
    // gap instead of hardcoding it: it is 0 on undecorated windows and in
    // fullscreen, and this keeps Windows and Linux correct for free.
    const chromeOffset = async () => {
      try {
        const win = getCurrentWindow();
        const [outer, scale] = await Promise.all([
          win.outerPosition(),
          win.scaleFactor(),
        ]);
        return Math.max(0, window.screenY - outer.y / scale);
      } catch {
        return 0;
      }
    };

    let offset = 0;
    const report = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width < 2 || rect.height < 2) return;
      void setBrowserBounds(paneId, rect.x, rect.y + offset, rect.width, rect.height);
    };

    // Re-measured alongside the bounds poll: a window moved between screens
    // or toggled fullscreen changes the gap.
    void chromeOffset().then((v) => {
      offset = v;
      report();
    });
    const remeasure = setInterval(() => {
      void chromeOffset().then((v) => {
        if (v !== offset) {
          offset = v;
          report();
        }
      });
    }, 1000);

    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    // Rect changes that don't resize the element (divider drags move
    // siblings, tab switches) — a slow poll catches strays cheaply.
    const poll = setInterval(report, 1000);
    return () => {
      observer.disconnect();
      clearInterval(poll);
      clearInterval(remeasure);
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
        {/* The page is a native child webview, so keystrokes inside it
            never reach the host keymap — mod+W cannot close this pane from
            the page. This button lives in our own chrome, which does get
            real clicks. */}
        <button
          className="browser-close"
          title="Close browser pane"
          onClick={() => void closePane(paneId)}
        >
          ×
        </button>
      </div>
      <div className="browser-placeholder" ref={placeholderRef} />
    </div>
  );
}
