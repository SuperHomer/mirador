import { useEffect } from "react";
import { claudeIntegrationOffer, dismissClaudeIntegration } from "../bindings";
import { useClaudeStore } from "../state/claudeStore";

/** How long an outcome stays on screen. */
const TRANSIENT_MS = 6000;

/**
 * Foot of the sidebar, above the update banner. Offers the Claude Code
 * integration once — when Claude Code is used here and Mirador's hooks are
 * not installed — rather than editing another tool's settings unasked.
 * "Not now" is remembered; the palette's "Set Up Claude Code Integration"
 * runs the same setup later and answers here.
 */
export function ClaudeBanner() {
  const state = useClaudeStore((s) => s.state);
  const error = useClaudeStore((s) => s.error);
  const setState = useClaudeStore((s) => s.setState);
  const run = useClaudeStore((s) => s.run);

  useEffect(() => {
    void claudeIntegrationOffer().then((offer) => {
      if (offer && useClaudeStore.getState().state === "idle") setState("offer");
    });
  }, [setState]);

  // An outcome settles back to nothing on its own.
  useEffect(() => {
    if (state !== "done" && state !== "error") return;
    const timer = setTimeout(
      () => setState("idle"),
      state === "error" ? TRANSIENT_MS * 2 : TRANSIENT_MS,
    );
    return () => clearTimeout(timer);
  }, [state, setState]);

  if (state === "offer") {
    return (
      <div className="update-banner">
        <div className="update-banner-text">
          <span className="update-banner-title">Claude Code integration</span>
          <span className="update-banner-note">
            Light up tabs when an agent needs you, and add /mira-diff.
          </span>
        </div>
        <button className="update-banner-action" onClick={() => void run()}>
          Set up
        </button>
        <button
          className="update-banner-notes"
          onClick={() => {
            setState("idle");
            void dismissClaudeIntegration();
          }}
          title="The palette's “Set Up Claude Code Integration” does it later"
        >
          Not now
        </button>
      </div>
    );
  }

  if (state === "running") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-note">Setting up Claude Code…</span>
      </div>
    );
  }

  if (state === "done") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-note">
          Claude Code integration set up. New Claude Code sessions pick it up.
        </span>
      </div>
    );
  }

  if (state === "error") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-error" title={error ?? undefined}>
          Claude Code setup failed: {error}
        </span>
      </div>
    );
  }

  return null;
}
