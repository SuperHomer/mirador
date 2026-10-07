import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  UpdateInfo,
  appVersion,
  availableUpdate,
  installUpdate,
  openWhatsNew,
} from "../bindings";
import { Progress, useUpdateStore } from "../state/updateStore";

/** "6.7 MB" — one decimal is as much as a progress line can use. */
function mb(bytes: number): string {
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

/**
 * What to show while installing. A download the server sized becomes a
 * fraction; one it did not becomes a running total, which still moves and so
 * still reads as progress. Once the download finishes there is nothing left
 * to count — verifying and swapping the bundle have no size.
 */
function installLabel(progress: Progress | null): string {
  if (!progress) return "Installing…";
  const { downloaded, total } = progress;
  // A zero or absent total is "unknown size", not "complete".
  if (total === null || total <= 0) return `Downloading… ${mb(downloaded)}`;
  const pct = Math.min(100, Math.round((downloaded / total) * 100));
  return `Downloading… ${pct}% of ${mb(total)}`;
}

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
  const progress = useUpdateStore((s) => s.progress);
  const setProgress = useUpdateStore((s) => s.setProgress);
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
    const unlistenProgress = listen<Progress>("update-progress", (e) =>
      setProgress(e.payload),
    );
    // The download is over; what follows has no size to report.
    const unlistenDone = listen("update-download-finished", () =>
      setProgress(null),
    );
    return () => {
      void unlisten.then((fn) => fn());
      void unlistenProgress.then((fn) => fn());
      void unlistenDone.then((fn) => fn());
    };
  }, [setUpdate, setProgress]);

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
    setProgress(null);
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
          <>
            <button
              className="update-banner-action"
              disabled={installing}
              onClick={start}
              title="Download, install and restart"
            >
              {installing ? installLabel(progress) : "Install and restart"}
            </button>
            {/* Deciding whether to restart a terminal mid-task is easier
                having read what the update does — all of it, when it is
                several releases ahead of the running one. */}
            {update.notes && !installing && (
              <button
                className="update-banner-notes"
                onClick={() =>
                  void openWhatsNew(update.version, update.currentVersion)
                }
                title={`What's new in ${update.version}`}
              >
                What's new
              </button>
            )}
          </>
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
