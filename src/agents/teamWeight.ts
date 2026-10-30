import { useEffect, useState } from "react";
import {
  EditorCatalog,
  SeatWeight,
  TeamWeight,
  TranscriptSummary,
  getAgentActivity,
  teamCost,
} from "../bindings";

/**
 * The weight of team `team` as its files are saved (`team_cost`), read
 * again whenever the editor's catalog is — after a save, a refresh, or
 * coming back to the pane. Null until it comes; an error for a team
 * that cannot start. An answer is kept with the team and pane it was
 * asked for, and given only while those are still the ones shown: going
 * from one team to another never shows the first one's weight on the
 * second's page while the second's is on its way.
 */
export function useTeamWeight(paneId: string, team: string | null, cat: EditorCatalog | null) {
  const [answer, setAnswer] = useState<{
    paneId: string;
    team: string;
    weight: TeamWeight | null;
    error: string | null;
  } | null>(null);
  useEffect(() => {
    if (!team) return;
    let stale = false;
    teamCost(team, paneId)
      .then((weight) => {
        if (!stale) setAnswer({ paneId, team, weight, error: null });
      })
      .catch((e: unknown) => {
        if (!stale) setAnswer({ paneId, team, weight: null, error: String(e) });
      });
    return () => {
      stale = true;
    };
  }, [paneId, team, cat]);
  const current = answer && team && answer.team === team && answer.paneId === paneId ? answer : null;
  return { weight: current?.weight ?? null, error: current?.error ?? null };
}

/** The seats of a member of `weight`, as `teams::seats` names them:
 *  `dev` twice is `dev-1` and `dev-2`. */
export function seatsOf(weight: TeamWeight, base: string, count: number): SeatWeight[] {
  const names = count > 1 ? Array.from({ length: count }, (_, i) => `${base}-${i + 1}`) : [base];
  return weight.seats.filter((s) => names.includes(s.name));
}

/** "≈820 tokens", "≈1.1k tokens", "≈12k tokens". */
export function tokensLabel(n: number): string {
  if (n < 1000) return `≈${n} tokens`;
  const k = n / 1000;
  return `≈${k < 10 ? k.toFixed(1).replace(/\.0$/, "") : Math.round(k)}k tokens`;
}

/** Dollars: cents from $0.01 up, four decimals below, and "<$0.0001"
 *  under that rather than a row of zeros. */
export function usd(d: number): string {
  if (d >= 0.01) return `$${d.toFixed(2)}`;
  if (d > 0 && d < 0.0001) return "<$0.0001";
  return `$${d.toFixed(4)}`;
}

/** What a cost figure is, for a tooltip or a note beside it. */
export const SPENT_MEANS =
  "At API list prices for the tokens its transcripts record. On a Claude subscription it is what this would have cost, not what is billed.";

/** What some transcripts cost together: none when none was priced, and
 *  whether replies with no known price were left out. */
export type Spent = { usd: number | null; unpriced: boolean };

export function spentOf(summaries: Pick<TranscriptSummary, "costUsd" | "unpriced">[]): Spent {
  const priced = summaries.filter((s) => s.costUsd !== null);
  return {
    usd: priced.length > 0 ? priced.reduce((sum, s) => sum + s.costUsd!, 0) : null,
    unpriced: summaries.some((s) => s.unpriced),
  };
}

/** "$1.24", "$0.0042", "$1.24+" when some replies had no price, "—" when
 *  none was priced. */
export function spentLabel({ usd: dollars, unpriced }: Spent): string {
  if (dollars === null) return "—";
  return `${usd(dollars)}${unpriced ? "+" : ""}`;
}

/** How often a running team's spend is read again. */
const SPENT_POLL_MS = 5000;

/**
 * What the agents in `panes` have spent so far, each session with its
 * subagents, read again every few seconds while any runs. Null with no
 * pane, or until the first read; a pane whose activity cannot be read
 * is left out of the sum, as one that reported nothing yet.
 */
export function useSpent(panes: string[]): Spent | null {
  const [spent, setSpent] = useState<Spent | null>(null);
  const key = panes.join(" ");
  useEffect(() => {
    setSpent(null);
    if (panes.length === 0) return;
    let cancelled = false;
    let timer: number | undefined;
    const load = () => {
      void Promise.all(panes.map((p) => getAgentActivity(p).catch(() => null)))
        .then((all) => {
          if (cancelled) return;
          const summaries = all.flatMap((a) => (a ? [a.session, ...a.subagents] : []));
          setSpent(spentOf(summaries));
        })
        .finally(() => {
          if (!cancelled) timer = window.setTimeout(load, SPENT_POLL_MS);
        });
    };
    load();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [key]);
  return spent;
}
