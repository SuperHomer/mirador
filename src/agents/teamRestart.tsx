import { TabSnapshot, WorkspaceSnapshot, teamRestart } from "../bindings";

/** A member of a running team, as its tab keeps it. */
type Seat = { name: string; role: string };

/** Each team tab with members `concerned` — by its team's name and the
 *  member — those of them that are not running, which `team restart`
 *  starts again, and those that are. A tab with none is left out. */
export function teamRuns(
  snapshot: WorkspaceSnapshot | null,
  concerned: (team: string, seat: Seat) => boolean,
) {
  const agentPanes = new Set((snapshot?.agents ?? []).map((a) => a.paneId));
  return (snapshot?.tabs ?? []).flatMap((t: TabSnapshot) => {
    const members = (t.team?.members ?? []).filter((m) => concerned(t.team!.name, m));
    if (members.length === 0) return [];
    return [
      {
        tab: t,
        stopped: members.filter((m) => !agentPanes.has(m.pane)).map((m) => m.name),
        running: members.filter((m) => agentPanes.has(m.pane)).map((m) => m.name),
      },
    ];
  });
}

/** A restart waiting for a yes: what it would do. */
export interface Confirm {
  /** Each team tab and the members `team restart` would start again. */
  runs: ReturnType<typeof teamRuns>;
  /** Running agents it cannot restart: no command restarts one. */
  keep: string[];
}

/** Restarts the stopped members of `confirm`; every member's outcome by
 *  name — one whose shell runs something else is "busy", not restarted. */
export function restart(confirm: Confirm): Promise<string> {
  const runs = confirm.runs.filter((r) => r.stopped.length > 0);
  return Promise.all(
    // Those members by name, in their tab: any of its panes names the
    // team, and its focused one is still there.
    runs.map((r) => teamRestart(r.stopped, r.tab.focusedPane)),
  ).then((done) => {
    const outcomes = done.flat().map((d) => `${d.name} ${d.outcome}`);
    return outcomes.length ? outcomes.join(", ") : "Nothing restarted";
  });
}

/** The question a restart asks, inline: who it restarts, and who it
 *  cannot because they are running. */
export function RestartConfirm({
  confirm,
  onRestart,
  onCancel,
}: {
  confirm: Confirm;
  onRestart: () => void;
  onCancel: () => void;
}) {
  const stopped = confirm.runs.flatMap((r) => r.stopped);
  const keep = confirm.keep;
  const one = keep.length === 1;
  return (
    <div className="editor-bar confirm">
      <span>
        {stopped.length > 0 ? `Restart ${stopped.join(", ")}?` : "Nothing to restart."}
        {keep.length > 0 &&
          ` ${keep.join(", ")} ${one ? "is" : "are"} running and keep${one ? "s" : ""} the prompt ${one ? "it" : "they"} started with: nothing restarts a running agent, so stop ${one ? "it" : "them"} first.`}
      </span>
      {stopped.length > 0 && (
        <button className="primary" onClick={onRestart}>
          Restart
        </button>
      )}
      <button onClick={onCancel}>{stopped.length > 0 ? "Cancel" : "OK"}</button>
    </div>
  );
}
