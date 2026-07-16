import { useEffect } from "react";
import {
  newTab,
  closePane,
  splitPane,
  focusDirection,
  setActiveTab,
  Direction,
} from "../bindings";
import { useWorkspaceStore, activeTab } from "../state/workspaceStore";

const isMac = navigator.platform.toUpperCase().includes("MAC");

/** Hardcoded default keymap (config-driven in M3). "mod" = Cmd on macOS,
 * Ctrl elsewhere. */
export function useKeymap() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod) return;

      const { snapshot, toggleSidebar } = useWorkspaceStore.getState();
      const tab = activeTab(snapshot);
      const focused = tab?.focusedPane;

      const arrowDirs: Record<string, Direction> = {
        ArrowLeft: "left",
        ArrowRight: "right",
        ArrowUp: "up",
        ArrowDown: "down",
      };

      let handled = true;
      if (e.key === "t" && !e.shiftKey && !e.altKey) {
        void newTab();
      } else if (e.key === "w" && !e.shiftKey && !e.altKey) {
        if (focused) void closePane(focused);
      } else if ((e.key === "d" || e.key === "D") && !e.altKey) {
        if (focused) void splitPane(focused, e.shiftKey ? "column" : "row");
      } else if (e.altKey && e.key in arrowDirs) {
        void focusDirection(arrowDirs[e.key]);
      } else if (e.key === "b" && !e.shiftKey && !e.altKey) {
        toggleSidebar();
      } else if (/^[1-9]$/.test(e.key) && !e.shiftKey && !e.altKey) {
        const idx = Number(e.key) - 1;
        const target = snapshot?.tabs[idx];
        if (target) void setActiveTab(target.id);
      } else {
        handled = false;
      }

      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, []);
}
