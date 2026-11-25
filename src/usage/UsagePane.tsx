import { useCallback, useEffect, useState } from "react";
import {
  AccountUsage,
  HARNESS_LABEL,
  LimitWindow,
  TokenTotals,
  accountUsage,
  closePane,
  focusPane,
} from "../bindings";
import { useClaudeStore } from "../state/claudeStore";

type Props = {
  paneId: string;
  focused: boolean;
};

/**
 * Each harness's account, read in Rust from the harnesses' own files
 * (cmux-core/src/usage.rs): how it is signed in, its plan's limits, and
 * what it used in the last five hours and seven days. Read on opening and
 * on Refresh, never on a timer: it is a scan of every recent transcript.
 */
export function UsagePane({ paneId, focused }: Props) {
  const [accounts, setAccounts] = useState<AccountUsage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reading, setReading] = useState(false);

  const read = useCallback(() => {
    setReading(true);
    void accountUsage()
      .then((a) => {
        setAccounts(a);
        setError(null);
      })
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setReading(false));
  }, []);

  useEffect(read, [read]);

  return (
    <div
      className={`pane usage-pane${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">Usage</span>
        <span className="graph-repo">Accounts</span>
        <button
          className="graph-refresh"
          title="Read the files again"
          disabled={reading}
          onClick={read}
        >
          ⟳
        </button>
        <button className="graph-close" title="Close" onClick={() => void closePane(paneId)}>
          ✕
        </button>
      </div>
      {error && <div className="editor-bar error">{error}</div>}
      <div className="usage-body">
        {accounts === null && !error && <div className="usage-muted">Reading…</div>}
        {accounts?.map((a) => (
          <Account
            key={a.harness}
            account={a}
            onSetUpLimits={() => void useClaudeStore.getState().run().then(read)}
          />
        ))}
      </div>
    </div>
  );
}

const LOGIN_LABEL = {
  subscription: "Subscription",
  apiKey: "API key",
  planOrApiKey: "Plan or API key",
  none: "Not signed in",
};

function Account({ account, onSetUpLimits }: { account: AccountUsage; onSetUpLimits: () => void }) {
  const login = LOGIN_LABEL[account.login];
  const settingUp = useClaudeStore((s) => s.state === "running");
  return (
    <section className="usage-card">
      <div className="usage-head">
        <span className="usage-harness">{HARNESS_LABEL[account.harness]}</span>
        <span className="usage-login">{account.plan ? `${login} · ${account.plan}` : login}</span>
        {account.email && (
          <span className="usage-email" title={account.email}>
            {account.email}
          </span>
        )}
        {account.provider && <span className="usage-muted">via {account.provider}</span>}
        {account.note && <span className="usage-muted">{account.note}</span>}
      </div>
      {account.limits.map((w) => (
        <Limit key={w.windowMinutes} limit={w} />
      ))}
      {account.asOf !== null && account.limits.length > 0 && (
        <div className="usage-muted">as of {clock(account.asOf)}</div>
      )}
      {account.limitsNote && <div className="usage-muted">{account.limitsNote}</div>}
      {account.limitsSetup && (
        <button className="usage-open" disabled={settingUp} onClick={onSetUpLimits}>
          Show limits
        </button>
      )}
      <Totals name="Last 5 hours" totals={account.fiveHours} />
      <Totals name="Last 7 days" totals={account.sevenDays} />
    </section>
  );
}

function Limit({ limit: w }: { limit: LimitWindow }) {
  const used = Math.round(w.usedPercent);
  return (
    <div className="usage-limit">
      <span className="usage-limit-name">{w.name}</span>
      <div className="activity-bar usage-bar">
        <div
          className="activity-bar-fill usage-bar-fill"
          style={{ width: `${Math.min(100, Math.max(0, w.usedPercent))}%` }}
        />
      </div>
      <span className="usage-limit-used">{w.limitReached ? "limit reached" : `${used}% used`}</span>
      {w.resetsAt !== null && (
        <span className="usage-muted">
          resets{" "}
          {new Date(w.resetsAt * 1000).toLocaleString(undefined, {
            weekday: "short",
            day: "numeric",
            month: "short",
            hour: "2-digit",
            minute: "2-digit",
          })}
        </span>
      )}
    </div>
  );
}

/** A time of day on this machine's clock: `14:52`. */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function Totals({ name, totals: t }: { name: string; totals: TokenTotals }) {
  const tokens = t.input + t.cacheWrite + t.cacheRead + t.output;
  return (
    <div className="usage-total">
      <span className="usage-total-name">{name}</span>
      <span title={`${t.input.toLocaleString()} input, ${t.cacheWrite.toLocaleString()} cache write, ${t.cacheRead.toLocaleString()} cache read, ${t.output.toLocaleString()} output`}>
        {tokens.toLocaleString()} tokens
      </span>
      {t.cost !== null && <span className="usage-muted">${t.cost.toFixed(2)}</span>}
    </div>
  );
}
