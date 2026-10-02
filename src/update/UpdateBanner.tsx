import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  UpdateInfo,
  availableUpdate,
  installUpdate,
} from "../bindings";
import { useUpdateStore } from "../state/updateStore";

/**
 * Foot of the sidebar: nothing at all until an update exists, then one row
 * the user can act on. Installing restarts the app, so it never happens on
 * its own — a terminal that relaunches under a running agent is not a
 * favour, however new the version.
 */
export function UpdateBanner() {
  const update = useUpdateStore((s) => s.update);
  const installing = useUpdateStore((s) => s.installing);
  const error = useUpdateStore((s) => s.error);
  const setUpdate = useUpdateStore((s) => s.setUpdate);
  const setInstalling = useUpdateStore((s) => s.setInstalling);
  const setError = useUpdateStore((s) => s.setError);

  useEffect(() => {
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

  if (!update) return null;

  const start = () => {
    setError(null);
    setInstalling(true);
    // Resolves only on failure — success replaces the app and restarts it.
    void installUpdate().catch((e: unknown) => setError(String(e)));
  };

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
