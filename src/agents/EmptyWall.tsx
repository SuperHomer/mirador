import { openAgent, openAgentEditor, startTeam, TabSnapshot, toggleAgentWall } from "../bindings";
import { roleLine, useRoleCatalog } from "./NewMenu";
import { opensInTab } from "../state/configStore";

/**
 * The wall of a tab with no agents yet (21d): the ways to start one — a
 * role of this repository, a whole team, or plain Claude Code or Codex —
 * or back to the splits.
 */
export function EmptyWall({ tab }: { tab: TabSnapshot }) {
  const pane = tab.focusedPane;
  const catalog = useRoleCatalog(pane);
  const roles = catalog?.roles ?? [];
  const teams = catalog?.teams ?? [];
  const start = (role: string | null, harness: "claude" | "codex" | null) =>
    openAgent(role, null, pane, false, harness).catch((e) =>
      console.error(`mirador: ${role ?? harness}: ${e}`),
    );
  return (
    <div className="empty-wall">
      <div className="empty-wall-column">
        <h2 className="empty-wall-title">No agents in {tab.title} yet.</h2>
        <p className="empty-wall-body">
          Start one here, or run <code>claude</code>, <code>codex</code>… in any pane of this tab. It
          shows up on the wall by itself.
        </p>

        {roles.length > 0 && (
          <>
            <div className="empty-wall-label">Start a role</div>
            <div className="empty-wall-roles">
              {roles.map((r) => (
                <div key={r.name} className={`empty-wall-role${r.problem ? " problem" : ""}`}>
                  <span className="empty-wall-role-name">{r.name}</span>
                  <span className="empty-wall-role-meta">{r.problem ?? roleLine(r)}</span>
                  <button
                    className="empty-wall-start"
                    disabled={!!r.problem}
                    onClick={() => void start(r.name, null)}
                  >
                    Start
                  </button>
                </div>
              ))}
            </div>
          </>
        )}

        {teams.length > 0 && (
          <>
            <div className="empty-wall-label">Or a whole team</div>
            {teams.map((t) => (
              <div key={t.name} className="empty-wall-team">
                <span className="empty-wall-role-name">{t.name}</span>
                <span className="empty-wall-role-meta">
                  {t.problem ?? t.members.map((m) => (m.count > 1 ? `${m.role}×${m.count}` : m.role)).join(", ")}
                </span>
                <button
                  className="empty-wall-link"
                  onClick={() =>
                    startTeam(t.name, null, pane).catch((e) => console.error(`mirador: team ${t.name}: ${e}`))
                  }
                >
                  Start team…
                </button>
              </div>
            ))}
          </>
        )}

        <div className="empty-wall-links">
          <button className="empty-wall-link" onClick={() => void start(null, "claude")}>
            Plain Claude Code
          </button>
          <button className="empty-wall-link" onClick={() => void start(null, "codex")}>
            Plain Codex
          </button>
          <button className="empty-wall-link" onClick={() => void toggleAgentWall(pane)}>
            Show Splits
          </button>
          <button className="empty-wall-link" onClick={() => void openAgentEditor(pane, opensInTab("editor"))}>
            No roles? Create one
          </button>
        </div>
      </div>
    </div>
  );
}
