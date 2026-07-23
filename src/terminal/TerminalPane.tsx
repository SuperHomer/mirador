import { useEffect, useRef } from "react";
import { Channel } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { createTerminal, attachRenderer, applyConfig } from "./xtermFactory";
import { registerTerminal, unregisterTerminal } from "./registry";
import { useConfigStore } from "../state/configStore";
import {
  attachPane,
  writePty,
  resizePty,
  ackPty,
  focusPane,
  loadScrollback,
  PtyData,
} from "../bindings";

/** Panes whose scrollback was already restored this app session. */
const restoredScrollback = new Set<string>();

/** Ack processed output back to Rust every 256KB to release backpressure. */
const ACK_THRESHOLD = 256 * 1024;

interface Props {
  paneId: string;
  focused: boolean;
  unread: boolean;
  /** Set when this is a command pane (🤖): the command it runs. */
  agentCommand?: string;
  /** Set when this is a remote (SSH) pane: the destination host. */
  remoteHost?: string;
}

export function TerminalPane({
  paneId,
  focused,
  unread,
  agentCommand,
  remoteHost,
}: Props) {
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
    const serialize = new SerializeAddon();
    term.loadAddon(serialize);
    registerTerminal(paneId, term, serialize);
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
        // Previous session's scrollback, once per pane per app run.
        if (!restoredScrollback.has(paneId)) {
          const saved = await loadScrollback(paneId);
          // A disposed mount must go no further: attaching would re-sink
          // the PTY to a dead terminal (StrictMode remount race). It also
          // must not mark the pane restored — the live mount does that.
          if (disposed) return;
          restoredScrollback.add(paneId);
          if (saved) {
            term.write(saved);
            term.writeln("\r\n\x1b[2m──── session restored ────\x1b[0m");
          }
        }
        if (disposed) return;
        const status = await attachPane(paneId, term.cols, term.rows, channel);
        if (disposed) return;
        if (status === "restored") {
          // Command/remote pane from the previous session: idle until a
          // keypress — never auto-rerun or auto-reconnect on launch.
          exited = true;
          term.writeln(
            remoteHost
              ? `\x1b[2m[press any key to reconnect: ssh ${remoteHost}]\x1b[0m`
              : `\x1b[2m[press any key to rerun: ${agentCommand ?? "command"}]\x1b[0m`,
          );
          return;
        }
        const buf = term.buffer.active;
        if (
          status === "reattached" &&
          !agentCommand &&
          buf.cursorX === 0 &&
          buf.cursorY === 0
        ) {
          // Re-attached to a live shell with an empty screen (remount):
          // Ctrl-L makes it repaint the prompt. Never nudge command panes —
          // it would inject a byte into the running command's stdin.
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
    void listen<{
      pane_id: string;
      exit_code: number | null;
      is_command: boolean;
      is_remote: boolean;
    }>(
      "pane-exit",
      (event) => {
        if (event.payload.pane_id !== paneId) return;
        exited = true;
        const code = event.payload.exit_code;
        const status =
          code === null
            ? "exited"
            : event.payload.is_remote
              ? "disconnected"
              : code === 0
                ? "done ✓"
                : `exit ${code}`;
        const color =
          event.payload.is_remote || code === null
            ? "\x1b[2m"
            : code === 0
              ? "\x1b[32m"
              : "\x1b[31m";
        const hint = event.payload.is_remote
          ? "press any key to reconnect"
          : event.payload.is_command
            ? "press any key to rerun"
            : "press any key to start a new shell";
        term.writeln(
          `\r\n${color}[${status}]\x1b[0m \x1b[2m— ${hint}\x1b[0m`,
        );
      },
    ).then((fn) => {
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
    >
      <div className="pane-term" ref={containerRef} />
      {remoteHost ? (
        <div className="agent-chip remote-chip" title={`ssh ${remoteHost}`}>
          ⇅ {remoteHost}
        </div>
      ) : (
        agentCommand && (
          <div className="agent-chip" title={agentCommand}>
            🤖 {agentCommand}
          </div>
        )
      )}
    </div>
  );
}
