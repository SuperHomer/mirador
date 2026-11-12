import { useEffect } from "react";
import { zoomTerminals } from "../bindings";
import { terminalAt } from "../terminal/registry";

/** The wheel distance that makes one zoom step: a mouse's notch. Smaller
 *  pixel deltas add up to a step the same way. */
const STEP_PX = 100;

/** How long a burst of wheel steps is gathered before it is sent as one
 *  zoom, so a flick writes mirador.json once and not once a notch. */
const GATHER_MS = 120;

/**
 * Ctrl + wheel over a terminal zooms every terminal's text (fontSize),
 * taken before the terminal sees it, so it does not scroll. Anywhere else
 * the wheel is left alone: on the board it zooms the board's camera, as it
 * always has. And it is left to a program that reads the mouse (tmux, vim
 * with the mouse on), which gets Ctrl + wheel as in any other terminal;
 * the zoom keys still work there.
 *
 * A touchpad pinch reaches this only where the webview turns it into
 * Ctrl + wheel, as WebView2 does, and then zooms as a wheel would. On
 * macOS it arrives as gesture events, which nothing here takes.
 */
export function useZoomWheel() {
  useEffect(() => {
    let pending = 0;
    let travel = 0;
    let timer: number | undefined;

    const flush = () => {
      timer = undefined;
      const steps = pending;
      pending = 0;
      if (steps) void zoomTerminals(steps).catch((e) => console.warn(`mirador: zoom not saved: ${e}`));
    };

    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey || e.altKey || e.metaKey) return;
      const over = e.target instanceof Element ? e.target : null;
      // On the board, Ctrl + wheel is the camera's zoom (BoardHost).
      if (!over || over.closest(".tabs-host.board")) return;
      const term = terminalAt(over);
      if (!term || term.modes.mouseTrackingMode !== "none") return;
      e.preventDefault();
      e.stopPropagation();
      // Lines and pages are a notch each; pixels add up.
      travel += e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY : Math.sign(e.deltaY) * STEP_PX;
      // Wheel away from you (negative deltaY) zooms in, as everywhere.
      const steps = Math.trunc(-travel / STEP_PX);
      if (steps === 0) return;
      travel += steps * STEP_PX;
      pending += steps;
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
