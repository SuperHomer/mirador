import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useBoardStore } from "../state/boardStore";
import { useUiStore } from "../state/uiStore";

export interface MenuItem {
  label: string;
  /** Shown on the right: the item's keyboard shortcut, when it has one. */
  hint?: string;
  disabled?: boolean;
  run: () => void;
}

/** A line between groups of items. */
export const SEPARATOR = "separator";

/** What a menu lists: items and the separators between them. */
export type MenuEntry = MenuItem | typeof SEPARATOR;

/** How close to the window's edge the menu may come. */
const MARGIN = 8;

/**
 * A right-click's menu, drawn at the pointer and kept inside the window.
 *
 * Like FrameMenu it is drawn on screen, above the panes and the board, and
 * is an overlay for as long as it is open, so a browser page — a native
 * view, above anything the app draws — hides rather than covering it. It
 * closes on a press elsewhere, Escape, the window losing focus or
 * resizing, the board moving away from where it was opened, and once an
 * item has run.
 */
export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuEntry[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ x, y });
  const close = useRef(onClose);
  close.current = onClose;

  // Its size is known only once drawn: moved back inside before it shows.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setAt({
      x: Math.max(MARGIN, Math.min(x, window.innerWidth - width - MARGIN)),
      y: Math.max(MARGIN, Math.min(y, window.innerHeight - height - MARGIN)),
    });
  }, [x, y]);

  useEffect(() => {
    useUiStore.getState().setMenuOpen(true);
    const onPress = (e: PointerEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      close.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close.current();
    };
    const onGone = () => close.current();
    const unwatch = useBoardStore.subscribe((s, prev) => {
      if (s.camera !== prev.camera) close.current();
    });
    // Capture: a press elsewhere closes it before anything there handles
    // the press — a right-click on another pane opens that one's after.
    window.addEventListener("pointerdown", onPress, { capture: true });
    window.addEventListener("keydown", onKey, { capture: true });
    window.addEventListener("blur", onGone);
    window.addEventListener("resize", onGone);
    return () => {
      useUiStore.getState().setMenuOpen(false);
      window.removeEventListener("pointerdown", onPress, { capture: true });
      window.removeEventListener("keydown", onKey, { capture: true });
      window.removeEventListener("blur", onGone);
      window.removeEventListener("resize", onGone);
      unwatch();
    };
  }, []);

  return createPortal(
    <div
      ref={ref}
      className="pop-menu"
      style={{ top: at.y, left: at.x }}
      // A press on an item keeps the keyboard where it was — in the
      // terminal — which WebView2 and WebKitGTK would otherwise move to it.
      onMouseDown={(e) => e.preventDefault()}
      // A right-click on the menu itself opens nothing else.
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, i) =>
        item === SEPARATOR ? (
          <hr key={i} />
        ) : (
          <button
            key={item.label}
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.run();
            }}
          >
            <span>{item.label}</span>
            {item.hint && <span className="pop-menu-hint">{item.hint}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
