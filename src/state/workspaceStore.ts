import { create } from "zustand";
import { WorkspaceSnapshot, TabSnapshot } from "../bindings";

interface WorkspaceStore {
  snapshot: WorkspaceSnapshot | null;
  /** The tab whose agents the sidebar lists: the active tab. */
  currentTab: string | null;
  sidebarVisible: boolean;
  setSnapshot: (s: WorkspaceSnapshot) => void;
  toggleSidebar: () => void;
}

export const useWorkspaceStore = create<WorkspaceStore>((set) => ({
  snapshot: null,
  currentTab: null,
  sidebarVisible: true,
  setSnapshot: (snapshot) => set({ snapshot, currentTab: currentTab(snapshot) }),
  toggleSidebar: () => set((s) => ({ sidebarVisible: !s.sidebarVisible })),
}));

export function activeTab(
  snapshot: WorkspaceSnapshot | null,
): TabSnapshot | null {
  if (!snapshot) return null;
  return snapshot.tabs.find((t) => t.id === snapshot.activeTab) ?? null;
}

function currentTab(snapshot: WorkspaceSnapshot): string | null {
  return activeTab(snapshot)?.id ?? null;
}
