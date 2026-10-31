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
  /** The tab shows its agents as the wall instead of its splits. */
  wall: boolean;
  /** The side of the wall it shows, or will when the wall is next on. */
  wallView: WallView;
  /** How the Agents side lays its agents out. */
  wallMode: WallMode;
  /** How the Tasks side lays its board out. */
  taskLayout: TaskLayout;
  /** Its agents' panes by urgency: who Focus shows when none is pinned. */
  wallUrgency: string[];
  /** Its agents' panes in team order, in pages of `agentsPerTab`. */
  wallPages: string[][];
  /** Its panes that are not agents — terminals, browsers, diffs, graphs —
   *  in pane order: the wall's Tools. */
  wallTools: WallTool[];
  /** The team running in it, when it is a team's tab. */
  team: TeamRun | null;
  /** Its frame on the board; its cards are placed relative to `x`, `y`. */
  frame: BoardRect;
  /** Every pane's card, in the tab's pane order. */
  cards: CardRect[];
}

/** A pane of a tab that is not an agent but is worth a place beside them. */
export interface WallTool {
  paneId: string;
  kind: ToolKind;
  /** What the pane is, in a few words, as its card on the board says. */
  title: string;
}

/** A terminal is a shell, a command pane or an SSH pane. */
export type ToolKind = "terminal" | "browser" | "diff" | "graph";

/** Tabs one at a time with their splits, or every pane on one board. */
export type View = "tabs" | "board";

/** The side of a tab's wall: its agents' terminals, or its task board. */
export type WallView = "agents" | "tasks";

/** The Agents side: one agent large with a strip beside it, or a grid. */
export type WallMode = "focus" | "grid";

/** The Tasks side: a column per status, or a row per agent. */
export type TaskLayout = "columns" | "swimlanes";

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

/** A tool call an agent asks permission for, summed up as in its trail. */
export interface Asking {
  tool: string;
  summary: string;
}

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

/** The agents of tab `tabId`, in the snapshot's order: the sidebar's pips
 *  for it, and its frame's on the board. */
export const tabAgents = (s: WorkspaceSnapshot, tabId: string): AgentInfo[] =>
  s.agents.filter((a) => a.tabId === tabId);

/** The agents waiting on you — the Inbox's cards and the count on its
 *  button — tabs in sidebar order, each tab's in its wall's order. */
export const waitingAgents = (s: WorkspaceSnapshot): AgentInfo[] =>
  s.tabs.flatMap((t) =>
    s.agents
      .filter((a) => a.tabId === t.id && a.status === "needsYou")
      .sort((a, b) => t.wallUrgency.indexOf(a.paneId) - t.wallUrgency.indexOf(b.paneId)),
  );

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
  /** A Claude Code agent with a known transcript, or a Codex one with a
   *  known rollout: `getAgentActivity` has something to read. */
  hasActivity: boolean;
  /** The Claude Code session's name, as `claude agents` lists it. */
  name: string | null;
  model: string | null;
  /** Its role's effort ("high", "max"); null without one. */
  effort: string | null;
  /** Its hooks' status, or else busy/idle from Claude Code's session file. */
  status: AgentStatus | null;
  message: string | null;
  /** The tool call a Claude Code agent waits on a permission for. */
  asking: Asking | null;
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
  /** How large the context can grow, when known: a guess of 1M for every
   *  Claude model for now. */
  contextWindow: number | null;
  toolCalls: number;
  /** What its replies cost at API list prices, each at its own model's
   *  rates: on a Claude subscription, what it would have cost, not what is
   *  billed. Null when no reply could be priced (Codex, always). */
  costUsd: number | null;
  /** Some replies had no known price and are not in `costUsd`. */
  unpriced: boolean;
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
  /** What sort of thing it did. */
  kind: ActionKind;
}

/** What a trail action did (`cmux_protocol::ActionKind`). */
export type ActionKind = "read" | "edit" | "run" | "other";

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
  /** Turns before the first one sent. */
  older: number;
  /** Turns left out between the first — a card's, older than the latest —
   *  and the latest; 0 when they follow on. */
  gap: number;
  /** What it is a reading of: the same as the last answer's, the same answer. */
  version: string | null;
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

export type TaskColumn = "todo" | "doing" | "needsYou" | "done";
export type TaskSource = "teamStart" | "agentNew" | "agentSend" | "manual";

/** A card of the task board (`cmux_protocol::TaskCard`). */
export interface TaskCard {
  id: string;
  title: string;
  /** The whole task when it says more than the title; a card written by
   *  hand's own description. */
  description: string | null;
  source: TaskSource;
  from: string;
  assignee: string | null;
  pane: string | null;
  team: string | null;
  tab: string | null;
  column: TaskColumn;
  createdMs: number;
  updatedMs: number;
  /** In Doing: the failed actions in its agent's latest turn. */
  failing: number;
}

/** One line of the task log (`cmux_protocol::TaskHistoryEntry`): the
 *  event's fields, flattened, and the line `mira agent history` prints. */
export interface TaskHistoryEntry {
  atMs: number;
  kind: "given" | "moved" | "turnEnded" | "resumed";
  card?: string;
  /** A task given: its whole text. */
  text?: string;
  /** A task given: who gave it (`you` for a person), and to whom. */
  from?: string;
  to?: string | null;
  /** A task given: its card's title when one was given, and where its
   *  agent was: the tab's id. */
  title?: string;
  tab?: string | null;
  /** A task given, a turn ended or resumed: the agent's pane. */
  pane?: string | null;
  /** A move: the column, and who made it. */
  column?: TaskColumn;
  by?: string;
  /** A turn end: how it ended. */
  end?: "done" | "needsYou" | "exited";
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
/** Writes a card in a tab, as `mira task add`; its id. */
export const taskAdd = (
  tabId: string,
  title: string,
  description: string | null,
  assignee: string | null,
) => invoke<string>("task_add", { tabId, title, description, assignee });
/** Sends a card written by hand to an agent and puts it in Doing. */
export const taskSend = (card: string, paneId: string) =>
  invoke<string>("task_send", { card, paneId });
/** Sends an agent its own first To do card, else its tab's first unassigned. */
export const sendNextTask = (paneId: string) =>
  invoke<string>("send_next_task", { paneId });

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
  /** Files hidden by one of the same name that comes first. */
  shadowed: ShadowedFile[];
}

/** What the catalog holds (`cmux_protocol::CatalogKind`). */
export type CatalogKind = "role" | "way" | "team";

/** A role, way or team hidden by one of the same name — repo, then yours,
 *  then mirador.json, then built-in — and so not used. */
export interface ShadowedFile {
  kind: CatalogKind;
  name: string;
  source: RoleSource;
  /** Its file; null for an `agentRoles` entry. */
  path: string | null;
  /** Where the one used instead is defined, and its file. */
  by: RoleSource;
  byPath: string | null;
}

/** One part of what a team member is told, in order (`cmux_protocol::PromptLayer`). */
export interface PromptLayer {
  kind: "role" | "roleWay" | "team" | "charter" | "teamWay";
  /** The file it comes from, or "written by Mirador". */
  source: string;
  text: string;
}

/** A team or role using a role or a way of working, and whether it runs. */
export interface UsedBy {
  kind: CatalogKind;
  name: string;
  running: boolean;
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
  settingsPanes: SettingsPaneInfo[];
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
  /** The app's colors from the same theme: one per `:root` token. */
  ui: UiColors;
}

/** The app's colors (`UiColors`), each `#rrggbb`: `bgAlt` is `--bg-alt`. */
export interface UiColors {
  bg: string;
  bgAlt: string;
  surface: string;
  text: string;
  subtext: string;
  muted: string;
  accent: string;
  overlay: string;
  statusWorking: string;
  statusNeedsYou: string;
  statusIdle: string;
  danger: string;
  warn: string;
  merged: string;
  tintNeedsYouRow: string;
  tintNeedsYouHead: string;
  tintNeedsYouArea: string;
  tintNeedsYouPanel: string;
  tintHover: string;
  tintInfo: string;
  tintWarn: string;
  tintErrorRow: string;
  /** Ink on a needs-you fill. */
  onNeedsYou: string;
  /** Eight distinct hues for the graph's lanes and subagent types. */
  categorical: string[];
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
  /** How many agents a page of the wall's Grid layout holds: 4, 6 or 9. */
  agentsPerTab: number;
  agentClick: AgentClick;
  /** Where the app opens the editor, a diff and the commit graph. */
  editorOpensIn: OpensIn;
  diffOpensIn: OpensIn;
  graphOpensIn: OpensIn;
}

/** A sidebar agent's click: its tab's wall on it, or its pane alone. */
export type AgentClick = "wall" | "pane";

/** A pane the app opens: in a tab of its own, or split beside the
 *  focused one (`cmux_protocol::OpensIn`). */
export type OpensIn = "tab" | "split";

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

/** What a team tells its members, in estimated tokens, and the least that
 *  costs per turn (`cmux_protocol::TeamWeight`). A lower bound: the
 *  harness's own prompt and tools, CLAUDE.md, MCP tool schemas, the
 *  conversation and all output are not counted. */
export interface TeamWeight {
  team: string;
  seats: SeatWeight[];
  tokens: number;
  /** The priced seats' floors, summed. */
  perTurnFloorUsd: number;
  /** The priced seats' start costs, summed. */
  startUsd: number;
  /** Seats whose model has no known price, left out of the floor and the
   *  start cost. */
  unpriced: number;
  /** When the prices were checked, as YYYY-MM-DD. */
  pricesChecked: string;
}

/** One member's share of a team's weight. */
export interface SeatWeight {
  name: string;
  role: string;
  harness: Harness;
  /** As the role says it; null for the harness's default. */
  model: string | null;
  effort: string | null;
  layers: LayerWeight[];
  tokens: number;
  /** The model's cache-read price, USD per million tokens, the floor is
   *  worked out at; null with no known price. */
  cacheReadPerMillion: number | null;
  /** Those tokens read from the prompt cache each turn; null with no
   *  known price. */
  perTurnFloorUsd: number | null;
  /** Those tokens written to the prompt cache, by the first turn; null
   *  with no known price. */
  startUsd: number | null;
}

export interface LayerWeight {
  kind: PromptLayer["kind"];
  source: string;
  tokens: number;
}

/** The weight of `team`, the one `paneId`'s directory sees. */
export const teamCost = (team: string, paneId: string | null) =>
  invoke<TeamWeight>("team_cost", { team, paneId });

/** The settings pane: at most one, showing mirador.json's settings. */
export interface SettingsPaneInfo {
  paneId: string;
  /** The mirador.json it shows. */
  path: string;
}

/** What a setting holds, and so how it can be edited. */
/** One value a choice offers; an unavailable one does nothing here, and
 *  its note says why. */
export interface ChoiceOption {
  value: string | number;
  available: boolean;
  note: string | null;
}

export type SettingKind =
  | { kind: "choice"; options: ChoiceOption[] }
  | { kind: "bool" }
  | { kind: "number"; min: number; max: number }
  | { kind: "text" }
  | { kind: "readOnly" };

/** A mirador.json key, and what the file sets it to (`SettingRow`). */
export interface SettingRow {
  key: string;
  group: string;
  kind: SettingKind;
  /** The value in effect when the file does not set it, for a person. */
  default: string;
  description: string;
  /** Whether the file has the key at all, even as null. */
  set: boolean;
  /** The value in the file as written; null when it sets none (or null). */
  value: unknown;
  /** Whether that value is one the key takes. */
  valid: boolean;
}

/** Shows the settings pane, or opens one beside `paneId` or in a tab. */
export const openSettings = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_settings", { paneId, tab });
/** Every setting, or why mirador.json cannot be read. */
export const settingsView = () => invoke<SettingRow[]>("settings_view");
/** Sets `key` in mirador.json, or removes it with null. */
export const setConfigValue = (key: string, value: unknown) =>
  invoke<void>("set_config_value", { key, value });

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
  /** What the role form's pickers offer, per harness a role can start. */
  choices: HarnessChoices[];
}

export type AgentFileKind = "role" | "way" | "team";

/** Models and efforts the launch passes on for a harness; a file's own
 *  value is offered besides, so one not listed still round-trips. */
export interface HarnessChoices {
  harness: Harness;
  models: string[];
  efforts: string[];
}

/** The team form's fields (`cmux_protocol::TeamFields`). */
export interface TeamFields {
  description: string | null;
  way: string | null;
  layout: string | null;
  members: AgentTeam["members"];
  /** The body. */
  charter: string | null;
}

/** The role form's fields (`cmux_protocol::RoleFields`). */
export interface RoleFields {
  harness: Harness;
  model: string | null;
  effort: string | null;
  way: string | null;
  description: string | null;
  /** `tools.deny`. */
  deny: string[];
  /** The body. */
  prompt: string | null;
}

/** The way page's fields (`cmux_protocol::WayFields`). */
export interface WayFields {
  description: string | null;
  /** The body. */
  prompt: string;
}

export type FileFields =
  | ({ kind: "team" } & TeamFields)
  | ({ kind: "role" } & RoleFields)
  | ({ kind: "way" } & WayFields);

export const openAgentEditor = (paneId: string | null, tab: boolean) =>
  invoke<string>("open_agent_editor", { paneId, tab });
export const agentEditorCatalog = (paneId: string) =>
  invoke<EditorCatalog>("agent_editor_catalog", { paneId });
/** The teams and roles using the role or way `name`, running or not. */
export const agentEditorUsedBy = (paneId: string, kind: CatalogKind, name: string) =>
  invoke<UsedBy[]>("agent_editor_used_by", { paneId, kind, name });
/** What `member` of `team` is told when the team starts, layer by layer,
 *  and the prompt they join into, as the launch joins them. */
export const agentEditorTold = (paneId: string, team: string, member: string) =>
  invoke<{ layers: PromptLayer[]; prompt: string | null }>("agent_editor_told", { paneId, team, member });
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
/** Saves a form's fields into `name` in `source`'s place, applied to the
 *  file at `from` (a built-in's `builtin:` name saves a new file of yours
 *  or the repository's, with `create`), or to nothing for a new one. */
export const agentFileSaveFields = (
  paneId: string,
  source: RoleSource,
  name: string,
  from: string | null,
  fields: FileFields,
  create: boolean,
) =>
  invoke<EditorCatalog>("agent_file_save_fields", { paneId, source, name, from, fields, create });
/** What `team restart` did for a member: "restarted", "reopened", or why
 *  not ("running", "busy"). */
export interface Restarted {
  name: string;
  pane: string;
  outcome: string;
}
/** Starts again the members of `paneId`'s team (`members` by name, all
 *  when empty) whose agent exited or whose pane was closed; a running
 *  member is left alone. */
export const teamRestart = (members: string[], paneId: string | null) =>
  invoke<Restarted[]>("team_restart", { members, paneId });
export const agentFileDelete = (paneId: string, path: string) =>
  invoke<EditorCatalog>("agent_file_delete", { paneId, path });

/** The roles `paneId`'s directory sees: its repository's, then yours. */
export const agentRoles = (paneId: string | null) =>
  invoke<RoleCatalog>("agent_roles", { paneId });
/** `harness`: without a role, Claude Code (default) or Codex. */
export const openAgent = (
  role: string | null,
  task: string | null,
  paneId: string | null,
  tab: boolean,
  harness: Harness | null = null,
) => invoke<string>("open_agent", { role, task, paneId, tab, harness });
/** Shows a pane alone in its tab; `false` shows the whole tab; absent toggles. */
export const zoomPane = (paneId: string, zoom: boolean | null = null) =>
  invoke<boolean>("zoom_pane", { paneId, zoom });
export const markPaneRead = (paneId: string) =>
  invoke<void>("mark_pane_read", { paneId });
/** The wall for the tab of `paneId` (the focused pane when null). */
export const toggleAgentWall = (paneId: string | null) =>
  invoke<boolean>("toggle_agent_wall", { paneId });
export const setWallView = (paneId: string | null, view: WallView) =>
  invoke<void>("set_wall_view", { paneId, view });
/** Lays out a side of the wall of `paneId`'s tab; neither turns it on. */
export const setWallMode = (paneId: string | null, mode: WallMode) =>
  invoke<void>("set_wall_mode", { paneId, mode });
export const setTaskLayout = (paneId: string | null, layout: TaskLayout) =>
  invoke<void>("set_task_layout", { paneId, layout });
/** Writes `agentsPerTab` (4, 6 or 9) to mirador.json. */
export const setAgentsPerTab = (n: number) => invoke<void>("set_agents_per_tab", { n });
export const getAgentActivity = (paneId: string) =>
  invoke<AgentActivity>("get_agent_activity", { paneId });
/** Its last `turns` turns, and any begun at or after `sinceMs`. */
export const getAgentTrail = (paneId: string, turns: number | null, sinceMs: number | null) =>
  invoke<AgentTrail>("get_agent_trail", { paneId, turns, sinceMs });
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
