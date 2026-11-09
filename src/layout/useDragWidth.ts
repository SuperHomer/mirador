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
    live.current = null;
    setDragging(true);

    const onMove = (e: PointerEvent) => {
      const delta = grows === "right" ? e.clientX - startX : startX - e.clientX;
      const width = Math.round(Math.max(range.min, Math.min(max, from + delta)));
      live.current = width;
      setDragged(width);
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setDragging(false);
      // A press with no move is a click (or half a double-click): nothing
      // to write.
      const width = live.current;
      if (width === null || width === configured) return;
      setConfigValue(key, width).catch((err) => {
        console.warn(`mirador: ${key} was not saved: ${err}`);
        setDragged(null);
      });
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const onDoubleClick = () => {
    setDragged(null);
    if (configured === null) return;
    setConfigValue(key, null).catch((err) => console.warn(`mirador: ${key} was not reset: ${err}`));
  };

  return { width: dragged ?? configured, dragging, onPointerDown, onDoubleClick };
}
