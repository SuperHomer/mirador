// Hand-written mirror of cmux-protocol types and command wrappers.
// (Replaced by tauri-specta generation in a later milestone.)
import { invoke, Channel } from "@tauri-apps/api/core";

export type SplitDir = "row" | "column";
export type Direction = "left" | "right" | "up" | "down";

export type Node =
  | { type: "leaf"; paneId: string }
  | { type: "split"; dir: SplitDir; ratios: number[]; children: Node[] };

export interface TabSnapshot {
  id: string;
  title: string;
  cwd: string | null;
  root: Node;
  focusedPane: string;
  unread: number;
  lastNotification: string | null;
}

export interface AgentPane {
  paneId: string;
  command: string;
}

export interface WorkspaceSnapshot {
  tabs: TabSnapshot[];
  activeTab: string;
  unreadPanes: string[];
  agentPanes: AgentPane[];
}

export interface NotificationDto {
  id: string;
  paneId: string;
  title: string | null;
  body: string;
  atMs: number;
  read: boolean;
}

export type PtyData = ArrayBuffer | Uint8Array | string;

export interface CustomCommand {
  name: string;
  command: string;
  target: "tab" | "split" | string;
}

export interface ResolvedColors {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  palette: string[];
}

export interface ResolvedConfig {
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  colors: ResolvedColors;
  keybindings: Record<string, string>;
  customCommands: CustomCommand[];
}

export const workspaceSnapshot = () =>
  invoke<WorkspaceSnapshot>("workspace_snapshot");
export const getConfig = () => invoke<ResolvedConfig>("get_config");
export const newTab = (command?: string) =>
  invoke<{ tabId: string; paneId: string }>("new_tab", {
    command: command ?? null,
  });
export const closeTab = (tabId: string) => invoke<void>("close_tab", { tabId });
export const setActiveTab = (tabId: string) =>
  invoke<void>("set_active_tab", { tabId });
export const renameTab = (tabId: string, title: string) =>
  invoke<void>("rename_tab", { tabId, title });
export const moveTab = (tabId: string, to: number) =>
  invoke<void>("move_tab", { tabId, to });
export const splitPane = (paneId: string, dir: SplitDir, command?: string) =>
  invoke<string>("split_pane", { paneId, dir, command: command ?? null });
export const closePane = (paneId: string) =>
  invoke<void>("close_pane", { paneId });
export const focusPane = (paneId: string) =>
  invoke<void>("focus_pane", { paneId });
export const focusDirection = (direction: Direction) =>
  invoke<void>("focus_direction", { direction });
export const setSplitRatios = (
  tabId: string,
  path: number[],
  ratios: number[],
) => invoke<void>("set_split_ratios", { tabId, path, ratios });
/** Returns true when a fresh shell was spawned (vs re-attached). */
export const attachPane = (
  paneId: string,
  cols: number,
  rows: number,
  onData: Channel<PtyData>,
) => invoke<boolean>("attach_pane", { paneId, cols, rows, onData });
export const writePty = (paneId: string, data: string) =>
  invoke<void>("write_pty", { paneId, data });
export const resizePty = (paneId: string, cols: number, rows: number) =>
  invoke<void>("resize_pty", { paneId, cols, rows });
export const ackPty = (paneId: string, bytes: number) =>
  invoke<void>("ack_pty", { paneId, bytes });
export const listNotifications = () =>
  invoke<NotificationDto[]>("list_notifications");
export const markAllNotificationsRead = () =>
  invoke<void>("mark_all_notifications_read");
