import { create } from "zustand";

interface UiStore {
  paletteOpen: boolean;
  notificationsOpen: boolean;
  togglePalette: () => void;
  closePalette: () => void;
  toggleNotifications: () => void;
  closeNotifications: () => void;
}

export const useUiStore = create<UiStore>((set) => ({
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
