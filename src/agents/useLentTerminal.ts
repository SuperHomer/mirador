import { useEffect, useRef, useState } from "react";
import {
  fitTerminal,
  isLent,
  isLentTo,
  lendTerminal,
  watchTerminal,
} from "../terminal/registry";

/**
 * Borrows `pane`'s terminal into the returned host while `visible`, the
 * way every wall tile does. `fit`: the terminal (and so the PTY) is sized
 * to the host, now and whenever the host resizes; without it, it keeps
 * its size and the tile scales it (`lendTerminal`). `shown` is false while
 * another place holds the terminal.
 */
export function useLentTerminal(pane: string, visible: boolean, { fit }: { fit: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null);
  // Assumed until a lend fails, so a tile does not flash the placeholder.
  const [shown, setShown] = useState(true);
  const releaseRef = useRef<(() => void) | null>(null);

  // Borrow the pane's terminal whenever the wall is on screen. The pane may
  // register after the wall mounts (tabs mount in order), remount, or have
  // been taken back while the wall was hidden; a terminal lent nowhere is
  // picked up whenever it appears.
  useEffect(() => {
    const host = hostRef.current;
    if (!visible || !host) return;
    const borrow = () => {
      if (isLentTo(pane, host)) {
        setShown(true);
        return;
      }
      if (isLent(pane)) {
        setShown(false);
        return;
      }
      const release = lendTerminal(pane, host, { fit });
      if (release) releaseRef.current = release;
      setShown(!!release);
    };
    borrow();
    const unwatch = watchTerminal(pane, borrow);
    if (!fit) return unwatch;
    // The tile may have changed size while the wall was hidden.
    if (isLentTo(pane, host)) fitTerminal(pane);
    const observer = new ResizeObserver(() => {
      if (isLentTo(pane, host)) fitTerminal(pane);
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      unwatch();
    };
  }, [pane, visible, fit]);

  // Give the terminal back only when the tile goes away: while the wall is
  // merely hidden it stays here, until its own tab is shown and takes it
  // (see `lendTerminal` for why every move counts). Declared after the
  // borrow so that React, which runs cleanups in order, unwatches first:
  // the release announces the terminal free, and a tile still listening
  // would borrow it again into a host leaving the page.
  useEffect(
    () => () => {
      releaseRef.current?.();
      releaseRef.current = null;
    },
    [pane],
  );

  return { hostRef, shown };
}
