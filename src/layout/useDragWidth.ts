import { useEffect, useRef, useState } from "react";
import { setConfigValue } from "../bindings";

/** A width in pixels a drag may set, and mirador.json may hold. */
export interface WidthRange {
  min: number;
  max: number;
}

/** What `sidebarWidth` takes: `cmux_core::config::SIDEBAR_WIDTH`. The
 *  write is checked there too, so a drift between the two is a refused
 *  write, never a file the app cannot read. */
export const SIDEBAR_WIDTH: WidthRange = { min: 160, max: 600 };

/** What `focusRailWidth` takes: `cmux_core::config::FOCUS_RAIL_WIDTH`. */
export const FOCUS_RAIL_WIDTH: WidthRange = { min: 150, max: 1600 };

/**
 * A width a person sets by dragging an edge, kept in mirador.json under
 * `key`. While a drag is in flight the width is the pointer's; on release
 * it is written, and shown until the config carrying it comes back (a
 * refused write puts the config's back). A double-click removes the key,
 * which is the default again. `grows` is the way the pointer moves to make
 * it wider: right for the sidebar, left for a column on the right.
 *
 * `width` is null when neither a drag nor the config sets one: the
 * caller's default layout applies.
 */
export function useDragWidth(key: string, configured: number | null, range: WidthRange, grows: "right" | "left") {
  const [dragged, setDragged] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const live = useRef<number | null>(null);

  // The config came back from the write, or changed by another hand (the
  // Settings pane, the file): from here it is the width.
  useEffect(() => setDragged(null), [configured]);

  /** Starts a drag from an edge whose box is `from` pixels wide now, and
   *  which the layout around it lets grow to `room` at most. */
  const onPointerDown = (down: React.PointerEvent, from: number, room: number) => {
    if (down.button !== 0) return;
    down.preventDefault();
    const max = Math.max(range.min, Math.min(range.max, room));
    const startX = down.clientX;
    // The edge keeps the pointer for the drag, as the board's drags do
    // (boardDrag): the system can still take it away mid-drag (Alt+Tab,
    // Cmd+Tab, Mission Control), and then pointercancel or
    // lostpointercapture ends the drag here instead of the width
    // following the mouse until the next click, which used to write it.
    const edge = down.currentTarget as HTMLElement;
    const pointer = down.pointerId;
    try {
      edge.setPointerCapture(pointer);
    } catch {
      // The pointer is already gone: its pointerup or cancel ends it.
    }
    live.current = null;
    setDragging(true);

    const onMove = (e: PointerEvent) => {
      if (e.pointerId !== pointer) return;
      // The button came up where no pointerup reached us: over.
      if ((e.buttons & 1) === 0) return finish(false);
      const delta = grows === "right" ? e.clientX - startX : startX - e.clientX;
      const width = Math.round(Math.max(range.min, Math.min(max, from + delta)));
      live.current = width;
      setDragged(width);
    };
    /** `dropped`: released where the drag was going. Otherwise it was
     *  interrupted, and the width goes back to what it was. */
    const finish = (dropped: boolean) => {
      edge.removeEventListener("pointermove", onMove);
      edge.removeEventListener("pointerup", onUp);
      edge.removeEventListener("pointercancel", onCancel);
      edge.removeEventListener("lostpointercapture", onCancel);
      if (edge.hasPointerCapture(pointer)) edge.releasePointerCapture(pointer);
      setDragging(false);
      const width = live.current;
      live.current = null;
      if (!dropped) {
        setDragged(null);
        return;
      }
      // A press with no move is a click (or half a double-click): nothing
      // to write.
      if (width === null || width === configured) return;
      setConfigValue(key, width).catch((err) => {
        console.warn(`mirador: ${key} was not saved: ${err}`);
        setDragged(null);
      });
    };
    const onUp = (e: PointerEvent) => {
      if (e.pointerId === pointer) finish(true);
    };
    const onCancel = (e: PointerEvent) => {
      if (e.pointerId === pointer) finish(false);
    };
    edge.addEventListener("pointermove", onMove);
    edge.addEventListener("pointerup", onUp);
    edge.addEventListener("pointercancel", onCancel);
    edge.addEventListener("lostpointercapture", onCancel);
  };

  const onDoubleClick = () => {
    setDragged(null);
    if (configured === null) return;
    setConfigValue(key, null).catch((err) => console.warn(`mirador: ${key} was not reset: ${err}`));
  };

  return { width: dragged ?? configured, dragging, onPointerDown, onDoubleClick };
}
