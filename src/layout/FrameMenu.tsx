import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  AgentRole,
  agentRoles,
  openAgent,
  openBrowser,
  openDiff,
  openGraph,
  roleHint,
  splitPane,
} from "../bindings";
import { useBoardStore } from "../state/boardStore";
import { useUiStore } from "../state/uiStore";

/** The menu's width, to line its right edge up with the "+". */
const WIDTH = 220;

/**
 * What a frame's "+" adds to its tab: what the tabs view can create, by
 * the same calls, from the frame's focused pane — a split of it, whose
 * card the board places in the frame. The agent wall opens as it always
 * does, in a tab of its own.
 *
 * Drawn on screen, not on the board: inside the board it would be scaled
 * with it, tiny at a low zoom, and under every card and frame drawn after
 * it. It is an overlay for as long as it is open, so a browser page —
 * a native view, above anything the app draws — hides rather than
 * covering it. It closes on a press elsewhere, Escape, or the board
 * moving away from the "+" it hangs from.
 */
export function FrameMenu({
  pane,
  at,
  onClose,
}: {
  pane: string;
  /** The "+" it hangs from, on screen. */
  at: DOMRect;
  onClose: () => void;
}) {
  const [agents, setAgents] = useState(false);
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    useUiStore.getState().setBoardMenuOpen(true);
    const onPress = (e: PointerEvent) => {
      const target = e.target as Element;
      // The "+" toggles it itself.
      if (ref.current?.contains(target)) return;
      if (target.closest?.("[data-frame-menu]")) return;
      close.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close.current();
    };
    const unwatch = useBoardStore.subscribe((s, prev) => {
      if (s.camera !== prev.camera) close.current();
    });
    // Capture: the press that opened it is over by now, and a press on
    // the board must close it before anything there handles it.
    window.addEventListener("pointerdown", onPress, { capture: true });
    window.addEventListener("keydown", onKey, { capture: true });
    return () => {
      useUiStore.getState().setBoardMenuOpen(false);
      window.removeEventListener("pointerdown", onPress, { capture: true });
      window.removeEventListener("keydown", onKey, { capture: true });
      unwatch();
    };
  }, []);

  // The roles of the frame's focused pane's directory, as the palette's
  // "Agents: New Agent…" lists them.
  useEffect(() => {
    if (!agents) return;
    let live = true;
    agentRoles(pane)
      .then((c) => live && setRoles(c.roles))
      .catch(() => live && setRoles([]));
    return () => {
      live = false;
    };
  }, [agents, pane]);

  const run = (create: () => Promise<unknown>) => () => {
    onClose();
    create().catch((e) => console.error(`mirador: ${e}`));
  };

  return createPortal(
    <div
      ref={ref}
      className="frame-menu"
      style={{
        top: at.bottom + 4,
        left: Math.max(8, at.right - WIDTH),
        width: WIDTH,
      }}
    >
      {agents ? (
        <>
          <button onClick={() => setAgents(false)}>‹ Back</button>
          <button onClick={run(() => openAgent(null, null, pane, false))}>
            <span>Claude</span>
            <span className="frame-menu-hint">default model</span>
          </button>
          {roles.map((r) => (
            <button
              key={r.name}
              disabled={!!r.problem}
              title={r.problem ?? r.description ?? undefined}
              onClick={run(() => openAgent(r.name, null, pane, false))}
            >
              <span>{r.name}</span>
              <span className="frame-menu-hint">{roleHint(r)}</span>
            </button>
          ))}
        </>
      ) : (
        <>
          <button onClick={run(() => splitPane(pane, "row"))}>Terminal</button>
          {/* As the palette's "Pane: New Browser Pane": a blank page, its URL
              typed in the pane's own bar. */}
          <button onClick={run(() => openBrowser(pane, false, "about:blank"))}>
            Browser
          </button>
          <button onClick={() => setAgents(true)}>Agent…</button>
          <button onClick={run(() => openDiff(pane, false, null))}>Diff</button>
          <button onClick={run(() => openGraph(pane, false))}>
            Commit graph
          </button>
        </>
      )}
    </div>,
    document.body,
  );
}
