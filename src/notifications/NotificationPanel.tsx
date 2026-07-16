import { useEffect } from "react";
import {
  focusPane,
  listNotifications,
  markAllNotificationsRead,
} from "../bindings";
import { useNotificationStore } from "../state/notificationStore";
import { useUiStore } from "../state/uiStore";
import { useWorkspaceStore } from "../state/workspaceStore";

function timeAgo(atMs: number): string {
  const s = Math.max(0, Math.floor((Date.now() - atMs) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

export function NotificationPanel() {
  const open = useUiStore((s) => s.notificationsOpen);
  const close = useUiStore((s) => s.closeNotifications);
  const notifications = useNotificationStore((s) => s.notifications);
  const setAll = useNotificationStore((s) => s.setAll);
  const unreadPanes = useWorkspaceStore(
    (s) => s.snapshot?.unreadPanes ?? [],
  );

  // Refresh read-flags whenever the panel opens.
  useEffect(() => {
    if (open) void listNotifications().then(setAll);
  }, [open, setAll]);

  if (!open) return null;

  const items = [...notifications].reverse();

  return (
    <div className="notif-panel">
      <div className="notif-header">
        <span>Notifications</span>
        <div className="notif-header-actions">
          <button
            onClick={() => {
              void markAllNotificationsRead();
              void listNotifications().then(setAll);
            }}
          >
            Mark all read
          </button>
          <button onClick={close}>×</button>
        </div>
      </div>
      <div className="notif-list">
        {items.length === 0 && (
          <div className="notif-empty">
            No notifications yet. Agents can ping you with
            <code> cmux notify "done"</code> or OSC 9/99/777 sequences.
          </div>
        )}
        {items.map((n) => (
          <div
            key={n.id}
            className={`notif-item${unreadPanes.includes(n.paneId) && !n.read ? " unread" : ""}`}
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
  );
}
