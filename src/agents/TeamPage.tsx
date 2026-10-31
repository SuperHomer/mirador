import { Ref, useState } from "react";
import {
  AgentStatus,
  HARNESS_LABEL,
  PromptLayer,
  RoleCatalog,
  TeamFields,
  TeamWeight,
} from "../bindings";
import { useWorkspaceStore } from "../state/workspaceStore";
import { STATUS_LABEL, StatusMark } from "./StatusMark";
import { SPENT_MEANS, Spent, seatsOf, spentLabel, tokensLabel, usd, useSpent } from "./teamWeight";

type Props = {
  name: string;
  fields: TeamFields;
  onChange: (fields: TeamFields) => void;
  catalog: RoleCatalog;
  /** A built-in or anything else that cannot be saved here. */
  readOnly: boolean;
  selected: number | null;
  onSelect: (member: number) => void;
  /** The charter's editor, for the inspector's Edit link to focus. */
  charterRef?: Ref<HTMLTextAreaElement>;
  /** What the team as saved tells its members (`useTeamWeight`). */
  weight: { weight: TeamWeight | null; error: string | null };
  /** The form has unsaved edits, which the weight does not count. */
  dirty: boolean;
};

/** How much a status wants you, to show a member's most urgent seat. */
const URGENCY: AgentStatus[] = ["needsYou", "working", "idle"];

/**
 * A team file as a form (17a): its members as cards, its way, layout and
 * description, and its charter. Every edit goes to `fields`; saving them
 * into the file is the editor's.
 */
export function TeamPage({ name, fields, onChange, catalog, readOnly, selected, onSelect, charterRef, weight, dirty }: Props) {
  const [adding, setAdding] = useState(false);
  const snapshot = useWorkspaceStore((s) => s.snapshot);

  // Where the team runs — in every tab running it — each seat's statuses
  // by its name, the seat names `teams::seats` gives: `dev` twice is
  // `dev-1` and `dev-2`.
  const statusOf = new Map<string, (AgentStatus | null)[]>();
  // And the panes of those that report what they spend.
  const running: string[] = [];
  for (const tab of snapshot?.tabs ?? []) {
    if (tab.team?.name !== name) continue;
    for (const seat of tab.team.members) {
      const agent = snapshot?.agents.find((a) => a.paneId === seat.pane);
      if (agent) statusOf.set(seat.name, [...(statusOf.get(seat.name) ?? []), agent.status]);
      if (agent?.hasActivity) running.push(agent.paneId);
    }
  }
  const spent = useSpent(running);
  // The most urgent of a member's seats, across every run: undefined when
  // none of them runs, null when running with no status known.
  const seatStatus = (base: string, count: number): AgentStatus | null | undefined => {
    const seats = count > 1 ? Array.from({ length: count }, (_, i) => `${base}-${i + 1}`) : [base];
    const found = seats.flatMap((s) => statusOf.get(s) ?? []);
    if (found.length === 0) return undefined;
    return URGENCY.find((u) => found.includes(u)) ?? null;
  };

  const ways = catalog.ways.map((w) => w.name);
  if (fields.way && !ways.includes(fields.way)) ways.push(fields.way);

  return (
    <div className="team-page">
      <div className="team-label">MEMBERS</div>
      <div className="team-members">
        {fields.members.map((m, i) => {
          const base = m.name ?? m.role;
          const role = catalog.roles.find((r) => r.name === m.role);
          const problem = role ? role.problem : `no role \`${m.role}\``;
          const status = seatStatus(base, m.count);
          // All its seats together; none for a member not saved yet.
          const seats = weight.weight ? seatsOf(weight.weight, base, m.count) : [];
          const tokens = seats.length > 0 ? seats.reduce((n, s) => n + s.tokens, 0) : null;
          return (
            <button
              key={i}
              className={`team-member${m.lead ? " lead" : ""}${problem ? " problem" : ""}${selected === i ? " selected" : ""}`}
              onClick={() => onSelect(i)}
            >
              <span className="team-member-head">
                <span className="team-member-name">
                  {base}
                  {m.count > 1 && <span className="team-member-count"> ×{m.count}</span>}
                </span>
                {m.lead && <span className="team-lead">LEAD</span>}
                {status !== undefined && (
                  <span className={`team-member-status ${status ?? "unknown"}`}>
                    <StatusMark status={status} small />
                    {status ? STATUS_LABEL[status] : "running"}
                  </span>
                )}
              </span>
              <span className={`team-member-role${problem ? " problem" : ""}`}>
                {problem ??
                  [HARNESS_LABEL[role!.harness], role!.model ?? "default model", role!.effort]
                    .filter(Boolean)
                    .join(" · ")}
                {!problem && tokens !== null && <span className="team-member-tokens"> · {tokensLabel(tokens)}</span>}
              </span>
            </button>
          );
        })}
      </div>
      {!readOnly &&
        (adding ? (
          <select
            className="team-add"
            autoFocus
            value=""
            onBlur={() => setAdding(false)}
            onChange={(e) => {
              setAdding(false);
              if (!e.target.value) return;
              onChange({
                ...fields,
                members: [...fields.members, { role: e.target.value, name: null, count: 1, lead: false }],
              });
            }}
          >
            <option value="">Pick a role…</option>
            {catalog.roles.map((r) => (
              <option key={r.name} value={r.name}>
                {r.name}
              </option>
            ))}
          </select>
        ) : (
          <button className="team-add" onClick={() => setAdding(true)}>
            + Add member
          </button>
        ))}
      <div className="team-label">PROMPT WEIGHT</div>
      <TeamWeightBlock weight={weight} dirty={dirty} spent={running.length > 0 ? spent : null} />
      <div className="team-fields">
        <label>
          <span>Way of working</span>
          <select
            disabled={readOnly}
            value={fields.way ?? ""}
            onChange={(e) => onChange({ ...fields, way: e.target.value || null })}
          >
            <option value="">none</option>
            {ways.map((w) => (
              <option key={w} value={w}>
                {w}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Layout</span>
          <select
            disabled={readOnly}
            value={fields.layout ?? "grid"}
            onChange={(e) => onChange({ ...fields, layout: e.target.value })}
          >
            {["grid", "columns", "rows"].map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
            {fields.layout && !["grid", "columns", "rows"].includes(fields.layout) && (
              <option value={fields.layout}>{fields.layout}</option>
            )}
          </select>
        </label>
        <label>
          <span>Description</span>
          <input
            readOnly={readOnly}
            value={fields.description ?? ""}
            onChange={(e) => onChange({ ...fields, description: e.target.value || null })}
          />
        </label>
      </div>
      <div className="team-label">
        CHARTER · TEAM PROMPT
        <span className="team-label-note">Markdown · every member is told this</span>
      </div>
      <textarea
        ref={charterRef}
        className="team-charter"
        spellCheck={false}
        readOnly={readOnly}
        value={fields.charter ?? ""}
        onChange={(e) => onChange({ ...fields, charter: e.target.value || null })}
      />
    </div>
  );
}

/**
 * What the team adds to its members' prompts, in tokens, and the least
 * that costs a turn — a lower bound, which says what it leaves out.
 */
function TeamWeightBlock({
  weight: { weight, error },
  dirty,
  spent,
}: Pick<Props, "weight" | "dirty"> & { spent: Spent | null }) {
  const [details, setDetails] = useState(false);
  const floor = (w: TeamWeight) => {
    if (w.unpriced === w.seats.length) return "no price";
    const without =
      w.unpriced === 0 ? "" : ` · ${w.unpriced} ${w.unpriced === 1 ? "member" : "members"} without a price`;
    return `≥ ${usd(w.perTurnFloorUsd)} a turn${without}`;
  };
  return (
    <div className="team-weight">
      {error ? (
        <p className="team-weight-error">{error}</p>
      ) : weight ? (
        <>
          <div className="team-weight-figures">
            <span className="team-weight-tokens">{tokensLabel(weight.tokens)}</span>
            <span className="team-weight-floor">{floor(weight)}</span>
            <button className="team-weight-toggle" onClick={() => setDetails(!details)}>
              {details ? "Hide details" : "Details"}
            </button>
          </div>
          {details && <WeightDetails weight={weight} />}
          <p className="team-weight-note">
            Mirador's prompts to the members, re-sent from the cache each turn at least. Not counted: each
            agent's own system prompt and tools (Claude Code's, Codex's), CLAUDE.md, MCP tools, the
            conversation and its output. Prices as of{" "}
            {weight.pricesChecked}.
          </p>
        </>
      ) : (
        <p className="team-weight-note">Weighing…</p>
      )}
      {dirty && <p className="team-weight-note">Unsaved changes are not counted.</p>}
      {spent && (
        <div className="team-weight-figures" title={SPENT_MEANS}>
          <span className="team-weight-tokens">Spent so far: {spentLabel(spent)}</span>
          <span className="team-weight-floor">
            its running members and their subagents, at API list prices — on a Claude subscription, what
            it would have cost, not what is billed
            {spent.unpriced ? "; some replies had no known price and are not in it" : ""}
          </span>
        </div>
      )}
    </div>
  );
}

/** The parts of a member's prompt, in the order it is told them. */
const PARTS: { kind: PromptLayer["kind"]; label: string }[] = [
  { kind: "role", label: "Role prompt" },
  { kind: "roleWay", label: "Role's way of working" },
  { kind: "team", label: "Its place in the team (by Mirador)" },
  { kind: "charter", label: "Charter" },
  { kind: "teamWay", label: "Team's way of working" },
];

/**
 * Where a team's tokens come from: each part of what each member is told,
 * by member, with each part's share of the whole, and how each member's
 * floor is worked out from its model's price.
 */
function WeightDetails({ weight }: { weight: TeamWeight }) {
  const tokensOf = (layers: TeamWeight["seats"][number]["layers"], kind: PromptLayer["kind"]) =>
    layers.filter((l) => l.kind === kind).reduce((n, l) => n + l.tokens, 0);
  const parts = PARTS.filter((p) => weight.seats.some((s) => tokensOf(s.layers, p.kind) > 0));
  const share = (n: number) => (weight.tokens > 0 ? `${Math.round((n / weight.tokens) * 100)}%` : "");
  const count = (n: number) => n.toLocaleString("en-US");
  // Each priced model's cache-read price, once.
  const rates = [
    ...new Set(
      weight.seats.flatMap((s) =>
        s.cacheReadPerMillion === null ? [] : [`${s.model}: $${s.cacheReadPerMillion} per million`],
      ),
    ),
  ];
  return (
    <div className="team-weight-details">
      <table>
        <thead>
          <tr>
            <th />
            {weight.seats.map((s) => (
              <th key={s.name}>
                {s.name}
                <span className="team-weight-model">{s.model ?? "default model"}</span>
              </th>
            ))}
            <th>Team</th>
          </tr>
        </thead>
        <tbody>
          {parts.map((p) => {
            const all = weight.seats.reduce((n, s) => n + tokensOf(s.layers, p.kind), 0);
            return (
              <tr key={p.kind}>
                <th>{p.label}</th>
                {weight.seats.map((s) => {
                  const n = tokensOf(s.layers, p.kind);
                  const sources = s.layers.filter((l) => l.kind === p.kind).map((l) => l.source);
                  return (
                    <td key={s.name} title={sources.join("\n")}>
                      {n > 0 ? count(n) : "—"}
                    </td>
                  );
                })}
                <td>
                  {count(all)} <span className="team-weight-share">{share(all)}</span>
                </td>
              </tr>
            );
          })}
        </tbody>
        <tfoot>
          <tr>
            <th>Tokens</th>
            {weight.seats.map((s) => (
              <td key={s.name}>
                {count(s.tokens)} <span className="team-weight-share">{share(s.tokens)}</span>
              </td>
            ))}
            <td>{count(weight.tokens)}</td>
          </tr>
          <tr>
            <th>Per turn, at least</th>
            {weight.seats.map((s) => (
              <td
                key={s.name}
                title={
                  s.cacheReadPerMillion === null
                    ? "No known price for this model"
                    : `${count(s.tokens)} tokens × $${s.cacheReadPerMillion} per million, its cache-read price`
                }
              >
                {s.perTurnFloorUsd === null ? "no price" : usd(s.perTurnFloorUsd)}
              </td>
            ))}
            <td>{usd(weight.perTurnFloorUsd)}</td>
          </tr>
        </tfoot>
      </table>
      <p className="team-weight-note">
        Tokens are estimated at about 4 characters a token. Claude Code sends its whole system prompt again every
        turn; these parts of it stay the same, so they come from the prompt cache, at the model&apos;s cache-read
        price{rates.length > 0 ? ` (${rates.join(", ")})` : ""}. The first turn writes them into the cache, at
        1.25× the input price, and costs more.
      </p>
    </div>
  );
}
