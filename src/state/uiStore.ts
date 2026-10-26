import { create } from "zustand";
import { AgentStatus } from "../bindings";

/** All of a wall's agents, or those in one status. */
export type WallFilter = "all" | AgentStatus;

interface UiStore {
  paletteOpen: boolean;
  notificationsOpen: boolean;
  /** The agent pane whose activity panel is open. */
  activityPane: string | null;
  /** A frame's "+" menu is open on the board. */
  boardMenuOpen: boolean;
  /** Per tab, the agent its wall shows large in Focus, when one was picked
   *  (from the sidebar). Not persisted. */
  pinned: Record<string, string>;
  pinAgent: (tabId: string, paneId: string | null) => void;
  /** Per tab, which agents its wall shows, by status. Not persisted. */
  wallFilter: Record<string, WallFilter>;
  setWallFilter: (tabId: string, filter: WallFilter) => void;
  setBoardMenuOpen: (open: boolean) => void;
  togglePalette: () => void;
  closePalette: () => void;
  toggleNotifications: () => void;
  closeNotifications: () => void;
  openActivity: (paneId: string) => void;
  closeActivity: () => void;
}

export const useUiStore = create<UiStore>((set) => ({
  paletteOpen: false,
  notificationsOpen: false,
  activityPane: null,
  boardMenuOpen: false,
  pinned: {},
  pinAgent: (tabId, paneId) =>
    set((s) => {
      const pinned = { ...s.pinned };
      if (paneId) pinned[tabId] = paneId;
      else delete pinned[tabId];
      return { pinned };
    }),
  wallFilter: {},
  setWallFilter: (tabId, filter) =>
    set((s) => ({ wallFilter: { ...s.wallFilter, [tabId]: filter } })),
  setBoardMenuOpen: (boardMenuOpen) => set({ boardMenuOpen }),
  togglePalette: () =>
    set((s) => ({ paletteOpen: !s.paletteOpen, notificationsOpen: false })),
  closePalette: () => set({ paletteOpen: false }),
  toggleNotifications: () =>
    set((s) => ({
      notificationsOpen: !s.notificationsOpen,
      paletteOpen: false,
      activityPane: null,
    })),
  closeNotifications: () => set({ notificationsOpen: false }),
  // The two drawers share the window's right edge: one at a time.
  openActivity: (paneId) =>
    set({ activityPane: paneId, notificationsOpen: false, paletteOpen: false }),
  closeActivity: () => set({ activityPane: null }),
}));
