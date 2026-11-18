import { Harness, StartWhere, startWhere } from "../bindings";
import { reportFailure, useUiStore } from "../state/uiStore";

/** What is started: a role, a plain harness, or a team. */
export interface StartWhat {
  role?: string | null;
  harness?: Harness | null;
  team?: string | null;
}

/**
 * Starts an agent or a team from `pane`, as the person asked: at once when
 * it can only run here — a local pane, Codex, a role the remote refuses, a
 * role joining its tab's team (`start_where` says) — else after the
 * palette's Where step, here or on the pane's remote. `label` ("Start
 * reviewer") is the step's crumb and names a failure in the error bar.
 * `run` starts it, given the remote chosen (null: here; the command is
 * told only "on the remote", Rust reads which from the pane); a failure it
 * does not handle itself goes to the error bar.
 */
export function startHere(
  pane: string | null,
  what: StartWhat,
  label: string,
  run: (remote: StartWhere | null) => Promise<unknown>,
) {
  const start = (remote: StartWhere | null) => void run(remote).catch((e) => reportFailure(label, e));
  startWhere(pane, what.role ?? null, what.harness ?? null, what.team ?? null).then(
    (remote) =>
      remote ? useUiStore.getState().askWhere({ label, remote, go: (on) => start(on ? remote : null) }) : start(null),
    (e) => reportFailure(label, e),
  );
}
