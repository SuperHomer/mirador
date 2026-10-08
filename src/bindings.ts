// Hand-written mirror of the Rust protocol crate types and command wrappers.
// (Replaced by tauri-specta generation in a later milestone.)
import { invoke, Channel } from "@tauri-apps/api/core";

export type SplitDir = "row" | "column";
export type Direction = "left" | "right" | "up" | "down";

export type Node =
  | { type: "leaf"; paneId: string }
  | { type: "split"; dir: SplitDir; ratios: number[]; children: Node[] };

export interface PrStatus {
  number: number;
  state: "OPEN" | "MERGED" | "CLOSED" | string;
  url: string;
  checks: "pass" | "fail" | "pending" | "none" | string;
}

export interface TabSnapshot {
  id: string;
  title: string;
  cwd: string | null;
  root: Node;
  focusedPane: string;
  unread: number;
  lastNotification: string | null;
  branch: string | null;
  pr: PrStatus | null;
  ports: number[];
  /** Runs a Claude Code agent (the wall's tab does not count). */
  agent: boolean;
}

export interface AgentPane {
  paneId: string;
  command: string;
}

export interface BrowserPaneInfo {
  paneId: string;
  url: string;
}

export interface DiffPaneInfo {
  paneId: string;
  repo: string;
  spec: string;
}

export interface DiffLine {
  kind: "context" | "add" | "del" | "meta";
  oldLine: number | null;
  newLine: number | null;
  content: string;
}

export interface DiffHunk {
  oldStart: number;
  newStart: number;
  header: string;
  lines: DiffLine[];
}

export interface DiffFile {
  path: string;
  oldPath: string | null;
  status: "added" | "deleted" | "renamed" | "copied" | "modified" | "untracked";
  additions: number;
  deletions: number;
  binary: boolean;
  truncated: boolean;
  hunks: DiffHunk[];
}

export interface Worktree {
  path: string;
  branch: string | null;
  head: string | null;
  main: boolean;
  bare: boolean;
  locked: boolean;
  prunable: boolean;
  current: boolean;
}

export interface DiffResult {
  repo: string;
  spec: string;
  label: string;
  files: DiffFile[];
}

export type NoteSpan =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string }
  | { kind: "link"; text: string; href: string };

export type NoteBlock =
  | { kind: "image"; alt: string; url: string }
  | { kind: "heading"; level: number; spans: NoteSpan[] }
  | { kind: "paragraph"; spans: NoteSpan[] }
  | { kind: "list"; items: NoteSpan[][] }
  | { kind: "code"; text: string; lang: string | null }
  | { kind: "rule" };

export interface ReleaseNotes {
  version: string;
  title: string;
  /** Parsed in Rust; see cmux-core/src/notes.rs for why. */
  blocks: NoteBlock[];
  url: string;
}

export interface WhatsNewPaneInfo {
  paneId: string;
  version: string;
  /** Set when the pane covers every release after this one up to `version`. */
  since: string | null;
}

export interface GraphRef {
  name: string;
  kind: "head" | "branch" | "remote" | "tag";
}

/** One lane segment between a row and the row below it. */
export interface GraphLink {
  from: number;
  to: number;
}

export interface GraphRow {
  sha: string;
  short: string;
  subject: string;
  author: string;
  timestamp: number;
  refs: GraphRef[];
  lane: number;
  links: GraphLink[];
  merge: boolean;
}

export interface GraphResult {
  repo: string;
  rows: GraphRow[];
  lanes: number;
  truncated: boolean;
}

export interface GraphPaneInfo {
  paneId: string;
  repo: string;
}

export type AgentStatus = "working" | "needsYou" | "idle";

/** A pane running a Claude Code agent. */
export interface AgentInfo {
  paneId: string;
  role: string | null;
  model: string | null;
  /** Null until its first hook arrives. */
  status: AgentStatus | null;
  message: string | null;
  sinceMs: number | null;
  cwd: string | null;
  branch: string | null;
}

export interface AgentWallPaneInfo {
  paneId: string;
}

export interface AgentRole {
  name: string;
  model: string | null;
  prompt: string | null;
}

export interface RemotePaneInfo {
  paneId: string;
  host: string;
}

export interface WorkspaceSnapshot {
  tabs: TabSnapshot[];
  activeTab: string;
  unreadPanes: string[];
  agentPanes: AgentPane[];
  browserPanes: BrowserPaneInfo[];
  remotePanes: RemotePaneInfo[];
  diffPanes: DiffPaneInfo[];
  whatsNewPanes: WhatsNewPaneInfo[];
  graphPanes: GraphPaneInfo[];
  agentWallPanes: AgentWallPaneInfo[];
  agents: AgentInfo[];
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
  macOptionIsMeta: boolean;
  colors: ResolvedColors;
  keybindings: Record<string, string>;
  customCommands: CustomCommand[];
  agentRoles: AgentRole[];
}

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

export const appVersion = () => invoke<string>("app_version");
/** The update a previous check found, if any. Pulled on mount because the
 *  `update-available` event can fire before React is listening. */
export const availableUpdate = () =>
  invoke<UpdateInfo | null>("available_update");
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");
/** Downloads, verifies, installs and restarts. Does not return. */
export const installUpdate = () => invoke<void>("install_update");

export const workspaceSnapshot = () =>
  invoke<WorkspaceSnapshot>("workspace_snapshot");
export const getConfig = () => invoke<ResolvedConfig>("get_config");
/** Whether to offer the Claude Code integration (decided in Rust). */
export const claudeIntegrationOffer = () => invoke<boolean>("claude_integration_offer");
/** `mira` on PATH, then the hooks and `/mira-diff`. Lines of what was done. */
export const setupClaudeIntegration = () => invoke<string[]>("setup_claude_integration");
/** "Not now": the offer is not made again. */
export const dismissClaudeIntegration = () => invoke<void>("dismiss_claude_integration");
/** Ends every terminal session, held ones included, then quits. */
export const quitEndingSessions = () => invoke<void>("quit_ending_sessions");
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
/**
 * "spawned" | "reattached" | "restored" (session command pane, idle).
 * `rerun` marks the attach as user-initiated: only then may a pane restored
 * from the last session actually run its command.
 */
export const attachPane = (
  paneId: string,
  cols: number,
  rows: number,
  onData: Channel<PtyData>,
  rerun = false,
) =>
  invoke<"spawned" | "reattached" | "resumed" | "restored">("attach_pane", {
    paneId,
    cols,
    rows,
    onData,
    rerun,
  });
export const openBrowser = (paneId: string | null, tab: boolean, url: string) =>
  invoke<string>("open_browser", { paneId, tab, url });
export const openDiff = (
  paneId: string | null,
  tab: boolean,
  spec: string | null,
  worktree: string | null = null,
) => invoke<string>("open_diff", { paneId, tab, spec, worktree });
export const loadDiff = (paneId: string) =>
  invoke<DiffResult>("load_diff", { paneId });
export const setDiffSpec = (paneId: string, spec: string) =>
  invoke<void>("set_diff_spec", { paneId, spec });
/** Newest first: one release, or every release after `since`. */
export const whatsNew = (version: string, since: string | null) =>
  invoke<ReleaseNotes[]>("whats_new", { version, since });
/** Omit `version` for the running build; pass one to preview an update's. */
/** Raw image bytes, fetched and cached by the backend — never by the page. */
export const releaseImage = (url: string) =>
  invoke<ArrayBuffer>("release_image", { url });
export const openWhatsNew = (
  version: string | null = null,
  since: string | null = null,
) => invoke<string>("open_whats_new", { version, since });
export const openGraph = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_graph", { paneId, tab });
export const openAgent = (
  role: string | null,
  task: string | null,
  paneId: string | null,
  tab: boolean,
) => invoke<string>("open_agent", { role, task, paneId, tab });
export const markPaneRead = (paneId: string) =>
  invoke<void>("mark_pane_read", { paneId });
export const openAgentWall = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_agent_wall", { paneId, tab });
export const loadGraph = (paneId: string, limit: number | null = null) =>
  invoke<GraphResult>("load_graph", { paneId, limit });
export const graphShowCommit = (paneId: string, sha: string) =>
  invoke<string>("graph_show_commit", { paneId, sha });
export const listWorktrees = (paneId: string) =>
  invoke<Worktree[]>("list_worktrees", { paneId });
export const setDiffWorktree = (paneId: string, worktree: string) =>
  invoke<void>("set_diff_worktree", { paneId, worktree });
export const openSsh = (paneId: string | null, tab: boolean, host: string) =>
  invoke<string>("open_ssh", { paneId, tab, host });
export const sshHosts = () => invoke<string[]>("ssh_hosts");
export const setBrowserBounds = (
  paneId: string,
  x: number,
  y: number,
  w: number,
  h: number,
) => invoke<void>("set_browser_bounds", { paneId, x, y, w, h });
export const setBrowserVisible = (paneId: string, visible: boolean) =>
  invoke<void>("set_browser_visible", { paneId, visible });
export const browserNavigate = (paneId: string, url: string) =>
  invoke<void>("browser_navigate", { paneId, url });
export const browserHistory = (
  paneId: string,
  action: "back" | "forward" | "reload",
) => invoke<void>("browser_history", { paneId, action });
export const storeScrollback = (paneId: string, data: string) =>
  invoke<void>("store_scrollback", { paneId, data });
export const loadScrollback = (paneId: string) =>
  invoke<string | null>("load_scrollback", { paneId });
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
