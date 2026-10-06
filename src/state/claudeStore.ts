import { create } from "zustand";
import { setupClaudeIntegration } from "../bindings";

/**
 * The Claude Code integration offer and its outcome. Shared by the sidebar
 * banner, which makes the offer, and the palette entry, which runs the same
 * setup on demand and shows its answer in the same place.
 */
export type ClaudeState = "idle" | "offer" | "running" | "done" | "error";

interface ClaudeStore {
  state: ClaudeState;
  error: string | null;
  setState: (s: ClaudeState) => void;
  run: () => Promise<void>;
}

export const useClaudeStore = create<ClaudeStore>((set) => ({
  state: "idle",
  error: null,
  setState: (state) => set({ state }),
  run: async () => {
    set({ state: "running", error: null });
    try {
      await setupClaudeIntegration();
      set({ state: "done" });
    } catch (e) {
      set({ state: "error", error: String(e) });
    }
  },
}));
