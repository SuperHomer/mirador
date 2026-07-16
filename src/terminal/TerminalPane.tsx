import { useEffect, useRef } from "react";
import { Channel } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { FitAddon } from "@xterm/addon-fit";
import { createTerminal, attachRenderer, applyConfig } from "./xtermFactory";
import { registerTerminal, unregisterTerminal } from "./registry";
import { useConfigStore } from "../state/configStore";
import {
  attachPane,
  writePty,
  resizePty,
  ackPty,
  focusPane,
  PtyData,
} from "../bindings";

/** Ack processed output back to Rust every 256KB to release backpressure. */
const ACK_THRESHOLD = 256 * 1024;

interface Props {
  paneId: string;
  focused: boolean;
  unread: boolean;
}

export function TerminalPane({ paneId, focused, unread }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<ReturnType<typeof createTerminal> | null>(null);
  const config = useConfigStore((s) => s.config);
  const fitRef = useRef<FitAddon | null>(null);

  // Hot-reloaded config applies to the live terminal without recreating it.
  useEffect(() => {
    const term = termRef.current;
    if (term && config) {
      applyConfig(term, config);
      fitRef.current?.fit();
    }
  }, [config]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    // App gates rendering on config being loaded.
    const cfg = useConfigStore.getState().config;
    if (!cfg) return;

    let disposed = false;
    let exited = false;
    let pendingAck = 0;

    const term = createTerminal(cfg);
    termRef.current = term;
    registerTerminal(paneId, term);
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    term.open(container);
    attachRenderer(term);
    fit.fit();

    const channel = new Channel<PtyData>();
    channel.onmessage = (data) => {
      const size =
        data instanceof ArrayBuffer
          ? data.byteLength
          : typeof data === "string"
            ? data.length
            : data.byteLength;
      const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
      term.write(bytes, () => {
        pendingAck += size;
        if (pendingAck >= ACK_THRESHOLD && !exited) {
          void ackPty(paneId, pendingAck);
          pendingAck = 0;
        }
      });
    };

    const attach = async () => {
      exited = false;
      pendingAck = 0;
      try {
        const spawned = await attachPane(paneId, term.cols, term.rows, channel);
        if (disposed) return;
        const buf = term.buffer.active;
        if (!spawned && buf.cursorX === 0 && buf.cursorY === 0) {
          // Re-attached to a live shell with an empty screen (remount):
          // Ctrl-L makes it repaint the prompt.
          void writePty(paneId, "\x0c");
        }
      } catch (err) {
        term.writeln(`\x1b[31mfailed to attach shell: ${err}\x1b[0m`);
      }
    };

    // Batch same-tick keystroke bursts (e.g. paste) into one IPC call.
    let writeQueue = "";
    let flushScheduled = false;
    term.onData((d) => {
      if (exited) {
        void attach();
        return;
      }
      writeQueue += d;
      if (!flushScheduled) {
        flushScheduled = true;
        queueMicrotask(() => {
          flushScheduled = false;
          if (writeQueue) {
            void writePty(paneId, writeQueue);
            writeQueue = "";
          }
        });
      }
    });

    term.onResize(({ cols, rows }) => {
      if (!exited) void resizePty(paneId, cols, rows);
    });

    let unlisten: UnlistenFn | undefined;
    void listen<{ pane_id: string }>("pane-exit", (event) => {
      if (event.payload.pane_id === paneId) {
        exited = true;
        term.writeln(
          "\r\n\x1b[2m[process exited — press any key to start a new shell]\x1b[0m",
        );
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    void attach();

    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);

    return () => {
      disposed = true;
      observer.disconnect();
      unlisten?.();
      termRef.current = null;
      unregisterTerminal(paneId, term);
      // The PTY itself belongs to the Rust tree; unmount only drops the view.
      term.dispose();
    };
  }, [paneId]);

  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused]);

  return (
    <div
      className={`pane${focused ? " focused" : ""}${unread ? " unread" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
      ref={containerRef}
    />
  );
}
