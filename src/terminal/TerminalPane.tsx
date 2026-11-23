import { memo, useEffect, useRef, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { FitAddon } from "@xterm/addon-fit";
import type { WebglAddon } from "@xterm/addon-webgl";
import { createTerminal, attachRenderer, applyConfig, cellWidth } from "./xtermFactory";
import { listenForImagePaste } from "./pasteImage";
import {
  fitTerminal,
  isLent,
  readSavedScrollback,
  reclaimTerminal,
  replayScrollback,
  registerTerminal,
  setPaneShown,
  unregisterTerminal,
  watchTerminal,
} from "./registry";
import { useConfigStore } from "../state/configStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { useAgentActivity } from "../agents/ActivityPanel";
import { AgentStatusBar } from "../agents/AgentStatusBar";
import { SubagentSheet } from "../agents/SubagentSheet";
import { useNow } from "../agents/time";
import { atFullSize, useBoardStore } from "../state/boardStore";
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
 * How long a pane on a background tab keeps its WebGL context. Chromium
 * allows ~16 live contexts per page and drops the oldest past that, so with
 * many panes open, idle ones give theirs back; a quick tab flip back and
 * forth still finds the renderer warm.
 */
const WEBGL_RELEASE_DELAY_MS = 30_000;
/**
 * On the board, many terminals are shown at once and the pan brings each
 * in and out of view: one that leaves gives its context back soon, or a
 * pan across the board would hold one per terminal it passed.
 */
const BOARD_WEBGL_RELEASE_DELAY_MS = 1_000;

/**
 * A remote agent's reconnects: the first wait, the longest, and how long
 * one must hold before the next loss starts over from the first.
 */
const RETRY_FIRST_MS = 2_000;
const RETRY_MAX_MS = 30_000;
const RETRY_HELD_MS = 60_000;

/**
 * Moves a background pane's terminal out of the viewport. Tabs stack with
 * `visibility: hidden`, which still counts as on-screen to the
 * IntersectionObserver xterm pauses rendering with — so every hidden pane
 * kept repainting its output. A transform leaves layout (and so fit and the
 * PTY size) untouched.
 */
const OFFSCREEN: React.CSSProperties = { transform: "translateX(-200vw)" };

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

/**
 * The size a saved scrollback with no size recorded is replayed at, and
 * the least Mirador writes its own lines at: xterm's own default.
 */
const WRITE_COLS = 80;
const WRITE_ROWS = 24;
/** No line of Mirador's is drawn wider than this, however long. */
const WRITE_COLS_MAX = 500;

/** Back to the normal screen, from the alternate one. */
const LEAVE_ALT_SCREEN = "\x1b[?1049l";

/** The columns `text` takes: its escape sequences none, its characters as
 *  wide as the terminal draws them (a CJK character two). */
function textWidth(text: string): number {
  return cellWidth(text.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, ""));
}

interface Props {
  paneId: string;
  focused: boolean;
  /** The pane's tab is the active one. */
  visible: boolean;
  /** Drawn as a card on the board. */
  onBoard: boolean;
  unread: boolean;
  /** Set when this is a command pane (🤖): the command it runs. */
  agentCommand?: string;
  /** Set when this is a remote (SSH) pane: the destination host. */
  remoteHost?: string;
}

// Memoized: every workspace snapshot re-renders the whole layout, and the
// props are primitives that rarely change for a given pane.
export const TerminalPane = memo(function TerminalPane({
  paneId,
  focused,
  visible,
  onBoard,
  unread,
  agentCommand,
  remoteHost,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<ReturnType<typeof createTerminal> | null>(null);
  const config = useConfigStore((s) => s.config);
  const webglRef = useRef<WebglAddon | null>(null);
  // Lent to the agent wall: drawn there while this pane is under it, so it
  // keeps its renderer as if it were on screen.
  const [lent, setLent] = useState(false);
  // For the registration below, which runs once per pane id.
  const visibleRef = useRef(visible);
  visibleRef.current = visible;

  useEffect(() => {
    setLent(isLent(paneId));
    return watchTerminal(paneId, () => setLent(isLent(paneId)));
  }, [paneId]);

  // The wall keeps a terminal it borrowed until the pane itself shows.
  useEffect(() => {
    if (visible && lent) reclaimTerminal(paneId);
  }, [visible, lent, paneId]);

  // Hot-reloaded config applies to the live terminal without recreating it,
  // refitted to a new font wherever it is drawn, unless a rail's thumbnail
  // keeps its size (`fitTerminal`). Given back, its pane fits it when shown.
  useEffect(() => {
    const term = termRef.current;
    if (term && config) {
      applyConfig(term, config);
      fitTerminal(paneId);
    }
  }, [config, paneId]);

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
    // Attaches started on this mount: a line of Mirador's waiting to be
    // written belongs to the one it was asked under (`say`).
    let attaches = 0;
    // A remote agent's pane reattaches by itself when ssh loses the
    // connection: after `retryDelay`, doubled each time a reconnect is
    // lost again soon, and back to the shortest once one has held. Not
    // "once an attach succeeds": ssh starts, and the attach succeeds,
    // even when it then fails to connect.
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let retryDelay = RETRY_FIRST_MS;
    let attachedAt = 0;

    const term = createTerminal(cfg);
    termRef.current = term;
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    // While a saved scrollback is replayed at its own size (`writeAt`),
    // nothing fits the terminal under it.
    let sizeHeld = false;
    /**
     * While no process backs the pane and it shows the alternate screen —
     * a restored agent's last screen, or a dead one's — the terminal is kept
     * at least this wide: the width Mirador wrote that screen and its own
     * lines at. The pane cuts it off at its edge.
     *
     * What is written at a narrow size stays that narrow on the alternate
     * screen, which xterm never rewraps, and xterm drops whatever lies past
     * the width the next time it widens. A pane that mounted narrow — the
     * window small at launch, a thin split — kept its restored screen and
     * "press any key to rerun" wrapped at two columns once it was wide.
     */
    let idleCols = 0;
    /**
     * While `idleCols` holds, the size the terminal would have without it:
     * its box's, at the last fit, or the size it had where nothing fits it
     * (a rail thumbnail keeps its terminal's size). A process that takes
     * the pane over gets this, never Mirador's width (`dropIdleWidth`).
     */
    let unfloored: { cols: number; rows: number } | null = null;
    /**
     * Sizes the terminal, and so its PTY, to the box it is drawn in — never
     * narrower than `idleCols`, and never to a box with no width or height.
     * That is a box not laid out (or under `display: none`), not a size:
     * FitAddon makes anything under three cells two columns by one row.
     * Whether it was sized.
     */
    const fit = {
      fit: (): boolean => {
        const box = term.element?.parentElement;
        if (sizeHeld || !box || box.clientWidth === 0 || box.clientHeight === 0) return false;
        const dims = fitAddon.proposeDimensions();
        if (!dims || isNaN(dims.cols) || isNaN(dims.rows)) return false;
        if (!idleCols) {
          fitAddon.fit();
          return true;
        }
        unfloored = { cols: dims.cols, rows: dims.rows };
        term.resize(Math.max(dims.cols, idleCols), dims.rows);
        return true;
      },
    };
    term.open(container);
    registerTerminal(paneId, term, fit, container, visibleRef.current);
    const stopImagePaste = listenForImagePaste(term, paneId);
    // The renderer is the visibility effect's job.
    fit.fit();

    const writeDone = (data: string) => new Promise<void>((resolve) => term.write(data, resolve));
    /**
     * Writes with the terminal `cols` × `rows`, then fits it to wherever it
     * is drawn now. Only before a PTY backs the pane, so no program sees the
     * size change.
     *
     * Straight to the box's size, not back through the size it had first:
     * an alternate screen has no scrollback, so a pane that mounted three
     * rows tall and grew while this wrote would have lost all but three of
     * its restored rows on the way. Where nothing fits it (a rail thumbnail
     * keeps its size; a box with no size), a normal screen goes back to the
     * size it had, rewrapping; an alternate one keeps this size, and its
     * process will get the old one (`unfloored`).
     */
    const writeAt = async (cols: number, rows: number, write: () => Promise<void>) => {
      const before = { cols: term.cols, rows: term.rows };
      sizeHeld = true;
      term.resize(cols, rows);
      try {
        await write();
      } finally {
        sizeHeld = false;
        if (!disposed && !fitTerminal(paneId)) {
          if (term.buffer.active.type === "alternate") unfloored ??= before;
          else term.resize(before.cols, before.rows);
        }
      }
    };
    /**
     * One of Mirador's own lines. Onto the alternate screen of a pane with
     * no process, at a width that holds it whole (`idleCols`); the normal
     * screen rewraps it anyway. What is still queued is drawn first: it may
     * change screens. A line an attach has overtaken meanwhile is dropped:
     * an exit hint still waiting when a key started the next process would
     * land on that process's screen.
     */
    const say = async (line: string) => {
      const asked = attaches;
      await writeDone("");
      if (disposed || asked !== attaches) return;
      if (exited && term.buffer.active.type === "alternate") {
        keepIdleWidth(Math.min(textWidth(line) + 1, WRITE_COLS_MAX));
      }
      return writeDone(`${line}\r\n`);
    };
    const keepIdleWidth = (cols: number) => {
      idleCols = Math.max(idleCols, cols);
      if (term.cols >= idleCols) return;
      // Directly, not by a fit: a rail thumbnail keeps its terminal's size.
      unfloored ??= { cols: term.cols, rows: term.rows };
      term.resize(idleCols, term.rows);
    };
    /**
     * A process is about to back the pane: the terminal takes its box's
     * size, or where nothing fits it, the size it had before Mirador widened
     * it. A rail thumbnail's reconnecting SSH agent otherwise started at the
     * width of its "reconnecting" line.
     */
    const dropIdleWidth = () => {
      if (!idleCols) return;
      idleCols = 0;
      const back = unfloored;
      unfloored = null;
      if (!fitTerminal(paneId) && back) term.resize(back.cols, back.rows);
    };

    /**
     * One output channel per attach, never shared between them. Rust owns the
     * `Channel` it was handed, so any attach that returns without parking it
     * in the PTY's sink — a session-restored pane, an error — drops it, and a
     * dropped channel tells the webview to unregister the callback for good.
     * Reusing that object would send the next spawn's output into a void: the
     * process runs, the pane stays frozen.
     *
     * Each channel acks its own output, whatever the attach is doing. Rust
     * counts the bytes it hands a channel and starts over at zero for a new
     * one, so what a channel delivered is that channel's to ack: output that
     * arrives before its attach resolves (a holder's replay, a first prompt)
     * included. Dropped instead, it stays counted in Rust for good, and past
     * 512 KB of such debt the pane freezes the next time it pauses.
     */
    const newChannel = () => {
      const channel = new Channel<PtyData>();
      // Delivered and drawn, not yet acked.
      let pendingAck = 0;
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
          // After an exit too: Rust ignores a pane it no longer has.
          if (pendingAck >= ACK_THRESHOLD) {
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
      attaches++;
      clearTimeout(retryTimer);
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
            // Replayed at the size it was saved at, the only one its
            // serialized form is right at (`saveAllScrollbacks`), not at the
            // size the pane mounted at; Mirador's line after it at least as
            // wide; then the pane's own size (`writeAt`, `idleCols`).
            const { data, size } = readSavedScrollback(saved);
            const cols = Math.max(size?.cols ?? 0, term.cols, WRITE_COLS);
            const rows = Math.max(size?.rows ?? 0, term.rows, WRITE_ROWS);
            await writeAt(cols, rows, async () => {
              await replayScrollback(term, data, size);
              if (disposed) return;
              term.resize(cols, rows);
              // A shell starts anew here, and on the normal screen, as in
              // any terminal. Left on the alternate screen a restored agent
              // had, it drew its prompt there, over that agent's last
              // screen, with no scrollback and never rewrapped. A command
              // or SSH pane comes back idle and keeps its last screen on
              // show until a key starts it again.
              if (!agentCommand && !remoteHost && term.buffer.active.type === "alternate") {
                await writeDone(LEAVE_ALT_SCREEN);
              }
              await writeDone(`${IDLE_MODE_RESET}\r\n\x1b[2m──── session restored ────\x1b[0m\r\n`);
              if (term.buffer.active.type === "alternate") idleCols = cols;
            });
            if (idleCols) keepIdleWidth(idleCols);
          }
        }
        if (disposed) return;
        // A key starts the pane's command or shell again: on the normal
        // screen, as any new process starts, not under the last one's.
        if (rerun) {
          if (term.buffer.active.type === "alternate") await writeDone(LEAVE_ALT_SCREEN);
          dropIdleWidth();
        }
        // Again after every wait: a pane closed meanwhile must not attach,
        // or Rust, finding no pane, would start a shell for nobody.
        if (disposed) return;
        // Whatever this attach finds — a live holder included — gets the
        // pane's own size, never the width an idle screen is held at.
        const size = idleCols && unfloored ? unfloored : { cols: term.cols, rows: term.rows };
        const status = await attachPane(
          paneId,
          size.cols,
          size.rows,
          newChannel(),
          rerun,
        );
        if (disposed) return;
        if (status === "restored") {
          // Command/remote pane from the previous session: idle until a
          // keypress — never auto-rerun or auto-reconnect on launch.
          await say(
            remoteHost
              ? `\x1b[2m[press any key to reconnect: ssh ${remoteHost}]\x1b[0m`
              : `\x1b[2m[press any key to rerun: ${agentCommand ?? "command"}]\x1b[0m`,
          );
          return;
        }
        // A process backs the pane now, at the size it was given.
        dropIdleWidth();
        // Not before: until the PTY exists, every keystroke written to it is
        // silently dropped.
        exited = false;
        attachedAt = Date.now();
        // Resizes while attaching were dropped (no PTY to take them), and
        // the layout often settles in exactly that window — right after a
        // split. A shell left at the old width redraws its line for a
        // width the terminal is not drawing: typed text lands a column off
        // and the prompt is overwritten. Say the real size once more; a
        // resize to the size it already has signals nothing.
        void resizePty(paneId, term.cols, term.rows);
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
        void say(`\x1b[31mfailed to attach shell: ${err}\x1b[0m`);
      } finally {
        attaching = false;
      }
    };

    // One write in flight per pane; input arriving meanwhile is batched
    // into the next. `write_pty` runs off the UI thread, where concurrent
    // calls could land out of order — awaiting each keeps keystrokes in
    // sequence, and under load typing coalesces instead of queueing up one
    // IPC call per key.
    let writeQueue = "";
    let writing = false;
    const flushWrites = async () => {
      try {
        while (writeQueue) {
          const data = writeQueue;
          writeQueue = "";
          try {
            await writePty(paneId, data);
          } catch {
            // Pane exited mid-write; pane-exit reports it.
          }
        }
      } finally {
        writing = false;
      }
    };
    term.onData((d) => {
      // An idle pane has no PTY to take this. Re-running is onKey's job:
      // onData also carries mouse and focus reports, so a stray mouse move
      // over a restored pane must not count as "any key".
      if (exited) return;
      writeQueue += d;
      if (!writing) {
        writing = true;
        // Same-tick bursts still go out as one call.
        queueMicrotask(() => void flushWrites());
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
      reconnects: boolean;
    }>(
      "pane-exit",
      (event) => {
        if (event.payload.pane_id !== paneId) return;
        exited = true;
        // A process killed mid-run never got to reset its own modes; with
        // mouse tracking left armed the dead pane can't even be selected.
        term.write(IDLE_MODE_RESET);
        const code = event.payload.exit_code;
        // For a remote agent, 255 is ssh's own failure, the connection
        // lost; the agent runs on under its holder there. Any other code
        // is the attach's: the agent's own exit, or 1 when the session is
        // gone, which it has just said.
        const lost = event.payload.reconnects
          ? code === 255
          : event.payload.is_remote;
        const status =
          code === null
            ? "exited"
            : lost
              ? "disconnected"
              : code === 0
                ? "done ✓"
                : `exit ${code}`;
        const color =
          lost || code === null
            ? "\x1b[2m"
            : code === 0
              ? "\x1b[32m"
              : "\x1b[31m";
        let wait = 0;
        if (event.payload.reconnects && lost) {
          if (Date.now() - attachedAt > RETRY_HELD_MS) {
            retryDelay = RETRY_FIRST_MS;
          }
          wait = retryDelay;
          retryDelay = Math.min(retryDelay * 2, RETRY_MAX_MS);
          retryTimer = setTimeout(() => void attach(true), wait);
        }
        const hint = wait
          ? `reconnecting in ${wait / 1000}s (press any key to retry now)`
          : event.payload.is_remote
            ? "press any key to reconnect"
            : event.payload.is_command
              ? "press any key to rerun"
              : "press any key to start a new shell";
        void say(`\r\n${color}[${status}]\x1b[0m \x1b[2m— ${hint}\x1b[0m`);
      },
    ).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    void attach();

    // Lent, the terminal is sized by whoever borrowed it, or kept as it is
    // by a wall thumbnail: fitting it from here would size it to the
    // borrower's box.
    const observer = new ResizeObserver(() => {
      if (!isLent(paneId)) fit.fit();
    });
    observer.observe(container);

    return () => {
      disposed = true;
      clearTimeout(retryTimer);
      observer.disconnect();
      unlisten?.();
      termRef.current = null;
      webglRef.current = null;
      stopImagePaste();
      unregisterTerminal(paneId, term);
      // The PTY itself belongs to the Rust tree; unmount only drops the view.
      term.dispose();
    };
  }, [paneId]);

  // Tells the registry whether this pane is on screen: a terminal the wall
  // gives back is sized to it only then, and once it shows again. After the
  // registration above, which React runs first.
  useEffect(() => {
    setPaneShown(paneId, visible);
  }, [visible, paneId]);

  // On the board every terminal counts as visible, and a browser allows
  // ~16 WebGL contexts. So there a terminal renders with WebGL only while
  // it is in view and the board is at 100%; otherwise xterm falls back to
  // its DOM renderer, which pauses while out of view and, being text,
  // stays crisp when the board scales it. At 100% the screen holds far
  // fewer than 16 cards. In view is where the terminal is drawn — its
  // card, or the tile of a wall on the board that holds it — so it is the
  // terminal's own element that is watched, which moves with it.
  const [inView, setInView] = useState(true);
  useEffect(() => {
    const el = termRef.current?.element ?? containerRef.current;
    if (!onBoard || !el) {
      setInView(true);
      return;
    }
    // Against the window, after the board's own clipping.
    const observer = new IntersectionObserver(([e]) => setInView(e.isIntersecting));
    observer.observe(el);
    return () => observer.disconnect();
  }, [onBoard]);
  const fullSize = useBoardStore((s) => atFullSize(s.camera));

  const onScreen = (lent || visible) && (!onBoard || (inView && fullSize));
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if (onScreen) {
      webglRef.current ??= attachRenderer(term, () => {
        webglRef.current = null;
      });
      return;
    }
    const timer = setTimeout(
      () => {
        webglRef.current?.dispose();
        webglRef.current = null;
      },
      onBoard ? BOARD_WEBGL_RELEASE_DELAY_MS : WEBGL_RELEASE_DELAY_MS,
    );
    return () => clearTimeout(timer);
  }, [onScreen, onBoard, paneId]);

  // An agent shown as a split gets its status bar, and its subagent sheet
  // when asked; on the board and on the wall, others draw it. Only then is
  // its activity read. Under its tab's wall a pane is not shown, whether or
  // not the wall borrowed its terminal: a bar there would only resize a
  // terminal nobody sees, each time its text wrapped.
  const agent = useWorkspaceStore((s) => s.snapshot?.agents.find((a) => a.paneId === paneId));
  const walled = useWorkspaceStore(
    (s) => !!agent && !!s.snapshot?.tabs.find((t) => t.id === agent.tabId)?.wall,
  );
  const withBar = !!agent && visible && !onBoard && !lent && !walled;
  const { activity } = useAgentActivity(paneId, withBar && !!agent?.hasActivity);
  const now = useNow(withBar);
  const [sheetOpen, setSheetOpen] = useState(false);

  // Also on becoming visible, and on coming back from the wall: showing an
  // agent alone from its tile means typing to it next.
  useEffect(() => {
    if (focused && visible && !lent) termRef.current?.focus();
  }, [focused, visible, lent]);

  return (
    <div
      className={`pane${focused ? " focused" : ""}${unread ? " unread" : ""}${withBar ? " with-bar" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      {/* Always here, agent or not: a parent that came and went with the
          bar would remount the terminal's container. The sheet lies over
          the terminal in it, beside the element the wall borrows. */}
      <div className="pane-main">
        <div
          className="pane-term"
          ref={containerRef}
          style={visible ? undefined : OFFSCREEN}
        />
        {withBar && sheetOpen && (
          <SubagentSheet
            activity={activity}
            now={now}
            onClose={() => setSheetOpen(false)}
          />
        )}
      </div>
      {withBar && agent && (
        <AgentStatusBar
          agent={agent}
          activity={activity}
          now={now}
          sheetOpen={sheetOpen}
          onToggleSheet={() => setSheetOpen(!sheetOpen)}
        />
      )}
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
});
