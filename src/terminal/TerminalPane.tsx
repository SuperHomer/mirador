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

/**
 * Input modes a serialized buffer replays onto a pane whose process is gone.
 * Mouse tracking is the harmful one: xterm turns off selection and reports
 * every mouse move as input, which an idle pane reads as "a key was pressed".
 * Focus reporting and bracketed paste manufacture input the same way, and app
 * cursor keys would misdirect the arrows. The screen buffer (?1049) is left
 * alone on purpose — the restored screen is what the user wants to look at.
 */
const IDLE_MODE_RESET =
  "\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l" + // mouse tracking
  "\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l" + // mouse encodings
  "\x1b[?1004l" + // focus reporting
  "\x1b[?2004l" + // bracketed paste
  "\x1b[?1l" + // application cursor keys
  "\x1b[?25h"; // cursor visible

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
    // True whenever no live PTY backs the pane: before the first attach,
    // after an exit, and for a session-restored command pane. Input is
    // dropped while it holds — there is nothing to write to.
    let exited = true;
    let attaching = false;
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

    /**
     * One output channel per attach, never shared between them. Rust owns the
     * `Channel` it was handed, so any attach that returns without parking it
     * in the PTY's sink — a session-restored pane, an error — drops it, and a
     * dropped channel tells the webview to unregister the callback for good.
     * Reusing that object would send the next spawn's output into a void: the
     * process runs, the pane stays frozen.
     */
    const newChannel = () => {
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
      return channel;
    };

    /** `rerun`: this attach came from a keypress, so it may start the command. */
    const attach = async (rerun = false) => {
      // One in-flight attach at a time: a second would race the spawn and
      // lose to "pane already running".
      if (attaching) return;
      attaching = true;
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
            term.write(IDLE_MODE_RESET);
            term.writeln("\r\n\x1b[2m──── session restored ────\x1b[0m");
          }
        }
        if (disposed) return;
        const status = await attachPane(
          paneId,
          term.cols,
          term.rows,
          newChannel(),
          rerun,
        );
        if (disposed) return;
        if (status === "restored") {
          // Command/remote pane from the previous session: idle until a
          // keypress — never auto-rerun or auto-reconnect on launch.
          term.writeln(
            remoteHost
              ? `\x1b[2m[press any key to reconnect: ssh ${remoteHost}]\x1b[0m`
              : `\x1b[2m[press any key to rerun: ${agentCommand ?? "command"}]\x1b[0m`,
          );
          return;
        }
        // Not before: until the PTY exists, every keystroke written to it is
        // silently dropped.
        exited = false;
        pendingAck = 0;
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
      } finally {
        attaching = false;
      }
    };

    // Batch same-tick keystroke bursts (e.g. paste) into one IPC call.
    let writeQueue = "";
    let flushScheduled = false;
    term.onData((d) => {
      // An idle pane has no PTY to take this. Re-running is onKey's job:
      // onData also carries mouse and focus reports, so a stray mouse move
      // over a restored pane must not count as "any key".
      if (exited) return;
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

    // "Press any key to rerun/reconnect" — a real keypress only.
    term.onKey(() => {
      if (exited) void attach(true);
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
        // A process killed mid-run never got to reset its own modes; with
        // mouse tracking left armed the dead pane can't even be selected.
        term.write(IDLE_MODE_RESET);
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
