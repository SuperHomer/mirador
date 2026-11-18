import { create } from "zustand";
import { AgentStatus } from "../bindings";

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
  /** A frame's "+" menu is open on the board. */
  boardMenuOpen: boolean;
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
  setBoardMenuOpen: (open: boolean) => void;
  /** Browser panes a wall's tile draws now: the only place a walled
   *  tab's pages are shown (see `App`). Not persisted. */
  wallBrowsers: string[];
  setWallBrowser: (paneId: string, shown: boolean) => void;
  /** The tab bar's "+ New" menu is open, over the tab's panes. */
  newMenuOpen: boolean;
  setNewMenuOpen: (open: boolean) => void;
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
  boardMenuOpen: false,
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
  setBoardMenuOpen: (boardMenuOpen) => set({ boardMenuOpen }),
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
  computerDraft: false,
  setComputerDraft: (computerDraft) => set({ computerDraft }),
  togglePalette: () =>
    set((s) => ({ paletteOpen: !s.paletteOpen, paletteStep: null, notificationsOpen: false })),
  openPalette: (step) =>
    set({ paletteOpen: true, paletteStep: step, notificationsOpen: false, activityPane: null }),
  closePalette: () => set({ paletteOpen: false }),
  toggleNotifications: () =>
    set((s) => ({
      notificationsOpen: !s.notificationsOpen,
      paletteOpen: false,
      activityPane: null,
    })),
  closeNotifications: () => set({ notificationsOpen: false }),
  // The two drawers share the window's right edge: one at a time.
  openActivity: (paneId) =>
    set({ activityPane: paneId, notificationsOpen: false, paletteOpen: false }),
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
