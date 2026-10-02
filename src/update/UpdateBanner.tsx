import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  UpdateInfo,
  appVersion,
  availableUpdate,
  installUpdate,
} from "../bindings";
import { useUpdateStore } from "../state/updateStore";

/** How long a "nothing to do" answer stays on screen. */
const TRANSIENT_MS = 4000;

/**
 * Foot of the sidebar. Silent unless there is something to say:
 *
 * - an update exists → a row that stays until it is installed
 * - a *user-initiated* check found nothing, or failed → a transient answer,
 *   because a command that appears to do nothing reads as broken
 *
 * Background checks never produce the transient states; they are allowed to
 * find nothing quietly, which is the whole point of running them unasked.
 */
export function UpdateBanner() {
  const update = useUpdateStore((s) => s.update);
  const installing = useUpdateStore((s) => s.installing);
  const error = useUpdateStore((s) => s.error);
  const check = useUpdateStore((s) => s.check);
  const setUpdate = useUpdateStore((s) => s.setUpdate);
  const setInstalling = useUpdateStore((s) => s.setInstalling);
  const setError = useUpdateStore((s) => s.setError);
  const setCheck = useUpdateStore((s) => s.setCheck);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    void appVersion().then(setVersion);
    // Pull as well as listen: the background check runs ten seconds after
    // launch and can easily beat this listener, in which case the event is
    // already gone and only the stored value remains.
    void availableUpdate().then((u) => {
      if (u) setUpdate(u);
    });
    const unlisten = listen<UpdateInfo>("update-available", (e) =>
      setUpdate(e.payload),
    );
    return () => void unlisten.then((fn) => fn());
  }, [setUpdate]);

  // Clear a transient answer on its own, so the sidebar settles back.
  useEffect(() => {
    if (check !== "uptodate" && check !== "error") return;
    const timer = setTimeout(() => {
      setCheck(null);
      setError(null);
    }, check === "error" ? TRANSIENT_MS * 2 : TRANSIENT_MS);
    return () => clearTimeout(timer);
  }, [check, setCheck, setError]);

  const start = () => {
    setError(null);
    setInstalling(true);
    // Resolves only on failure — success replaces the app and restarts it.
    void installUpdate().catch((e: unknown) => setError(String(e)));
  };

  if (update) {
    return (
      <div className="update-banner">
        <div className="update-banner-text">
          <span className="update-banner-title">Update available</span>
          <span className="update-banner-version">
            {update.currentVersion} → {update.version}
          </span>
        </div>
        {error ? (
          <div className="update-banner-error" title={error}>
            {error}
          </div>
        ) : (
          <button
            className="update-banner-action"
            disabled={installing}
            onClick={start}
            title="Download, install and restart"
          >
            {installing ? "Installing…" : "Install and restart"}
          </button>
        )}
      </div>
    );
  }

  if (check === "checking") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-note">Checking for updates…</span>
      </div>
    );
  }

  if (check === "uptodate") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-note">
          Up to date{version ? ` — ${version}` : ""}
        </span>
      </div>
    );
  }

  if (check === "error") {
    return (
      <div className="update-banner quiet">
        <span className="update-banner-error" title={error ?? undefined}>
          Update check failed
        </span>
      </div>
    );
  }

  return null;
}
