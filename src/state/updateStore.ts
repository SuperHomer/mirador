import { create } from "zustand";
import { UpdateInfo } from "../bindings";

/**
 * Outcome of the most recent *user-initiated* check. Background checks leave
 * this alone: they are allowed to find nothing silently, but a person who
 * picked "Check for Updates" is owed an answer either way.
 */
export type CheckResult = null | "checking" | "uptodate" | "error";

interface UpdateStore {
  update: UpdateInfo | null;
  /** True from the moment the user asks to install until the app restarts. */
  installing: boolean;
  error: string | null;
  check: CheckResult;
  setUpdate: (u: UpdateInfo | null) => void;
  setInstalling: (v: boolean) => void;
  setError: (e: string | null) => void;
  setCheck: (c: CheckResult) => void;
}

export const useUpdateStore = create<UpdateStore>((set) => ({
  update: null,
  installing: false,
  error: null,
  check: null,
  setUpdate: (update) => set({ update }),
  setInstalling: (installing) => set({ installing }),
  setError: (error) => set({ error, installing: false }),
  setCheck: (check) => set({ check }),
}));
