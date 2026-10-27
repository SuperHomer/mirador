import { useEffect, useReducer, useRef, useState } from "react";
import { AgentInfo, agentLabel, focusPane, markPaneRead, TabSnapshot } from "../bindings";
import { useUiStore, WallFilter } from "../state/uiStore";
import { getTerminal } from "../terminal/registry";
import { useAgentActivity } from "./ActivityPanel";
import { AgentStatusBar, fullness } from "./AgentStatusBar";
import { STATUS_LABEL, StatusMark } from "./StatusMark";
import { SubagentSheet } from "./SubagentSheet";
import { elapsed } from "./time";
import { AskingLine } from "./AskingLine";
import { useLentTerminal } from "./useLentTerminal";
import { SendNextTask } from "./SendNextTask";

/** How long after the last keystroke into the focused agent Focus may
 *  move to another: never out from under someone typing. */
const TYPING_GRACE_MS = 2000;

/**
 * The agent Focus shows large: the pinned one while it is in the tab, else
 * the most urgent (`wallUrgency`). A pin or an unpin applies at once; a
 * change in urgency waits until nobody has typed into the focused agent
 * for `TYPING_GRACE_MS`.
 */
function useFocusedAgent(tab: TabSnapshot, agents: AgentInfo[]): AgentInfo | undefined {
  const pinnedPane = useUiStore((s) => s.pinned[tab.id]);
  const pin = agents.some((a) => a.paneId === pinnedPane) ? pinnedPane : null;
  const candidate =
    pin ?? tab.wallUrgency.find((p) => agents.some((a) => a.paneId === p)) ?? agents[0]?.paneId ?? null;
  const [focused, setFocused] = useState(candidate);
  const present = agents.some((a) => a.paneId === focused);
  const typedAt = useRef(0);
  const lastPin = useRef(pin);
  const [tick, recheck] = useReducer((n: number) => n + 1, 0);

  useEffect(() => {
    if (!focused) return;
    const term = getTerminal(focused);
    if (!term) return;
    const typing = term.onData(() => {
      typedAt.current = Date.now();
    });
    return () => typing.dispose();
  }, [focused]);

  useEffect(() => {
    const pinChanged = lastPin.current !== pin;
    lastPin.current = pin;
    if (candidate === focused) return;
    if (pinChanged || !present) {
      setFocused(candidate);
      return;
    }
    const wait = typedAt.current + TYPING_GRACE_MS - Date.now();
    if (wait <= 0) {
      setFocused(candidate);
      return;
    }
    const timer = window.setTimeout(recheck, wait);
    return () => window.clearTimeout(timer);
  }, [candidate, focused, pin, present, tick]);

  return agents.find((a) => a.paneId === focused);
}

/**
 * Focus (18a): one agent large, with its terminal live and full size, and
 * the others down a rail by status — those that need you, then those at
 * work, as live thumbnails, then the idle as one line each. The chip
 * filter narrows the rail, never the agent shown large.
 */
export function FocusLayout({
  tab,
  agents,
  filter,
  unread,
  visible,
  current,
  keyboard,
  sendable,
  now,
}: {
  tab: TabSnapshot;
  agents: AgentInfo[];
  filter: WallFilter;
  unread: string[];
  visible: boolean;
  current: boolean;
  /** The wall is on screen in the active tab: the keyboard is its. */
  keyboard: boolean;
  /** The agents with a next task to be sent. */
  sendable: Set<string>;
  now: number;
}) {
  const focused = useFocusedAgent(tab, agents);
  const pin = (pane: string) => useUiStore.getState().pinAgent(tab.id, pane);
  const layoutRef = useRef<HTMLDivElement>(null);

  // The keyboard goes to the agent shown large as the wall comes on screen,
  // and follows it when it changes — after a pin, or while it was in the
  // agent that was large, now a thumbnail that takes no input. The tile's
  // own effect has lent its terminal by now.
  const focusedPane = focused?.paneId;
  const shownBefore = useRef<string | undefined>(undefined);
  // Read while rendering, before the commit moves the terminals: moving a
  // focused element out of the page blurs it, so by the effect the agent
  // that was large has lost the keyboard either way.
  const held = useRef(false);
  if (shownBefore.current !== focusedPane) {
    const before = shownBefore.current ? getTerminal(shownBefore.current)?.element : undefined;
    held.current = !!before?.contains(document.activeElement);
  }
  useEffect(() => {
    const before = shownBefore.current;
    shownBefore.current = focusedPane;
    if (!keyboard || !focusedPane) return;
    const layout = layoutRef.current;
    const pinned = useUiStore.getState().pinned[tab.id] === focusedPane;
    if (before && before !== focusedPane && !held.current && !pinned) return;
    const term = getTerminal(focusedPane);
    if (term?.element && layout?.contains(term.element)) term.focus();
  }, [keyboard, focusedPane]);

  const others = agents.filter(
    (a) => a.paneId !== focusedPane && (filter === "all" || a.status === filter),
  );
  const asking = others.filter((a) => a.status === "needsYou");
  // Agents with no known status are with the working: there is no telling.
  const working = others.filter((a) => a.status === "working" || !a.status);
  const idle = others.filter((a) => a.status === "idle");
  const waiting = tab.wallUrgency
    .filter((p) => p !== focusedPane)
    .map((p) => agents.find((a) => a.paneId === p))
    .filter((a): a is AgentInfo => a?.status === "needsYou");

  return (
    <div className="focus-layout" ref={layoutRef}>
      {focused ? (
        <FocusedTile
          key={focused.paneId}
          agent={focused}
          waiting={waiting}
          onPin={pin}
          unread={unread.includes(focused.paneId)}
          visible={visible}
          current={current}
          canSend={sendable.has(focused.paneId)}
          now={now}
        />
      ) : (
        <div />
      )}
      <div className="focus-rail">
        {others.length === 0 && (
          <div className="rail-empty">
            {agents.length > 1 ? "No other agents match this filter." : "No other agents in this tab."}
          </div>
        )}
        {asking.length > 0 && (
          <section className="rail-section needs-you">
            <div className="rail-label">Needs you · {asking.length}</div>
            {asking.map((a) => (
              <MiniTile key={a.paneId} agent={a} visible={visible} now={now} onPick={pin} />
            ))}
          </section>
        )}
        {working.length > 0 && (
          <section className="rail-section working">
            <div className="rail-label">Working · {working.length}</div>
            <div className={`rail-grid${working.length >= 2 ? " two" : ""}`}>
              {working.map((a) => (
                <MiniTile key={a.paneId} agent={a} visible={visible} now={now} onPick={pin} />
              ))}
            </div>
          </section>
        )}
        {idle.length > 0 && (
          <section className="rail-section">
            <div className="rail-label">Idle · {idle.length}</div>
            {idle.map((a) => (
              <IdleLine key={a.paneId} agent={a} canSend={sendable.has(a.paneId)} now={now} onPick={pin} />
            ))}
          </section>
        )}
      </div>
    </div>
  );
}

/** The agent shown large: its live terminal at full size, what it is on,
 *  and its status bar. */
function FocusedTile({
  agent,
  waiting,
  onPin,
  unread,
  visible,
  current,
  canSend,
  now,
}: {
  agent: AgentInfo;
  /** It has a next task to be sent. */
  canSend: boolean;
  /** The others that need you, most urgent first. */
  waiting: AgentInfo[];
  onPin: (pane: string) => void;
  unread: boolean;
  visible: boolean;
  current: boolean;
  now: number;
}) {
  const pane = agent.paneId;
  const { hostRef, shown } = useLentTerminal(pane, visible, { fit: true });
  const [sheetOpen, setSheetOpen] = useState(false);
  const { activity } = useAgentActivity(pane, visible && agent.hasActivity);
  const status = agent.status;
  const task = agent.topic ?? activity?.session.tasks?.current ?? null;
  return (
    <div className={`focus-tile${status ? ` ${status}` : ""}`}>
      <div className="focus-head">
        <div className="focus-line">
          <StatusMark status={status} />
          <span className="focus-name">{agentLabel(agent)}</span>
          {status && (
            <span className={`focus-status ${status}`}>
              {STATUS_LABEL[status]}
              {agent.sinceMs ? ` · ${elapsed(agent.sinceMs, now)}` : ""}
            </span>
          )}
          {task && <span className="focus-task">{task}</span>}
          {waiting.length > 0 && (
            <button
              className="focus-waiting"
              title="Show the next agent that needs you"
              onClick={() => onPin(waiting[0].paneId)}
            >
              {waiting.length} more waiting: {agentLabel(waiting[0])} ›
            </button>
          )}
        </div>
        {status === "needsYou" && <AskingLine agent={agent} className="focus-message" />}
        {status === "idle" && (agent.sinceMs || canSend) && (
          <div className="focus-note">
            {agent.sinceMs ? `Idle. Its last turn ended ${elapsed(agent.sinceMs, now)} ago. ` : "Idle. "}
            {canSend && <SendNextTask pane={agent.paneId} label="Send next task…" />}
          </div>
        )}
      </div>
      <div className="focus-body">
        <div
          className="focus-term"
          data-pane={pane}
          onMouseDown={() => {
            if (unread) void markPaneRead(pane);
            // On the board, a wall in another tab's frame: its tab becomes
            // the active one, so the keyboard stays in the tile pressed.
            if (!current) void focusPane(pane);
          }}
        >
          {/* The terminal's element is appended here by `lendTerminal`, so
              React must own nothing inside it. */}
          <div className="focus-term-host" ref={hostRef} />
          {!shown && <span className="wall-term-missing">shown elsewhere</span>}
        </div>
        {sheetOpen && (
          <SubagentSheet activity={activity} now={now} onClose={() => setSheetOpen(false)} />
        )}
      </div>
      <AgentStatusBar
        agent={agent}
        activity={activity}
        now={now}
        sheetOpen={sheetOpen}
        onToggleSheet={() => setSheetOpen(!sheetOpen)}
      />
    </div>
  );
}

/** An agent on the rail: its live terminal, drawn smaller and cropped to
 *  its latest lines, and what it is on. Clicking it pins it large. */
function MiniTile({
  agent,
  visible,
  now,
  onPick,
}: {
  agent: AgentInfo;
  visible: boolean;
  now: number;
  onPick: (pane: string) => void;
}) {
  const pane = agent.paneId;
  const { hostRef, shown } = useLentTerminal(pane, visible, { fit: false });
  const { activity } = useAgentActivity(pane, visible && agent.hasActivity);
  const session = activity?.session;
  const pct =
    session?.contextWindow ? Math.min(100, Math.round((session.context / session.contextWindow) * 100)) : null;
  const task = agent.topic ?? session?.tasks?.current ?? null;
  const status = agent.status;
  return (
    <div
      className={`mini-tile${status ? ` ${status}` : ""}`}
      title={`Show ${agentLabel(agent)} large`}
      onClick={() => onPick(pane)}
    >
      <div className="mini-head">
        <StatusMark status={status} small />
        <span className="mini-name">{agentLabel(agent)}</span>
        {status && agent.sinceMs && (
          <span className={`mini-elapsed ${status}`}>{elapsed(agent.sinceMs, now)}</span>
        )}
      </div>
      <div className="mini-body">
        {/* The tile's own wrapper takes the scale; the terminal's element,
            appended into it by `lendTerminal`, is left untouched. */}
        <div className="mini-host" ref={hostRef} />
        {!shown && <span className="wall-term-missing">shown elsewhere</span>}
      </div>
      <div className="mini-foot">
        {pct !== null && (
          <>
            <span className={`agent-bar-meter mini-meter ${fullness(pct)}`}>
              <span style={{ "--fill": `${pct}%` } as React.CSSProperties} />
            </span>
            <span className={`mini-pct ${fullness(pct)}`}>{pct}%</span>
          </>
        )}
        {/* Waiting, what it asks for takes the task's place: the tile's
            110px have no room for another line. */}
        {status === "needsYou" ? (
          <AskingLine agent={agent} className="mini-task" />
        ) : (
          task && <span className="mini-task">{task}</span>
        )}
      </div>
    </div>
  );
}

/** An idle agent on the rail, in one line. Clicking it pins it large. */
function IdleLine({
  agent,
  canSend,
  now,
  onPick,
}: {
  agent: AgentInfo;
  canSend: boolean;
  now: number;
  onPick: (pane: string) => void;
}) {
  return (
    <div className="idle-line" title={`Show ${agentLabel(agent)} large`} onClick={() => onPick(agent.paneId)}>
      <StatusMark status="idle" small />
      <span className="idle-name">{agentLabel(agent)}</span>
      <span className="idle-task">{agent.topic ?? ""}</span>
      {canSend && <SendNextTask pane={agent.paneId} label="Send task" />}
      {agent.sinceMs && <span className="idle-elapsed">{elapsed(agent.sinceMs, now)}</span>}
    </div>
  );
}
