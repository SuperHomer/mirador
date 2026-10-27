import { useEffect, useState } from "react";
import { sendNextTask } from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";

/** How long a failed send says why, in place of the link. */
const ERROR_MS = 5000;

/**
 * Sends an idle agent its next task (`send_next_task`: its own first To do
 * card, else its tab's first unassigned one). Dimmed from the click until
 * the next snapshot, which shows it at work; a refusal shows briefly in
 * its place.
 */
export function SendNextTask({ pane, label }: { pane: string; label: string }) {
  const snapshot = useWorkspaceStore((s) => s.snapshot);
  const [sentAt, setSentAt] = useState<typeof snapshot | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(() => setError(null), ERROR_MS);
    return () => window.clearTimeout(timer);
  }, [error]);
  if (error) return <span className="send-next-error">{error}</span>;
  const dim = sentAt !== undefined && sentAt === snapshot;
  return (
    <button
      className={`send-next${dim ? " sent" : ""}`}
      disabled={dim}
      onClick={(e) => {
        // On a rail line, the click is the link's, not the line's.
        e.stopPropagation();
        setSentAt(snapshot);
        sendNextTask(pane).catch((err) => {
          setSentAt(undefined);
          setError(String(err));
        });
      }}
    >
      {label}
    </button>
  );
}
