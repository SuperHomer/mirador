import { create } from "zustand";
import { AgentInfo, WorkspaceSnapshot, TabSnapshot, Node } from "../bindings";

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

/** The agents the sidebar lists: the project on screen's (all, with none known yet). */
export function shownAgents(
  snapshot: WorkspaceSnapshot | null,
  project: string | null,
): AgentInfo[] {
  if (!snapshot) return [];
  return snapshot.agents.filter(
    (a) => project === null || a.project === project,
  );
}

/** A sidebar row: a tab, or one agent in a tab. */
export type SidebarEntry =
  | { kind: "tab"; tab: TabSnapshot }
  | { kind: "agent"; agent: AgentInfo };

/**
 * The sidebar's rows in order: every tab, then the agents of the project on
 * screen. A tab shows all its panes; an agent's row shows that agent's pane
 * alone (zoomed) in its tab — so a tab running Claude beside a dev server
 * is listed both ways. Tab shortcuts (mod+1…9, next/previous) go by this
 * order, so the number beside a row is the one that reaches it.
 */
export function sidebarEntries(
  snapshot: WorkspaceSnapshot | null,
  project: string | null,
): SidebarEntry[] {
  if (!snapshot) return [];
  return [
    ...snapshot.tabs.map((tab) => ({ kind: "tab" as const, tab })),
    ...shownAgents(snapshot, project).map((agent) => ({
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

/** The last path component, for naming a project in a heading. */
export function projectName(project: string): string {
  return project.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || project;
}
