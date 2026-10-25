import { create } from "zustand";
import { AgentInfo, WorkspaceSnapshot, TabSnapshot } from "../bindings";

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

/** The agents the sidebar lists: the current tab's, and no other tab's. */
export function shownAgents(
  snapshot: WorkspaceSnapshot | null,
  currentTab: string | null,
): AgentInfo[] {
  if (!snapshot || !currentTab) return [];
  return snapshot.agents.filter((a) => a.tabId === currentTab);
}

/** A sidebar row: a tab, or one agent in a tab. */
export type SidebarEntry =
  | { kind: "tab"; tab: TabSnapshot }
  | { kind: "agent"; agent: AgentInfo };

/**
 * The sidebar's rows in order: every tab, then the current tab's agents. A tab shows all its panes; an agent's row shows that agent's pane
 * alone (zoomed) in its tab — so a tab running Claude beside a dev server
 * is listed both ways. Tab shortcuts (mod+1…9, next/previous) go by this
 * order, so the number beside a row is the one that reaches it.
 */
export function sidebarEntries(
  snapshot: WorkspaceSnapshot | null,
  currentTab: string | null,
): SidebarEntry[] {
  if (!snapshot) return [];
  return [
    ...snapshot.tabs.map((tab) => ({ kind: "tab" as const, tab })),
    ...shownAgents(snapshot, currentTab).map((agent) => ({
      kind: "agent" as const,
      agent,
    })),
  ];
}

/** The row for what is on screen: an agent's when its pane is zoomed. */
export function isCurrentEntry(
  snapshot: WorkspaceSnapshot,
  entry: SidebarEntry,
): boolean {
  const tab = activeTab(snapshot);
  if (!tab) return false;
  if (entry.kind === "agent") return tab.zoomedPane === entry.agent.paneId;
  return entry.tab.id === tab.id && !tab.zoomedPane;
}
