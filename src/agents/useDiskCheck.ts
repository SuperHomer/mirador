import { useEffect, useRef, useState } from "react";
import { agentFileRead } from "../bindings";

/**
 * Whether the file at `path` changed on disk under unsaved edits. After
 * each refresh (`refreshed` counts them) the file is read again and
 * compared with `saved`, the text last loaded or saved: changed with
 * nothing unsaved, `onChanged` takes the new text; changed while `dirty`,
 * the edits stay and it is stale. A read is judged when it returns, by
 * the latest render: another file open by then drops it, edits begun
 * meanwhile make it stale rather than lost. No `path`, nothing is read.
 */
export function useDiskCheck(
  paneId: string,
  path: string | null,
  refreshed: number,
  saved: string,
  dirty: boolean,
  onChanged: (text: string) => void,
  onError: (e: unknown) => void,
): [boolean, (stale: boolean) => void] {
  const [stale, setStale] = useState(false);
  const latest = useRef({ path, saved, dirty, onChanged, onError });
  latest.current = { path, saved, dirty, onChanged, onError };

  useEffect(() => setStale(false), [path]);

  useEffect(() => {
    if (!path) return;
    let cancelled = false;
    agentFileRead(paneId, path)
      .then((t) => {
        const cur = latest.current;
        if (cancelled || cur.path !== path || t === cur.saved) return;
        if (cur.dirty) setStale(true);
        else cur.onChanged(t);
      })
      .catch((e: unknown) => {
        if (!cancelled && latest.current.path === path) latest.current.onError(e);
      });
    return () => {
      cancelled = true;
    };
    // Only a refresh reads: opening a file loads it already.
  }, [refreshed]);

  return [stale, setStale];
}
