import { create } from "zustand";
import { AgentStatus, StartWhere } from "../bindings";

/** A start waiting for "here, or on the pane's remote?". */
export interface PendingWhere {
  /** What is started, as the step's crumb and in a failure: "Start reviewer". */
  label: string;
  remote: StartWhere;
  go: (onRemote: boolean) => void;
}

/** All of a wall's tiles, the agents in one status, or its tools. */
export type WallFilter = "all" | AgentStatus | "tools";

interface UiStore {
  paletteOpen: boolean;
  /** The palette step to open on (e.g. "team"), when it was asked for. */
  paletteStep: string | null;
  notificationsOpen: boolean;
  /** When the Inbox's updates were last cleared: it shows only later
   *  ones. Not persisted. */
  inboxClearedMs: number | null;
  clearInbox: () => void;
  /** The agent pane whose activity panel is open. */
  activityPane: string | null;
  /** A menu drawn over the panes is open: a frame's "+" on the board, or
   *  a pane's right-click menu. Browser pages hide while it is. */
  menuOpen: boolean;
  /** Per tab, the agent its wall shows large in Focus, when one was picked
   *  (from the sidebar). Not persisted. */
  pinned: Record<string, string>;
  pinAgent: (tabId: string, paneId: string | null) => void;
  /** Per tab, which agents its wall shows, by status. Not persisted. */
  wallFilter: Record<string, WallFilter>;
  setWallFilter: (tabId: string, filter: WallFilter) => void;
  /** Per tab, the page of its wall's Grid on show. Not persisted. */
  gridPage: Record<string, number>;
  setGridPage: (tabId: string, page: number) => void;
  setMenuOpen: (open: boolean) => void;
  /** The tab whose sidebar row is to start renaming, asked from a menu:
   *  the row takes it and clears it. Not persisted. */
  renamingTab: string | null;
  setRenamingTab: (tabId: string | null) => void;
  /** Browser panes a wall's tile draws now: the only place a walled
   *  tab's pages are shown (see `App`). Not persisted. */
  wallBrowsers: string[];
  setWallBrowser: (paneId: string, shown: boolean) => void;
  /** The tab bar's "+ New" menu is open, over the tab's panes. */
  newMenuOpen: boolean;
  setNewMenuOpen: (open: boolean) => void;
  /** A start from a remote pane waiting for the palette's Where step:
   *  `go(false)` starts it here, `go(true)` on the pane's remote
   *  (`startHere`). Dropped when the palette closes without a choice. */
  where: PendingWhere | null;
  askWhere: (where: PendingWhere) => void;
  /** What the person last asked for that failed, said in the error bar
   *  until dismissed; a new failure replaces it (`reportFailure`). */
  error: string | null;
  setError: (error: string | null) => void;
  /** A new computer was asked for: the computers pane starts a draft and
   *  clears this, whether it was open already or opens for it. */
  computerDraft: boolean;
  setComputerDraft: (draft: boolean) => void;
  togglePalette: () => void;
  /** Opens the palette on one of its steps. */
  openPalette: (step: string) => void;
  closePalette: () => void;
  toggleNotifications: () => void;
  closeNotifications: () => void;
  openActivity: (paneId: string) => void;
  closeActivity: () => void;
}

export const useUiStore = create<UiStore>((set) => ({
  paletteOpen: false,
  paletteStep: null,
  notificationsOpen: false,
  inboxClearedMs: null,
  clearInbox: () => set({ inboxClearedMs: Date.now() }),
  activityPane: null,
  menuOpen: false,
  pinned: {},
  pinAgent: (tabId, paneId) =>
    set((s) => {
      const pinned = { ...s.pinned };
      if (paneId) pinned[tabId] = paneId;
      else delete pinned[tabId];
      return { pinned };
    }),
  wallFilter: {},
  setWallFilter: (tabId, filter) =>
    set((s) => ({ wallFilter: { ...s.wallFilter, [tabId]: filter } })),
  gridPage: {},
  setGridPage: (tabId, page) =>
    set((s) => ({ gridPage: { ...s.gridPage, [tabId]: page } })),
  setMenuOpen: (menuOpen) => set({ menuOpen }),
  renamingTab: null,
  setRenamingTab: (renamingTab) => set({ renamingTab }),
  wallBrowsers: [],
  setWallBrowser: (paneId, shown) =>
    set((s) => {
      const rest = s.wallBrowsers.filter((p) => p !== paneId);
      return { wallBrowsers: shown ? [...rest, paneId] : rest };
    }),
  newMenuOpen: false,
  setNewMenuOpen: (newMenuOpen) => set({ newMenuOpen }),
  error: null,
  setError: (error) => set({ error }),
  where: null,
  askWhere: (where) =>
    set({ where, paletteOpen: true, paletteStep: "where", notificationsOpen: false, activityPane: null }),
  computerDraft: false,
  setComputerDraft: (computerDraft) => set({ computerDraft }),
  togglePalette: () =>
    set((s) => ({ paletteOpen: !s.paletteOpen, paletteStep: null, notificationsOpen: false, where: null })),
  openPalette: (step) =>
    set({ paletteOpen: true, paletteStep: step, notificationsOpen: false, activityPane: null }),
  // A start waiting for its Where step is dropped with it: nothing opens.
  closePalette: () => set({ paletteOpen: false, where: null }),
  toggleNotifications: () =>
    set((s) => ({
      notificationsOpen: !s.notificationsOpen,
      paletteOpen: false,
      activityPane: null,
      where: null,
    })),
  closeNotifications: () => set({ notificationsOpen: false }),
  // The two drawers share the window's right edge: one at a time.
  openActivity: (paneId) =>
    set({ activityPane: paneId, notificationsOpen: false, paletteOpen: false, where: null }),
  closeActivity: () => set({ activityPane: null }),
}));

/**
 * Says that something the person asked for — opening a tab, a split, an
 * agent, a team — failed: in the error bar, where nothing else would say
 * it, and in the log.
 */
export function reportFailure(what: string, e: unknown) {
  console.error(`mirador: ${what}: ${e}`);
  useUiStore.getState().setError(`${what}: ${e}`);
}
