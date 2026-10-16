import { create } from "zustand";

interface UiStore {
  paletteOpen: boolean;
  notificationsOpen: boolean;
  /** The agent pane whose activity panel is open. */
  activityPane: string | null;
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
