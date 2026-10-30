import { useEffect, useState } from "react";
import { EditorCatalog, SeatWeight, TeamWeight, teamCost } from "../bindings";

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
