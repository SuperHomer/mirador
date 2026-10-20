import { ReactNode, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Camera, TabSnapshot, closeTab, newTab, setFramePosition, setView } from "../bindings";
import { FrameMenu } from "./FrameMenu";
import { useWorkspaceStore } from "../state/workspaceStore";
import { atFullSize, fitBoard, panToCard, useBoardStore } from "../state/boardStore";

const MIN_ZOOM = 0.1;
const MAX_ZOOM = 4;
/** Where the − / + buttons stop, MIN_ZOOM to MAX_ZOOM. */
const ZOOM_STOPS = [0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 1, 1.25, 1.5, 2, 3, 4];
/** Zooming this close to 100% lands on it: 100% is where cards take the
 *  pointer, and a pinch rarely stops exactly there. */
const SNAP = 0.04;
/** Wheel line steps, in pixels (Firefox-style `deltaMode` 1). */
const LINE_PX = 16;

function clampZoom(zoom: number): number {
  const z = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
  return Math.abs(z - 1) < SNAP ? 1 : z;
}

/** `camera` zoomed to `zoom`, keeping the board point under (px, py) there. */
function zoomAround(camera: Camera, zoom: number, px: number, py: number): Camera {
  const bx = camera.x + px / camera.zoom;
  const by = camera.y + py / camera.zoom;
  return { x: bx - px / zoom, y: by - py / zoom, zoom };
}

/** The next stop strictly above (`dir` 1) or below (-1) `zoom`, if any;
 *  "strictly" with some slack, so 99.9% after a flight counts as 100%. */
function nextZoomStop(zoom: number, dir: 1 | -1): number | undefined {
  const stops = dir > 0 ? ZOOM_STOPS : [...ZOOM_STOPS].reverse();
  return stops.find((z) => (z - zoom) * dir > 1e-3);
}

/** Flies to the next zoom stop, keeping the middle of the board in place. */
function zoomStep(dir: 1 | -1) {
  const { camera, viewport, flyTo } = useBoardStore.getState();
  const zoom = camera && nextZoomStop(camera.zoom, dir);
  if (!camera || !zoom) return;
  flyTo(zoomAround(camera, zoom, viewport.width / 2, viewport.height / 2));
}

/** Pans to a pane's card unless it is wholly in view already. */
function bringIntoView(paneId: string) {
  const snap = useWorkspaceStore.getState().snapshot;
  const tab = snap?.tabs.find((t) => t.cards.some((c) => c.paneId === paneId));
  const card = tab?.cards.find((c) => c.paneId === paneId);
  const { camera: cam, viewport } = useBoardStore.getState();
  if (!tab || !card || !cam || viewport.width === 0) return;
  const left = (tab.frame.x + card.x - cam.x) * cam.zoom;
  const top = (tab.frame.y + card.y - cam.y) * cam.zoom;
  const inView =
    left >= 0 &&
    top >= 0 &&
    left + card.w * cam.zoom <= viewport.width &&
    top + card.h * cam.zoom <= viewport.height;
  if (!inView) panToCard(paneId);
}

/**
 * The host every tab's layer lives in. In the tabs view it is inert: the
 * canvas fills it untransformed and the active tab's layer covers it. On
 * the board, the canvas is the board itself — one transform for the
 * camera — and each tab's layer is its frame on it. The layers are the
 * same elements in both views, so switching remounts no pane.
 *
 * Safari-style `gesture*` events are what WKWebView sends for a trackpad
 * pinch; Chromium-based webviews send ctrl+wheel instead. Both zoom.
 */
export function BoardHost({ board, children }: { board: boolean; children: ReactNode }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const camera = useBoardStore((s) => s.camera);
  const focused = useWorkspaceStore((s) => {
    const snap = s.snapshot;
    return snap?.tabs.find((t) => t.id === snap.activeTab)?.focusedPane ?? null;
  });
  const lastPointerDown = useRef(0);

  // A pane focused from elsewhere — a card just made, a new frame, a
  // shortcut, `mira focus` — is brought into view when it is not wholly
  // in it. A click on a card focuses what is under the pointer already,
  // and must not move the board under it.
  useEffect(() => {
    if (!board || !focused) return;
    if (performance.now() - lastPointerDown.current < 500) return;
    bringIntoView(focused);
  }, [board, focused]);

  // `mira focus` on the pane that has the focus already changes nothing
  // in the snapshot, so the request itself says to look at the card.
  useEffect(() => {
    if (!board) return;
    const unlisten = listen<string>("focus-requested", (e) => bringIntoView(e.payload));
    return () => void unlisten.then((fn) => fn());
  }, [board]);

  // The saved camera, once; from then on the board owns it.
  useEffect(() => {
    const saved = useWorkspaceStore.getState().snapshot?.camera;
    if (saved && !useBoardStore.getState().camera) useBoardStore.getState().moveTo(saved);
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const observer = new ResizeObserver(() =>
      useBoardStore.getState().setViewport(host.clientWidth, host.clientHeight),
    );
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !board) return;
    const store = useBoardStore.getState;
    const local = (e: { clientX: number; clientY: number }) => {
      const r = host.getBoundingClientRect();
      return [e.clientX - r.left, e.clientY - r.top] as const;
    };

    let pinching: { zoom: number } | null = null;

    const onWheel = (e: WheelEvent) => {
      const cam = store().camera;
      if (!cam) return;
      if (e.ctrlKey) {
        // A pinch reported both ways must not zoom twice.
        e.preventDefault();
        e.stopPropagation();
        if (pinching) return;
        const [px, py] = local(e);
        const step = e.deltaMode === 1 ? e.deltaY * LINE_PX : e.deltaY;
        store().moveTo(zoomAround(cam, clampZoom(cam.zoom * Math.exp(-step * 0.01)), px, py));
        return;
      }
      // At 100% a card's content scrolls itself (a terminal's scrollback).
      const inCard = (e.target as Element).closest?.(".pane-body");
      if (inCard && atFullSize(cam)) return;
      e.preventDefault();
      const scale = e.deltaMode === 1 ? LINE_PX : 1;
      store().moveTo({
        ...cam,
        x: cam.x + (e.deltaX * scale) / cam.zoom,
        y: cam.y + (e.deltaY * scale) / cam.zoom,
      });
    };

    type GestureEvent = UIEvent & { scale: number; clientX: number; clientY: number };
    const onGestureStart = (e: Event) => {
      e.preventDefault();
      pinching = { zoom: store().camera?.zoom ?? 1 };
      store().hold(true);
    };
    const onGestureChange = (e: Event) => {
      e.preventDefault();
      const cam = store().camera;
      if (!cam || !pinching) return;
      const g = e as GestureEvent;
      const [px, py] = local(g);
      store().moveTo(zoomAround(cam, clampZoom(pinching.zoom * g.scale), px, py));
    };
    const onGestureEnd = (e: Event) => {
      e.preventDefault();
      pinching = null;
      store().hold(false);
    };

    // Capture: ctrl+wheel over a terminal must zoom the board, not reach
    // xterm. Not passive, to stop the page scrolling or zooming itself.
    host.addEventListener("wheel", onWheel, { capture: true, passive: false });
    host.addEventListener("gesturestart", onGestureStart, { capture: true });
    host.addEventListener("gesturechange", onGestureChange, { capture: true });
    host.addEventListener("gestureend", onGestureEnd, { capture: true });
    return () => {
      host.removeEventListener("wheel", onWheel, { capture: true });
      host.removeEventListener("gesturestart", onGestureStart, { capture: true });
      host.removeEventListener("gesturechange", onGestureChange, { capture: true });
      host.removeEventListener("gestureend", onGestureEnd, { capture: true });
    };
  }, [board]);

  // Dragging empty board pans: the board itself, or a frame's background.
  // Only those — anything drawn on the board (a card, a title bar, a
  // button, the corner controls) has its own use for a press, and a pan
  // captures the pointer, which would take the click away from a button.
  const onPointerDown = (e: React.PointerEvent) => {
    if (!board || e.button !== 0) return;
    const target = e.target as Element;
    const empty =
      target === e.currentTarget ||
      target.matches(".board-canvas, .tab-layer, .split-layer");
    if (!empty) return;
    const start = useBoardStore.getState().camera;
    if (!start) return;
    e.preventDefault();
    const host = e.currentTarget as HTMLElement;
    host.setPointerCapture(e.pointerId);
    host.classList.add("panning");
    useBoardStore.getState().hold(true);
    const [x0, y0] = [e.clientX, e.clientY];
    const onMove = (m: PointerEvent) =>
      useBoardStore.getState().moveTo({
        ...start,
        x: start.x - (m.clientX - x0) / start.zoom,
        y: start.y - (m.clientY - y0) / start.zoom,
      });
    const onUp = () => {
      host.classList.remove("panning");
      useBoardStore.getState().hold(false);
      host.removeEventListener("pointermove", onMove);
      host.removeEventListener("pointerup", onUp);
      host.removeEventListener("pointercancel", onUp);
    };
    host.addEventListener("pointermove", onMove);
    host.addEventListener("pointerup", onUp);
    host.addEventListener("pointercancel", onUp);
  };

  const cam = camera ?? { x: 0, y: 0, zoom: 1 };
  const grid = 24 * cam.zoom;
  return (
    <div
      ref={hostRef}
      className={`tabs-host${board ? " board" : ""}${board && !atFullSize(camera) ? " overview" : ""}`}
      style={
        board
          ? {
              backgroundSize: `${grid}px ${grid}px`,
              backgroundPosition: `${-cam.x * cam.zoom}px ${-cam.y * cam.zoom}px`,
            }
          : undefined
      }
      onPointerDown={onPointerDown}
      onPointerDownCapture={(e) => {
        // A click on a card focuses that card, there under the pointer.
        if ((e.target as Element).closest(".pane-slot")) {
          lastPointerDown.current = performance.now();
        }
      }}
    >
      <div
        className="board-canvas"
        style={
          board
            ? {
                transform: `translate(${-cam.x * cam.zoom}px, ${-cam.y * cam.zoom}px) scale(${cam.zoom})`,
              }
            : undefined
        }
      >
        {children}
      </div>
      {board && (
        <div className="board-controls">
          <button
            className="board-zoom-step"
            title="Zoom out"
            disabled={!nextZoomStop(cam.zoom, -1)}
            onClick={() => zoomStep(-1)}
          >
            −
          </button>
          <span className="board-zoom">{Math.round(cam.zoom * 100)}%</span>
          <button
            className="board-zoom-step"
            title="Zoom in"
            disabled={!nextZoomStop(cam.zoom, 1)}
            onClick={() => zoomStep(1)}
          >
            +
          </button>
          {/* The way back, for when the sidebar's switch is hidden (⌘B). */}
          <button title="Back to tabs (⌘⇧O / Ctrl+Shift+O)" onClick={() => void setView("tabs")}>
            Tabs
          </button>
          <button
            title="A new tab, its frame where you are looking"
            onClick={() => void newTab()}
          >
            New frame
          </button>
          <button title="Show the whole board" onClick={fitBoard}>
            Fit
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * Follows a drag on the board that began with `down`: `move` gets the
 * pointer's travel in board units (the zoom divided out), `end` whether it
 * was dropped after moving (true) or cancelled — Escape, or never moved
 * (false). The board counts as moving meanwhile, so browser pages hide.
 */
export function boardDrag(
  down: React.PointerEvent,
  move: (dx: number, dy: number) => void,
  end: (dropped: boolean) => void,
) {
  if (down.button !== 0) return;
  down.preventDefault();
  down.stopPropagation();
  const zoom = useBoardStore.getState().camera?.zoom ?? 1;
  const el = down.currentTarget as HTMLElement;
  const pointer = down.pointerId;
  el.setPointerCapture(pointer);
  const [x0, y0] = [down.clientX, down.clientY];
  let moved = false;
  useBoardStore.getState().hold(true);

  const onMove = (e: PointerEvent) => {
    moved = true;
    move((e.clientX - x0) / zoom, (e.clientY - y0) / zoom);
  };
  const finish = (dropped: boolean) => {
    el.removeEventListener("pointermove", onMove);
    el.removeEventListener("pointerup", onUp);
    el.removeEventListener("pointercancel", onCancel);
    window.removeEventListener("keydown", onKey, { capture: true });
    if (el.hasPointerCapture(pointer)) el.releasePointerCapture(pointer);
    useBoardStore.getState().hold(false);
    end(dropped && moved);
  };
  const onUp = () => finish(true);
  const onCancel = () => finish(false);
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    // Not the keymap's, nor the terminal's: it cancels the drag.
    e.preventDefault();
    e.stopPropagation();
    finish(false);
  };
  el.addEventListener("pointermove", onMove);
  el.addEventListener("pointerup", onUp);
  el.addEventListener("pointercancel", onCancel);
  window.addEventListener("keydown", onKey, { capture: true });
}

/**
 * One tab's layer: in the tabs view, the whole host, shown when it is the
 * active tab; on the board, its frame, moved by dragging its label.
 */
export function TabLayer({
  tab,
  board,
  current,
  children,
}: {
  tab: TabSnapshot;
  board: boolean;
  current: boolean;
  children: ReactNode;
}) {
  // Where the frame is while it is dragged, and until the snapshot says
  // where Rust put it: there, or back where it was if another frame is.
  const [held, setHeld] = useState<{ x: number; y: number } | null>(null);
  const at = held ?? tab.frame;
  // Where the "+" that opened the menu is on screen, while it is open.
  const [menuAt, setMenuAt] = useState<DOMRect | null>(null);

  const drag = (e: React.PointerEvent) => {
    let to = { x: tab.frame.x, y: tab.frame.y };
    boardDrag(
      e,
      (dx, dy) => {
        to = { x: tab.frame.x + dx, y: tab.frame.y + dy };
        setHeld(to);
      },
      (dropped) => {
        if (!dropped) return setHeld(null);
        void setFramePosition(tab.id, to.x, to.y).finally(() =>
          setTimeout(() => setHeld(null), 150),
        );
      },
    );
  };

  return (
    <div
      className={`tab-layer${current ? " current" : ""}${held ? " dragging" : ""}`}
      style={
        board
          ? { left: at.x, top: at.y, width: tab.frame.w, height: tab.frame.h }
          : { visibility: current ? "visible" : "hidden" }
      }
    >
      {board && (
        <div className="frame-label" onPointerDown={drag}>
          <span className="frame-title">{tab.title}</span>
          <button
            className="frame-button"
            data-frame-menu
            title="Add a pane to this tab"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              const at = e.currentTarget.getBoundingClientRect();
              setMenuAt((open) => (open ? null : at));
            }}
          >
            +
          </button>
          <button
            className="frame-button"
            title="Close this tab"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => void closeTab(tab.id)}
          >
            ×
          </button>
        </div>
      )}
      {board && menuAt && (
        <FrameMenu
          pane={tab.focusedPane}
          at={menuAt}
          onClose={() => setMenuAt(null)}
        />
      )}
      {children}
    </div>
  );
}
