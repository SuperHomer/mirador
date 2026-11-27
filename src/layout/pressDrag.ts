import { useBoardStore } from "../state/boardStore";

/** How far a press moves before it is a drag rather than a click. */
const DRAG_THRESHOLD = 4;

/**
 * Follows a press that may become a drag: a tab's row in the sidebar, a
 * pane's grip. Moved more than a few pixels, it is a drag, and `move` hears
 * every move from then on; released, `drop` hears the release, and the
 * click that would follow is swallowed, so a drag never also clicks.
 * Escape, a cancelled pointer, or a release the page never heard end it
 * with `cancel`. Less than that, it stays a click, and none of them hears
 * of it.
 *
 * Pointer events, not HTML5 drag and drop: the window takes native file
 * drops, which swallow the webview's own drags. Only the main button
 * drags, and a press on a button or a field is theirs. Browser pages hide
 * meanwhile (`hold`): a native page under the pointer would take its
 * events.
 */
export function pressDrag(
  down: React.PointerEvent<HTMLElement>,
  on: {
    move: (e: PointerEvent) => void;
    drop: (e: PointerEvent) => void;
    cancel: () => void;
  },
) {
  if (down.button !== 0 || (down.target as HTMLElement).closest("button, input")) return;
  const el = down.currentTarget;
  const pointer = down.pointerId;
  const [x0, y0] = [down.clientX, down.clientY];
  let phase: "press" | "drag" | "cancelled" = "press";

  const swallowClick = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
  };
  const stopDragging = () => {
    if (el.hasPointerCapture(pointer)) el.releasePointerCapture(pointer);
    window.removeEventListener("keydown", onKey, { capture: true });
    useBoardStore.getState().hold(false);
  };
  const cancel = () => {
    removeListeners();
    if (phase !== "drag") return;
    stopDragging();
    on.cancel();
  };
  const onMove = (e: PointerEvent) => {
    if (e.pointerId !== pointer) return;
    // The button is up and no release came: it was let go where the page
    // could not hear it.
    if ((e.buttons & 1) === 0) return cancel();
    if (phase === "cancelled") return;
    if (phase === "press") {
      if (Math.max(Math.abs(e.clientX - x0), Math.abs(e.clientY - y0)) <= DRAG_THRESHOLD) return;
      phase = "drag";
      el.setPointerCapture(pointer);
      useBoardStore.getState().hold(true);
      window.addEventListener("keydown", onKey, { capture: true });
    }
    on.move(e);
  };
  const onUp = (e: PointerEvent) => {
    if (e.pointerId !== pointer) return;
    removeListeners();
    if (phase === "press") return;
    // The release ends a drag, not a click.
    window.addEventListener("click", swallowClick, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallowClick, { capture: true }));
    if (phase === "drag") {
      stopDragging();
      on.drop(e);
    }
  };
  const onCancel = (e: PointerEvent) => {
    if (e.pointerId === pointer) cancel();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    // Not the keymap's, nor the terminal's: it cancels the drag. The
    // button is still down, so its release is still awaited, to be no click.
    e.preventDefault();
    e.stopPropagation();
    stopDragging();
    phase = "cancelled";
    on.cancel();
  };
  const removeListeners = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onCancel);
}
