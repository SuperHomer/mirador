import { useEffect, useRef, useState } from "react";
import {
  AgentInfo,
  Node,
  TabSnapshot,
  agentLabel,
  focusPane,
  listNotifications,
  markAllNotificationsRead,
  waitingAgents,
} from "../bindings";
import { useNotificationStore } from "../state/notificationStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { StatusMark } from "../agents/StatusMark";
import { AskingLine } from "../agents/AskingLine";
import { elapsed, useNow } from "../agents/time";
import { showOnWall } from "../agents/showOnWall";

function timeAgo(atMs: number): string {
  const s = Math.max(0, Math.floor((Date.now() - atMs) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

function paneIds(node: Node): string[] {
  if (node.type === "leaf") return [node.paneId];
  return node.children.flatMap(paneIds);
}

/** The Inbox: the agents waiting on you, then what was said to you. */
export function NotificationPanel() {
  const open = useUiStore((s) => s.notificationsOpen);
  const close = useUiStore((s) => s.closeNotifications);
  const notifications = useNotificationStore((s) => s.notifications);
  const setAll = useNotificationStore((s) => s.setAll);
  const snapshot = useWorkspaceStore((s) => s.snapshot);
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
  const items = [...notifications].reverse();
  // A chip for each tab with someone waiting or something said.
  const chipTabs = snapshot.tabs.filter(
    (t) => waiting.some((a) => a.tabId === t.id) || items.some((n) => tabOfPane.get(n.paneId)?.id === t.id),
  );
  const filter = chipTabs.some((t) => t.id === tabFilter) ? tabFilter : null;
  const shownWaiting = waiting.filter((a) => !filter || a.tabId === filter);
  const shownItems = items.filter((n) => !filter || tabOfPane.get(n.paneId)?.id === filter);

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
          NOTIFICATIONS
          {items.length > 0 && (
            <button
              className="inbox-section-action"
              onClick={() => {
                void markAllNotificationsRead();
                void listNotifications().then(setAll);
              }}
            >
              Mark all read
            </button>
          )}
        </div>
        <div className="notif-list">
          {shownItems.length === 0 && (
            <div className="notif-empty">
              No notifications yet. Agents can ping you with
              <code> mira notify "done"</code> or OSC 9/99/777 sequences.
            </div>
          )}
          {shownItems.map((n) => (
            <div
              key={n.id}
              className={`notif-item${snapshot.unreadPanes.includes(n.paneId) && !n.read ? " unread" : ""}`}
              onClick={() => {
                void focusPane(n.paneId);
                close();
              }}
            >
              <div className="notif-item-top">
                <span className="notif-title">{n.title ?? "Notification"}</span>
                <span className="notif-time">{timeAgo(n.atMs)}</span>
              </div>
              {n.body && <div className="notif-body">{n.body}</div>}
            </div>
          ))}
        </div>
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
