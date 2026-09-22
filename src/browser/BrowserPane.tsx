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

    // Two coordinate hops separate this placeholder from the child webview
    // that draws over it. Rust handles the first — where the host webview
    // sits inside the window. This handles the second: the window's content
    // view spans the whole frame, but WebKit lays the page out below the
    // titlebar, so the page's viewport origin sits a titlebar lower than
    // the view's. Pass the rect with that gap added back.
    //
    // The gap is the window's inner height minus the page's own: two real
    // measurements, 0 on an undecorated window, in fullscreen, and on
    // platforms that inset nothing. Deriving it from window.screenY instead
    // was the bug this replaces — a wry webview reports no window rect, so
    // screenY reads back as the screen height and drew every page hundreds
    // of pixels below its pane.
    const viewportInset = async () => {
      try {
        const win = getCurrentWindow();
        const [inner, scale] = await Promise.all([
          win.innerSize(),
          win.scaleFactor(),
        ]);
        return Math.max(0, inner.height / scale - window.innerHeight);
      } catch {
        return 0;
      }
    };

    let inset = 0;
    const report = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width < 2 || rect.height < 2) return;
      void setBrowserBounds(paneId, rect.x, rect.y + inset, rect.width, rect.height);
    };

    // Re-measured on the bounds poll: toggling fullscreen or decorations
    // changes the gap.
    void viewportInset().then((v) => {
      inset = v;
      report();
    });
    const remeasure = setInterval(() => {
      void viewportInset().then((v) => {
        if (v !== inset) {
          inset = v;
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
