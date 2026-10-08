import { create } from "zustand";
import { WorkspaceSnapshot, TabSnapshot } from "../bindings";

interface WorkspaceStore {
  snapshot: WorkspaceSnapshot | null;
  sidebarVisible: boolean;
  setSnapshot: (s: WorkspaceSnapshot) => void;
  toggleSidebar: () => void;
}

export const useWorkspaceStore = create<WorkspaceStore>((set) => ({
  snapshot: null,
  sidebarVisible: true,
  setSnapshot: (snapshot) => set({ snapshot }),
  toggleSidebar: () => set((s) => ({ sidebarVisible: !s.sidebarVisible })),
}));

export function activeTab(
  snapshot: WorkspaceSnapshot | null,
): TabSnapshot | null {
  if (!snapshot) return null;
  return snapshot.tabs.find((t) => t.id === snapshot.activeTab) ?? null;
}

/**
 * Tabs in the order the sidebar shows them: the rest first, then the tabs
 * running agents, each group keeping its own order. Tab shortcuts (mod+1…9,
 * next/previous) go by this order too, so the number beside a tab is the
 * one that reaches it.
 */
export function orderedTabs(snapshot: WorkspaceSnapshot | null): TabSnapshot[] {
  if (!snapshot) return [];
  return [
    ...snapshot.tabs.filter((t) => !t.agent),
    ...snapshot.tabs.filter((t) => t.agent),
  ];
}
