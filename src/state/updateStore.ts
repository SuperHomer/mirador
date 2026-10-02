import { create } from "zustand";
import { UpdateInfo } from "../bindings";

/**
 * Outcome of the most recent *user-initiated* check. Background checks leave
 * this alone: they are allowed to find nothing silently, but a person who
 * picked "Check for Updates" is owed an answer either way.
 */
export type CheckResult = null | "checking" | "uptodate" | "error";

/** Bytes so far, and the total when the server bothered to say. */
export interface Progress {
  downloaded: number;
  total: number | null;
}

interface UpdateStore {
  update: UpdateInfo | null;
  /** True from the moment the user asks to install until the app restarts. */
  installing: boolean;
  /** Download progress, or null once the download is done (or never started). */
  progress: Progress | null;
  error: string | null;
  check: CheckResult;
  setUpdate: (u: UpdateInfo | null) => void;
  setInstalling: (v: boolean) => void;
  setProgress: (p: Progress | null) => void;
  setError: (e: string | null) => void;
  setCheck: (c: CheckResult) => void;
}

export const useUpdateStore = create<UpdateStore>((set) => ({
  update: null,
  installing: false,
  progress: null,
  error: null,
  check: null,
  setUpdate: (update) => set({ update }),
  setInstalling: (installing) => set({ installing }),
  setProgress: (progress) => set({ progress }),
  setError: (error) => set({ error, installing: false, progress: null }),
  setCheck: (check) => set({ check }),
}));
