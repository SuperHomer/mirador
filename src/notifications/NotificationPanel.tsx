import { useEffect, useRef, useState } from "react";
import {
  AgentInfo,
  Node,
  NotificationDto,
  TabSnapshot,
  TaskHistoryEntry,
  WorkspaceSnapshot,
  agentLabel,
  focusPane,
  listNotifications,
  markAllNotificationsRead,
  taskHistory,
  waitingAgents,
} from "../bindings";
import { useNotificationStore } from "../state/notificationStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { StatusMark } from "../agents/StatusMark";
import { AskingLine } from "../agents/AskingLine";
import { elapsed, useNow } from "../agents/time";
import { showOnWall } from "../agents/showOnWall";
import { COLUMN_NAME, age, took, useLogRead } from "../agents/TaskBoard";

/** How many updates the Inbox lists, newest first. */
const MAX_UPDATES = 50;

const NO_HISTORY: TaskHistoryEntry[] = [];

function paneIds(node: Node): string[] {
  if (node.type === "leaf") return [node.paneId];
  return node.children.flatMap(paneIds);
}

/** One row of UPDATES: a notification, a turn ended or a card moved. */
interface Update {
  key: string;
  atMs: number;
  kind: "done" | "exited" | "moved" | "notification";
  who: string;
  what: string;
  detail: string;
  tabId: string | null;
  /** The pane a click goes to; none when it is gone. */
  pane: string | null;
}

const ICON: Record<Update["kind"], string> = {
  done: "✓",
  exited: "✓",
  moved: "→",
  notification: "↯",
};

/**
 * What a person catches up on, newest first: the notifications, and from
 * the task log the turns that ended and the cards moved. Tasks given
 * between agents are History's, and anything waiting on a person is
 * WAITING ON YOU's, so neither is here.
 */
function updates(
  snapshot: WorkspaceSnapshot,
  tabOfPane: Map<string, TabSnapshot>,
  notifications: NotificationDto[],
  log: TaskHistoryEntry[],
): Update[] {
  const tabTitle = (id: string | null | undefined) => snapshot.tabs.find((t) => t.id === id)?.title ?? null;
  // A pane is worth going to while it exists, its agent exited or not.
  const existing = (pane: string | null | undefined) => (pane && tabOfPane.has(pane) ? pane : null);
  const out: Update[] = [];
  for (const n of notifications) {
    const tab = tabOfPane.get(n.paneId) ?? null;
    // An agent's title names its tab already (`reviewer · api`).
    const agent = snapshot.agents.some((a) => a.paneId === n.paneId);
    out.push({
      key: n.id,
      atMs: n.atMs,
      kind: "notification",
      who: n.title ?? "Notification",
      what: "",
      detail: [agent ? null : tab?.title, n.body].filter(Boolean).join(" · "),
      tabId: tab?.id ?? null,
      pane: existing(n.paneId),
    });
  }
  // Each card's title and place, and each pane's latest card, from the
  // tasks given before them: the log is oldest first.
  const cards = new Map<string, { title: string; tab: string | null; pane: string | null }>();
  const cardOfPane = new Map<string, string>();
  log.forEach((e, i) => {
    const key = `log-${e.atMs}-${i}`;
    if (e.kind === "given" && e.card) {
      const title = e.title ?? (e.text ?? "").split("\n").find((l) => l.trim())?.trim() ?? "";
      cards.set(e.card, { title, tab: e.tab ?? null, pane: e.pane ?? null });
      if (e.pane) cardOfPane.set(e.pane, title);
    } else if (e.kind === "moved" && e.card && e.column && e.column !== "needsYou") {
      const card = cards.get(e.card);
      out.push({
        key,
        atMs: e.atMs,
        kind: "moved",
        who: e.by ?? "someone",
        what: `moved ${card?.title || "a card"} to ${COLUMN_NAME[e.column]}`,
        detail: tabTitle(card?.tab) ?? "",
        tabId: card?.tab ?? null,
        pane: existing(card?.pane),
      });
    } else if (e.kind === "turnEnded" && e.pane && e.end !== "needsYou") {
      const tab = tabOfPane.get(e.pane) ?? null;
      const agent = snapshot.agents.find((a) => a.paneId === e.pane);
      const what =
        e.end === "exited"
          ? "exited"
          : e.turnMs === null
            ? "finished its turn"
            : `finished its turn after ${took(e.turnMs)}`;
      out.push({
        key,
        atMs: e.atMs,
        kind: e.end === "exited" ? "exited" : "done",
        who: e.agent ?? (agent ? agentLabel(agent) : "An agent"),
        what,
        detail: [tab?.title, cardOfPane.get(e.pane)].filter(Boolean).join(" · "),
        tabId: tab?.id ?? null,
        pane: existing(e.pane),
      });
    }
  });
  return out.sort((a, b) => b.atMs - a.atMs);
}

/** The Inbox: the agents waiting on you, then what happened since you
 *  last looked. */
export function NotificationPanel() {
  const open = useUiStore((s) => s.notificationsOpen);
  const close = useUiStore((s) => s.closeNotifications);
  const notifications = useNotificationStore((s) => s.notifications);
  const setAll = useNotificationStore((s) => s.setAll);
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const clearedMs = useUiStore((s) => s.inboxClearedMs);
  const clearInbox = useUiStore((s) => s.clearInbox);
  const log = useLogRead(open, "*", () => taskHistory(null, null), NO_HISTORY);
  const [tabFilter, setTabFilter] = useState<string | null>(null);
  const now = useNow(open);
  const panelRef = useRef<HTMLDivElement>(null);

  // Refresh read-flags whenever the panel opens.
  useEffect(() => {
    if (open) void listNotifications().then(setAll);
  }, [open, setAll]);

  // Esc closes it when it has focus (it takes it on opening) or nothing
  // does: with a terminal focused, Esc is the agent's, as for the
  // activity panel.
  useEffect(() => {
    if (!open) return;
    panelRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const focused = document.activeElement;
      if (focused === document.body || panelRef.current?.contains(focused)) {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  if (!open || !snapshot) return null;

  const tabOfPane = new Map<string, TabSnapshot>();
  for (const t of snapshot.tabs) for (const p of paneIds(t.root)) tabOfPane.set(p, t);

  const waiting = waitingAgents(snapshot);
  const all = updates(snapshot, tabOfPane, notifications, log).filter(
    (u) => clearedMs === null || u.atMs > clearedMs,
  );
  // A chip for each tab with someone waiting or an update.
  const chipTabs = snapshot.tabs.filter(
    (t) => waiting.some((a) => a.tabId === t.id) || all.some((u) => u.tabId === t.id),
  );
  const filter = chipTabs.some((t) => t.id === tabFilter) ? tabFilter : null;
  const shownWaiting = waiting.filter((a) => !filter || a.tabId === filter);
  const shown = all.filter((u) => !filter || u.tabId === filter).slice(0, MAX_UPDATES);

  return (
    <div className="inbox" ref={panelRef} tabIndex={-1}>
      <div className="inbox-header">
        <span>
          Inbox {waiting.length > 0 && <span className="inbox-count">{waiting.length}</span>}
        </span>
        <button className="inbox-close" onClick={close} title="Close (Esc)">
          ×
        </button>
      </div>
      <div className="inbox-body">
        {chipTabs.length > 0 && (
          <div className="inbox-chips">
            <button className={`inbox-chip${filter === null ? " selected" : ""}`} onClick={() => setTabFilter(null)}>
              All tabs
            </button>
            {chipTabs.map((t) => (
              <button
                key={t.id}
                className={`inbox-chip${filter === t.id ? " selected" : ""}`}
                onClick={() => setTabFilter(t.id)}
              >
                {t.title}
              </button>
            ))}
          </div>
        )}
        <div className="inbox-section waiting">WAITING ON YOU · {shownWaiting.length}</div>
        {shownWaiting.length === 0 ? (
          <div className="inbox-nobody">Nobody is waiting on you.</div>
        ) : (
          <div className="inbox-cards">
            {shownWaiting.map((agent) => (
              <WaitingCard
                key={agent.paneId}
                agent={agent}
                tab={snapshot.tabs.find((t) => t.id === agent.tabId)?.title ?? ""}
                now={now}
                onOpen={() => {
                  void focusPane(agent.paneId);
                  showOnWall(agent.tabId, agent.paneId);
                  close();
                }}
              />
            ))}
          </div>
        )}
        <div className="inbox-section">
          UPDATES
          {all.length > 0 && (
            <button
              className="inbox-section-action"
              onClick={() => {
                void markAllNotificationsRead();
                void listNotifications().then(setAll);
                clearInbox();
              }}
            >
              Clear
            </button>
          )}
        </div>
        {shown.length === 0 ? (
          <div className="inbox-nobody">No updates.</div>
        ) : (
          <div className="inbox-updates">
            {shown.map((u) => (
              <UpdateRow
                key={u.key}
                update={u}
                now={now}
                onOpen={
                  u.pane
                    ? () => {
                        void focusPane(u.pane!);
                        close();
                      }
                    : null
                }
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function WaitingCard({
  agent,
  tab,
  now,
  onOpen,
}: {
  agent: AgentInfo;
  tab: string;
  now: number;
  onOpen: () => void;
}) {
  return (
    <div className="inbox-card">
      <div className="inbox-card-head">
        <StatusMark status={agent.status} small />
        <b className="inbox-card-name">{agentLabel(agent)}</b>
        <span className="inbox-card-tab">{tab}</span>
        {agent.sinceMs !== null && <span className="inbox-card-since">{elapsed(agent.sinceMs, now)}</span>}
      </div>
      <AskingLine agent={agent} className="inbox-card-asking" />
      <div className="inbox-card-actions">
        <button className="inbox-open" title="Show this agent large on its wall" onClick={onOpen}>
          Open
        </button>
      </div>
    </div>
  );
}

function UpdateRow({ update, now, onOpen }: { update: Update; now: number; onOpen: (() => void) | null }) {
  const row = (
    <>
      <span className={`inbox-update-icon ${update.kind}`}>{ICON[update.kind]}</span>
      <span className="inbox-update-text">
        <span className="inbox-update-line">
          <b>{update.who}</b>
          {update.what && ` ${update.what}`}
        </span>
        {update.detail && <span className="inbox-update-detail">{update.detail}</span>}
      </span>
      <span className="inbox-update-time" title={new Date(update.atMs).toLocaleString()}>
        {age(update.atMs, now)}
      </span>
    </>
  );
  return onOpen ? (
    <button className="inbox-update" onClick={onOpen}>
      {row}
    </button>
  ) : (
    <div className="inbox-update">{row}</div>
  );
}
