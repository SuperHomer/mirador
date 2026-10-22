import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  browserHistory,
  browserNavigate,
  browserScreenshot,
  closePane,
  focusPane,
  setBrowserBounds,
  setBrowserVisible,
} from "../bindings";
import { atFullSize, useBoardStore } from "../state/boardStore";
import { useUiStore } from "../state/uiStore";

interface Props {
  paneId: string;
  url: string;
  focused: boolean;
  /**
   * Drawn as a card on the board. A native webview can be neither scaled
   * nor clipped with its card, nor drawn under another, so there the page
   * is shown only when nothing of that is asked of it; the pane decides
   * when, and shows what the page is meanwhile. In the tabs view the app
   * shows and hides pages (see `App`).
   */
  board: boolean;
}

/**
 * Host-side shell of a browser pane: the chrome (URL bar, nav buttons) is
 * ours; the page itself is a native child webview that Rust positions over
 * the placeholder div, tracked via ResizeObserver.
 */
export function BrowserPane({ paneId, url, focused, board }: Props) {
  const placeholderRef = useRef<HTMLDivElement>(null);
  // The page is on screen over the placeholder.
  const [live, setLive] = useState(!board);
  const boardRef = useRef(board);
  boardRef.current = board;
  const reportRef = useRef<() => void>(() => {});
  const updateRef = useRef<() => void>(() => {});
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
    //
    // Note what a height difference can and cannot say: it gives the total
    // vertical inset, not where that inset sits. Adding it to rect.y spends
    // all of it at the top, which is right wherever the inset *is* a
    // titlebar. A platform that inset the same amount at the bottom would
    // read identically here and push every page down by it — so if pages
    // ever sit low on some platform, this line is the one to doubt.
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
      // The page's size is its card's, whatever the board's zoom: a page
      // read by `mira browser` while hidden lays out as it will be shown.
      const zoom = boardRef.current ? (useBoardStore.getState().camera?.zoom ?? 1) : 1;
      void setBrowserBounds(paneId, rect.x, rect.y + inset, rect.width / zoom, rect.height / zoom);
    };
    reportRef.current = report;

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

  // On the board: shown only at 100%, with the board still, the card wholly
  // in view, and no overlay open. (No card ever covers another: the model
  // keeps them apart.) Bounds go first: a pan or zoom moves the card
  // without resizing it, which nothing else reports.
  useEffect(() => {
    const el = placeholderRef.current;
    if (!board || !el) {
      setLive(true);
      return;
    }
    let last: boolean | null = null;
    let lastAt = "";
    const update = () => {
      const { camera, moving } = useBoardStore.getState();
      const ui = useUiStore.getState();
      const r = el.getBoundingClientRect();
      const host = el.closest(".tabs-host")?.getBoundingClientRect();
      const inView =
        !!host &&
        r.width > 1 &&
        r.left >= host.left &&
        r.top >= host.top &&
        r.right <= host.right &&
        r.bottom <= host.bottom;
      const show =
        atFullSize(camera) &&
        !moving &&
        inView &&
        !ui.paletteOpen &&
        !ui.notificationsOpen &&
        !ui.activityPane &&
        !ui.boardMenuOpen;
      // A shown card that moved — its frame dragged, a snapshot placing it
      // anew — takes its page along now, not at the next slow poll.
      const at = `${r.x},${r.y},${r.width},${r.height}`;
      const moved = at !== lastAt;
      lastAt = at;
      if (show === last) {
        if (show && moved) reportRef.current();
        return;
      }
      last = show;
      if (show) reportRef.current();
      void setBrowserVisible(paneId, show);
      setLive(show);
    };
    updateRef.current = update;
    update();
    const unsubscribe = [useBoardStore.subscribe(update), useUiStore.subscribe(update)];
    return () => {
      updateRef.current = () => {};
      unsubscribe.forEach((u) => u());
    };
  }, [board, paneId]);
  // A snapshot can move the card (or its frame) without a pan: look again
  // after every render. It only measures, and acts on a change.
  useEffect(() => updateRef.current());

  // While its page is hidden, a card shows a picture of it, taken while
  // the card is at least partly on screen: when the page goes (what the
  // person just saw), when it navigates, when the card comes into view
  // with no picture yet, and every few seconds after, so a page that
  // changes itself stays fresh. One capture at a time per pane.
  const [still, setStill] = useState<string | null>(null);
  const stillRef = useRef<string | null>(null);
  const capturing = useRef(false);
  // Each run of the effect below is a generation, ended by the next run,
  // the page going live, or unmount. A capture answers for the page as it
  // was when it started: from an ended generation it would put an older
  // page, or one now shown live, over the card, so it is dropped.
  const generation = useRef(0);
  // The current generation's capture, for one that ends stale to hand on to.
  const retry = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      if (stillRef.current) URL.revokeObjectURL(stillRef.current);
      stillRef.current = null;
    },
    [],
  );
  useEffect(() => {
    if (!board || live) return;
    const gen = generation.current;
    const onScreen = () => {
      const r = placeholderRef.current?.getBoundingClientRect();
      const host = placeholderRef.current?.closest(".tabs-host")?.getBoundingClientRect();
      return (
        document.visibilityState === "visible" &&
        !!r &&
        !!host &&
        r.right > host.left &&
        r.left < host.right &&
        r.bottom > host.top &&
        r.top < host.bottom
      );
    };
    const capture = () => {
      if (capturing.current || !onScreen()) return;
      capturing.current = true;
      browserScreenshot(paneId)
        .then((png) => {
          if (gen !== generation.current) return;
          const next = URL.createObjectURL(new Blob([png], { type: "image/png" }));
          if (stillRef.current) URL.revokeObjectURL(stillRef.current);
          stillRef.current = next;
          setStill(next);
        })
        // A failed capture keeps the picture before it: on Windows a hidden
        // page may not capture at all.
        .catch(() => {})
        .finally(() => {
          capturing.current = false;
          // This one held off the capture of the generation that ended it.
          if (gen !== generation.current) retry.current?.();
        });
    };
    retry.current = capture;
    capture();
    const refresh = setInterval(capture, 5000);
    // Coming into view with no picture yet: at once, not at the next tick.
    const unsubscribe = useBoardStore.subscribe(() => {
      if (!stillRef.current) capture();
    });
    return () => {
      generation.current += 1;
      retry.current = null;
      clearInterval(refresh);
      unsubscribe();
    };
  }, [board, live, url, paneId]);

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
      <div className="browser-placeholder" ref={placeholderRef}>
        {!live &&
          (still ? (
            <img className="browser-shot" src={still} alt="" draggable={false} />
          ) : (
            <div className="browser-still">{url}</div>
          ))}
      </div>
    </div>
  );
}
