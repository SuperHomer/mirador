import { create } from "zustand";
import { UpdateInfo } from "../bindings";

interface UpdateStore {
  update: UpdateInfo | null;
  /** True from the moment the user asks to install until the app restarts. */
  installing: boolean;
  error: string | null;
  setUpdate: (u: UpdateInfo | null) => void;
  setInstalling: (v: boolean) => void;
  setError: (e: string | null) => void;
}

export const useUpdateStore = create<UpdateStore>((set) => ({
  update: null,
  installing: false,
  error: null,
  setUpdate: (update) => set({ update }),
  setInstalling: (installing) => set({ installing }),
  setError: (error) => set({ error, installing: false }),
}));
