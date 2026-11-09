import { useEffect } from "react";
import { zoomTerminals, zoomWindow } from "../bindings";

/** The wheel distance that makes one zoom step: a mouse's notch. A
 *  touchpad's pinch, which arrives as Ctrl + wheel in small deltas, adds
 *  up to a step the same way. */
const STEP_PX = 100;

/** How long a burst of wheel steps is gathered before it is sent as one
 *  zoom, so a flick writes mirador.json once and not once a notch. */
const GATHER_MS = 120;

/**
 * Ctrl + wheel zooms what is under the pointer: over a terminal, every
 * terminal's text (fontSize); anywhere else, the whole window (uiZoom).
 * Taken before the terminal or the page sees it, so a terminal never
 * scrolls and the webview never zooms on its own. Not on the board,
 * where it zooms the board's camera, as it always has.
 */
export function useZoomWheel() {
  useEffect(() => {
    const pending = { terminals: 0, window: 0 };
    let travel = 0;
    let lastTarget: "terminals" | "window" | null = null;
    let timer: number | undefined;

    const flush = () => {
      timer = undefined;
      const { terminals, window: win } = pending;
      pending.terminals = 0;
      pending.window = 0;
      if (terminals) void zoomTerminals(terminals).catch((e) => console.warn(`mirador: zoom not saved: ${e}`));
      if (win) void zoomWindow(win).catch((e) => console.warn(`mirador: zoom not saved: ${e}`));
    };

    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey || e.altKey || e.metaKey) return;
      const over = e.target instanceof Element ? e.target : null;
      // On the board, Ctrl + wheel is the camera's zoom (BoardHost).
      if (over?.closest(".tabs-host.board")) return;
      e.preventDefault();
      e.stopPropagation();
      const target = over?.closest(".xterm") ? "terminals" : "window";
      // Moving from a terminal to the rest mid-gesture starts over.
      if (target !== lastTarget) travel = 0;
      lastTarget = target;
      // Lines and pages are a notch each; pixels add up.
      travel += e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY : Math.sign(e.deltaY) * STEP_PX;
      // Wheel away from you (negative deltaY) zooms in, as everywhere.
      const steps = Math.trunc(-travel / STEP_PX);
      if (steps === 0) return;
      travel += steps * STEP_PX;
      pending[target] += steps;
      if (timer === undefined) timer = window.setTimeout(flush, GATHER_MS);
    };

    window.addEventListener("wheel", onWheel, { capture: true, passive: false });
    return () => {
      window.removeEventListener("wheel", onWheel, { capture: true });
      if (timer !== undefined) {
        window.clearTimeout(timer);
        flush();
      }
    };
  }, []);
}
