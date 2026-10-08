import { create } from "zustand";
import { WorkspaceSnapshot, TabSnapshot, Node } from "../bindings";

interface WorkspaceStore {
  snapshot: WorkspaceSnapshot | null;
  /**
   * The project on screen: the active tab's. While the active tab has none
   * of its own to offer — the agent wall, a pane with no directory — it
   * stays the project of the tab before, so the wall and the sidebar keep
   * showing the agents of the project you came from.
   */
  project: string | null;
  sidebarVisible: boolean;
  setSnapshot: (s: WorkspaceSnapshot) => void;
  toggleSidebar: () => void;
}

export const useWorkspaceStore = create<WorkspaceStore>((set) => ({
  snapshot: null,
  project: null,
  sidebarVisible: true,
  setSnapshot: (snapshot) =>
    set((s) => {
      const tab = activeTab(snapshot);
      const own = tab && !isWallTab(snapshot, tab) ? tab.project : null;
      return { snapshot, project: own ?? s.project };
    }),
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

/** The tab holds the agent wall. */
export function isWallTab(snapshot: WorkspaceSnapshot, tab: TabSnapshot) {
  const panes = paneIds(tab.root);
  return snapshot.agentWallPanes.some((w) => panes.includes(w.paneId));
}

/**
 * Agent tabs the sidebar lists: those of the project on screen. With no
 * project known yet, all of them.
 */
export function shownAgentTabs(
  snapshot: WorkspaceSnapshot | null,
  project: string | null,
): TabSnapshot[] {
  if (!snapshot) return [];
  return snapshot.tabs.filter(
    (t) => t.agent && (project === null || t.project === project),
  );
}

/**
 * Tabs in the order the sidebar shows them: the rest first, then the
 * agents of the project on screen; other projects' agents are not listed.
 * Tab shortcuts (mod+1…9, next/previous) go by this order too, so the
 * number beside a tab is the one that reaches it.
 */
export function orderedTabs(
  snapshot: WorkspaceSnapshot | null,
  project: string | null,
): TabSnapshot[] {
  if (!snapshot) return [];
  return [
    ...snapshot.tabs.filter((t) => !t.agent),
    ...shownAgentTabs(snapshot, project),
  ];
}

/** The last path component, for naming a project in a heading. */
export function projectName(project: string): string {
  return project.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || project;
}
