import { create } from "zustand";
import { AgentInfo, WorkspaceSnapshot, TabSnapshot, Node } from "../bindings";

interface WorkspaceStore {
  snapshot: WorkspaceSnapshot | null;
  /**
   * The tab whose agents the sidebar lists: the active tab, or — while the
   * active tab is the agent wall, which is a way of looking at agents
   * rather than a place they work — the tab the wall shows.
   */
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

function paneIds(node: Node): string[] {
  return node.type === "leaf" ? [node.paneId] : node.children.flatMap(paneIds);
}

function currentTab(snapshot: WorkspaceSnapshot): string | null {
  const tab = activeTab(snapshot);
  if (!tab) return null;
  const panes = paneIds(tab.root);
  const wall = snapshot.agentWallPanes.find((w) => panes.includes(w.paneId));
  return wall ? wall.tabId : tab.id;
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
