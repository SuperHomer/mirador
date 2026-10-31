import { useCallback, useEffect, useState } from "react";
import { TabSnapshot } from "../bindings";
import { NewMenu } from "../agents/NewMenu";
import { acceleratorsByAction, formatAccel } from "../keymap/accelerator";
import { useConfigStore } from "../state/configStore";
import { useWorkspaceStore } from "../state/workspaceStore";

/**
 * The slim bar over a tab's content, off the board (where each frame has
 * its own title bar): the sidebar's toggle, then which tab this is and the
 * branch of its focused pane, then + New, for a tab, an agent or a pane.
 */
export function TabBar({ tab, current }: { tab: TabSnapshot; current: boolean }) {
  const sidebarVisible = useWorkspaceStore((s) => s.sidebarVisible);
  const accel = acceleratorsByAction(useConfigStore((s) => s.config?.keybindings)).get(
    "toggle_sidebar",
  );
  const action = sidebarVisible ? "Hide sidebar" : "Show sidebar";
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  // The menu belongs to the tab on screen. Whatever switches tabs — a key,
  // the palette, the sidebar, `mira` — changes the snapshot's active tab,
  // so the menu of a tab gone under another closes, and with it the flag
  // that hides every browser page. Going to the board unmounts the bar.
  useEffect(() => {
    if (!current) setMenuOpen(false);
  }, [current]);
  return (
    <div className="tab-bar">
      <button
        className="tab-bar-icon"
        title={accel ? `${action} (${formatAccel(accel)})` : action}
        aria-label={action}
        onClick={() => useWorkspaceStore.getState().toggleSidebar()}
      >
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3">
          <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
          <line x1="6" y1="2.5" x2="6" y2="13.5" />
        </svg>
      </button>
      <span className="tab-bar-label">
        {tab.title}
        {tab.branch && ` · ⎇ ${tab.branch}`}
      </span>
      <div className="tab-bar-new">
        <button
          className={`tab-bar-new-button${menuOpen ? " open" : ""}`}
          title="A new tab, agent or pane"
          // The menu remembers which terminal had the keyboard, to give it
          // back; a click must not move it here first.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setMenuOpen((o) => !o)}
        >
          + New ▾
        </button>
        {menuOpen && current && <NewMenu pane={tab.focusedPane} onClose={closeMenu} />}
      </div>
    </div>
  );
}
