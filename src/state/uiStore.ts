import { create } from "zustand";

/** Which agents the wall draws: the current tab's, or every one. */
export type WallScope = "tab" | "all";

interface UiStore {
  wallScope: WallScope;
  setWallScope: (scope: WallScope) => void;
  paletteOpen: boolean;
  notificationsOpen: boolean;
  togglePalette: () => void;
  closePalette: () => void;
  toggleNotifications: () => void;
  closeNotifications: () => void;
}

export const useUiStore = create<UiStore>((set) => ({
  wallScope: "tab",
  setWallScope: (wallScope) => set({ wallScope }),
  paletteOpen: false,
  notificationsOpen: false,
  togglePalette: () =>
    set((s) => ({ paletteOpen: !s.paletteOpen, notificationsOpen: false })),
  closePalette: () => set({ paletteOpen: false }),
  toggleNotifications: () =>
    set((s) => ({
      notificationsOpen: !s.notificationsOpen,
      paletteOpen: false,
    })),
  closeNotifications: () => set({ notificationsOpen: false }),
}));
