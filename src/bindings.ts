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
  /** The pane shown alone, filling the tab, when one is. */
  zoomedPane: string | null;
  /** The team running in it, when it is a team's tab. */
  team: TeamRun | null;
  /** Its frame on the board; its cards are placed relative to `x`, `y`. */
  frame: BoardRect;
  /** Every pane's card, in the tab's pane order. */
  cards: CardRect[];
}

/** Tabs one at a time with their splits, or every pane on one board. */
export type View = "tabs" | "board";

/** The board point at the viewport's top-left, and screen px per unit. */
export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

export interface BoardRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A pane's card, relative to its tab's frame. */
export interface CardRect extends BoardRect {
  paneId: string;
  /** What the card's header says the pane is. */
  title: string;
}

/** A team running in a tab (`cmux_protocol::TeamRun`). */
export interface TeamRun {
  name: string;
  members: { name: string; role: string; pane: string; lead: boolean }[];
}

/** A team definition (`cmux_protocol::AgentTeam`). */
export interface AgentTeam {
  name: string;
  description: string | null;
  way: string | null;
  members: {
    role: string;
    name: string | null;
    count: number;
    lead: boolean;
  }[];
  layout: string | null;
  prompt: string | null;
  source: RoleSource;
  path: string;
  problem: string | null;
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

/** The agent program a pane runs (`cmux_protocol::Harness`). */
export type Harness =
  | "claude"
  | "codex"
  | "openCode"
  | "pi"
  | "gemini"
  | "amp"
  | "cursorAgent"
  | "copilot"
  | "aider"
  | "goose"
  | "qwen"
  | "crush"
  | "droid";

/** The name people know each harness by (`Harness::label`). */
export const HARNESS_LABEL: Record<Harness, string> = {
  claude: "Claude Code",
  codex: "Codex",
  openCode: "OpenCode",
  pi: "Pi",
  gemini: "Gemini CLI",
  amp: "Amp",
  cursorAgent: "Cursor Agent",
  copilot: "Copilot CLI",
  aider: "Aider",
  goose: "Goose",
  qwen: "Qwen Code",
  crush: "Crush",
  droid: "Droid",
};

/** What to call an agent: its name in its team, its session's name, its
 *  role, or its harness. A Codex member has no session name, and would
 *  otherwise go by its role (`omakase-reviewer`, not `reviewer`). */
export const agentLabel = (a: AgentInfo): string =>
  a.member ?? a.name ?? a.role ?? HARNESS_LABEL[a.harness];

/** A pane running a coding agent: Claude Code, Codex, OpenCode, … */
export interface AgentInfo {
  paneId: string;
  /** The tab holding it. */
  tabId: string;
  /** What it runs. Only Claude Code has hooks, and so a status. */
  harness: Harness;
  role: string | null;
  /** Its name in its tab's team, when it is a member. */
  member: string | null;
  /** A Claude Code agent whose transcript is known: activity to show. */
  hasActivity: boolean;
  /** The Claude Code session's name, as `claude agents` lists it. */
  name: string | null;
  model: string | null;
  /** Its hooks' status, or else busy/idle from Claude Code's session file. */
  status: AgentStatus | null;
  message: string | null;
  /** What its own terminal title says it is on, when that adds to the role. */
  topic: string | null;
  sinceMs: number | null;
  cwd: string | null;
  branch: string | null;
}

/** How far an agent is through its task list (`cmux_protocol::TaskProgress`). */
export interface TaskProgress {
  done: number;
  total: number;
  /** What the task in progress says it is doing. */
  current: string | null;
}

export type SubagentState = "running" | "done" | "stopped";

/** What one transcript adds up to (`cmux_protocol::TranscriptSummary`). */
export interface TranscriptSummary {
  model: string | null;
  /** Fresh tokens: input not read from the cache, plus output. */
  tokens: number;
  /** Context size at the last reply. */
  context: number;
  toolCalls: number;
  /** Null when it keeps no task list. */
  tasks: TaskProgress | null;
  startedMs: number | null;
  lastMs: number | null;
}

export interface SubagentInfo extends TranscriptSummary {
  id: string;
  description: string | null;
  agentType: string | null;
  depth: number;
  state: SubagentState;
}

/** A Claude Code agent's progress and its subagents. */
export interface AgentActivity {
  paneId: string;
  session: TranscriptSummary;
  /** Running ones first, then latest activity first. */
  subagents: SubagentInfo[];
}

export type ActionStatus = "ok" | "error" | "running" | "interrupted";

/** One tool call of an agent's trail (`cmux_protocol::TrailAction`). */
export interface TrailAction {
  id: string;
  tool: string;
  /** One line: a command, a path, a pattern, a URL, a description. */
  summary: string;
  startedMs: number | null;
  durationMs: number | null;
  status: ActionStatus;
  exitCode: number | null;
  /** The first lines of its result. */
  output: string;
}

/** One turn: a prompt and what the agent did for it. */
export interface TrailTurn {
  /** Its first line; null for actions with no prompt before them. */
  prompt: string | null;
  /** The whole prompt, up to a few thousand characters. */
  text: string | null;
  startedMs: number | null;
  lastMs: number | null;
  actions: TrailAction[];
}

/** What a pane's agent did, oldest turn first, or why there is nothing. */
export interface AgentTrail {
  paneId: string;
  turns: TrailTurn[];
  reason: string | null;
}

/** One action in full, read from the agent's own file on demand. */
export interface ActionDetail {
  id: string;
  tool: string;
  /** Its input, as pretty-printed JSON. */
  input: string;
  /** Its result; null while there is none. */
  output: string | null;
  isError: boolean;
}

/** A task-board pane, and the tab whose cards it shows. */
export interface TaskBoardPaneInfo {
  paneId: string;
  /** The tab it was opened from. */
  tabId: string | null;
}

export type TaskColumn = "todo" | "doing" | "needsYou" | "done";
export type TaskSource = "teamStart" | "agentNew" | "agentSend" | "manual";

/** A card of the task board (`cmux_protocol::TaskCard`). */
export interface TaskCard {
  id: string;
  title: string;
  source: TaskSource;
  from: string;
  assignee: string | null;
  pane: string | null;
  team: string | null;
  tab: string | null;
  column: TaskColumn;
  createdMs: number;
  updatedMs: number;
}

/** One line of the task log (`cmux_protocol::TaskHistoryEntry`): the
 *  event's fields, flattened, and the line `mira agent history` prints. */
export interface TaskHistoryEntry {
  atMs: number;
  kind: "given" | "moved" | "turnEnded" | "resumed";
  card?: string;
  /** A task given: its whole text. */
  text?: string;
  agent: string | null;
  turnMs: number | null;
  summary: string;
}

/** The history of a card, else of a tab, else of every tab; oldest first. */
export const taskHistory = (tabId: string | null, card: string | null) =>
  invoke<TaskHistoryEntry[]>("task_history", { tabId, card });

/** The cards of a tab, or of every tab when `tabId` is null; of the done
 *  ones, only the latest. */
export const taskCards = (tabId: string | null) =>
  invoke<TaskCard[]>("task_cards", { tabId });
export const openTaskBoard = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_task_board", { paneId, tab });

export interface AgentWallPaneInfo {
  paneId: string;
  /** The tab whose agents it shows: the one it was opened from. */
  tabId: string | null;
}

/** Where a role or a way of working was defined (`cmux_protocol::RoleSource`). */
export type RoleSource = "repo" | "global" | "config" | "builtin";

/** A named way to start an agent (`cmux_protocol::AgentRole`). */
export interface AgentRole {
  name: string;
  harness: Harness;
  model: string | null;
  /** How hard its model thinks: Claude Code's `--effort`, Codex's
   * `model_reasoning_effort`. */
  effort: string | null;
  /** Codex's `--profile`, from the person's own `<name>.config.toml`. */
  profile: string | null;
  prompt: string | null;
  /** The way of working it follows. */
  way: string | null;
  description: string | null;
  /** Only these MCP servers, plugins, skills; absent is everything. */
  mcp: string[] | null;
  plugins: string[] | null;
  skills: string[] | null;
  tools: { allow: string[]; deny: string[] } | null;
  source: RoleSource;
  path: string | null;
  /** Why it cannot start, when it cannot. */
  problem: string | null;
}

export interface AgentWay {
  name: string;
  prompt: string;
  description: string | null;
  source: RoleSource;
  path: string;
}

/** The roles and ways a pane's directory sees (`cmux_protocol::RoleCatalog`). */
export interface RoleCatalog {
  roles: AgentRole[];
  ways: AgentWay[];
  teams: AgentTeam[];
  problems: string[];
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
  agentEditorPanes: AgentEditorPaneInfo[];
  agentWallPanes: AgentWallPaneInfo[];
  taskBoardPanes: TaskBoardPaneInfo[];
  agents: AgentInfo[];
  view: View;
  camera: Camera;
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
  /** Only mirador.json's: a pane's roles come from `agentRoles()`. */
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
export const claudeIntegrationOffer = () =>
  invoke<boolean>("claude_integration_offer");
/** `mira` on PATH, then the hooks and `/mira-diff`. Lines of what was done. */
export const setupClaudeIntegration = () =>
  invoke<string[]>("setup_claude_integration");
/** "Not now": the offer is not made again. */
export const dismissClaudeIntegration = () =>
  invoke<void>("dismiss_claude_integration");
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
export const setView = (view: View) => invoke<void>("set_view", { view });
/**
 * Saved with the session; sends no snapshot back. With the board's size on
 * screen, it says where the board is looked at: where a new frame goes.
 */
export const setCamera = (
  camera: Camera,
  viewport: { width: number; height: number },
) => invoke<void>("set_camera", { ...camera, ...viewport });
/** The camera showing the whole board in a viewport of this size. */
export const boardFit = (width: number, height: number) =>
  invoke<Camera>("board_fit", { width, height });
/** Packs the frames into rows for a viewport of this size: the camera
 *  showing them all there. */
export const boardArrange = (width: number, height: number) =>
  invoke<Camera>("board_arrange", { width, height });
/**
 * Moves or resizes a card. Refused over another card, or growing its frame
 * into another frame: the answer is where the card is.
 */
export const setCardRect = (paneId: string, r: BoardRect) =>
  invoke<BoardRect>("set_card_rect", { paneId, x: r.x, y: r.y, w: r.w, h: r.h });
/** Moves a tab's frame; refused onto another frame. */
export const setFramePosition = (tabId: string, x: number, y: number) =>
  invoke<{ x: number; y: number }>("set_frame_position", { tabId, x, y });
/** The camera showing a pane's card centred: at 100%, or at `zoom`. */
export const boardCardCamera = (
  paneId: string,
  width: number,
  height: number,
  zoom: number | null = null,
) => invoke<Camera>("board_card_camera", { paneId, width, height, zoom });
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
/** What a role's menu entry says beside its name. */
export const roleHint = (r: AgentRole): string =>
  r.problem
    ? `can't start: ${r.problem}`
    : [
        r.harness === "claude" ? null : HARNESS_LABEL[r.harness],
        r.model ?? "default model",
        r.effort,
        r.profile ? `profile: ${r.profile}` : null,
        r.way ? `way: ${r.way}` : null,
        r.mcp || r.plugins || r.skills || r.tools ? "limited" : null,
        r.source === "repo"
          ? "repo"
          : r.source === "builtin"
            ? "built-in"
            : null,
      ]
        .filter(Boolean)
        .join(" · ");

/** Starts a team in a tab of its own; `task` goes to its lead. */
export const startTeam = (
  team: string,
  task: string | null,
  paneId: string | null,
) => invoke<string>("start_team", { team, task, paneId });

/** An agent-editor pane, and the directory whose files it edits. */
export interface AgentEditorPaneInfo {
  paneId: string;
  cwd: string | null;
}

/** What an editor pane shows (`agent_editor::EditorCatalog`). */
export interface EditorCatalog {
  catalog: RoleCatalog;
  /** Where a repository file goes; null outside a repository. */
  repoDir: string | null;
  globalDir: string;
}

export type AgentFileKind = "role" | "way" | "team";

export const openAgentEditor = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_agent_editor", { paneId, tab });
export const agentEditorCatalog = (paneId: string) =>
  invoke<EditorCatalog>("agent_editor_catalog", { paneId });
export const agentFileRead = (paneId: string, path: string) =>
  invoke<string>("agent_file_read", { paneId, path });
/** Warnings for the text; rejects with why it cannot be saved. */
export const agentFileCheck = (
  kind: AgentFileKind,
  name: string,
  text: string,
) => invoke<string[]>("agent_file_check", { kind, name, text });
export const agentFileTemplate = (kind: AgentFileKind) =>
  invoke<string>("agent_file_template", { kind });
export const agentFileSave = (
  paneId: string,
  kind: AgentFileKind,
  source: RoleSource,
  name: string,
  text: string,
  create: boolean,
) =>
  invoke<EditorCatalog>("agent_file_save", {
    paneId,
    kind,
    source,
    name,
    text,
    create,
  });
export const agentFileDelete = (paneId: string, path: string) =>
  invoke<EditorCatalog>("agent_file_delete", { paneId, path });

/** The roles `paneId`'s directory sees: its repository's, then yours. */
export const agentRoles = (paneId: string | null) =>
  invoke<RoleCatalog>("agent_roles", { paneId });
export const openAgent = (
  role: string | null,
  task: string | null,
  paneId: string | null,
  tab: boolean,
  background = false,
) => invoke<string>("open_agent", { role, task, paneId, tab, background });
/** Shows a pane alone in its tab; `false` shows the whole tab; absent toggles. */
export const zoomPane = (paneId: string, zoom: boolean | null = null) =>
  invoke<boolean>("zoom_pane", { paneId, zoom });
export const markPaneRead = (paneId: string) =>
  invoke<void>("mark_pane_read", { paneId });
/** The wall for the tab of `paneId` (the focused pane when null). */
export const openAgentWall = (paneId: string | null) =>
  invoke<string>("open_agent_wall", { paneId });
export const getAgentActivity = (paneId: string) =>
  invoke<AgentActivity>("get_agent_activity", { paneId });
export const getAgentTrail = (paneId: string) =>
  invoke<AgentTrail>("get_agent_trail", { paneId });
export const getTrailDetail = (paneId: string, actionId: string) =>
  invoke<ActionDetail>("get_trail_detail", { paneId, actionId });
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
/** The page as PNG bytes, raw: for a board card whose page is hidden. */
export const browserScreenshot = (paneId: string) =>
  invoke<ArrayBuffer>("browser_screenshot", { paneId });
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
