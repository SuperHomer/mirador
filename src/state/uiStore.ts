import { create } from "zustand";

interface UiStore {
  paletteOpen: boolean;
  togglePalette: () => void;
  closePalette: () => void;
}

export const useUiStore = create<UiStore>((set) => ({
  paletteOpen: false,
  togglePalette: () => set((s) => ({ paletteOpen: !s.paletteOpen })),
  closePalette: () => set({ paletteOpen: false }),
}));
