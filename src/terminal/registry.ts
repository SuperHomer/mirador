// Live Terminal instances by pane id, for automation round-trips
// (read-screen), scrollback persistence, and the agent wall, which borrows
// a pane's terminal to draw it somewhere else.
import type { IBuffer, IBufferCell, IBufferLine } from "@xterm/xterm";
import { Terminal } from "@xterm/xterm";
import { SerializeAddon } from "@xterm/addon-serialize";
import { storeScrollback } from "../bindings";

/** Sizes the terminal to wherever it is drawn: the pane's own fit, which
 *  knows when not to (`TerminalPane`), and says whether it did. */
export interface Fitter {
  fit(): boolean;
}

interface Entry {
  term: Terminal;
  serialize: SerializeAddon;
  fit: Fitter;
  /** The pane's own container, where the terminal lives when not lent. */
  home: HTMLElement;
  /** The element the terminal is lent to, if it is. */
  borrower: HTMLElement | null;
  /** Lent with `fit: false`: the borrower keeps it at the size it has. */
  keepsSize: boolean;
  /** The pane's own container is on screen (`setPaneShown`): only then is
   *  a terminal given back sized to it. */
  shown: boolean;
  /** Output arrived since the last save. */
  dirty: boolean;
}

const terminals = new Map<string, Entry>();
const listeners = new Map<string, Set<() => void>>();

function changed(paneId: string) {
  listeners.get(paneId)?.forEach((fn) => fn());
}

/**
 * Calls `fn` whenever the pane's terminal is registered, unregistered,
 * lent or returned. Returns the unsubscribe.
 */
export function watchTerminal(paneId: string, fn: () => void): () => void {
  let set = listeners.get(paneId);
  if (!set) listeners.set(paneId, (set = new Set()));
  set.add(fn);
  return () => {
    set.delete(fn);
    if (set.size === 0) listeners.delete(paneId);
  };
}

export function registerTerminal(
  paneId: string,
  term: Terminal,
  fit: Fitter,
  home: HTMLElement,
  shown: boolean,
) {
  // Saves only, so not loaded into the terminal: it reads it through
  // `clippedToWidth`.
  const serialize = new SerializeAddon();
  serialize.activate(clippedToWidth(term));
  const entry: Entry = {
    term,
    serialize,
    fit,
    home,
    borrower: null,
    keepsSize: false,
    shown,
    dirty: false,
  };
  term.onWriteParsed(() => {
    entry.dirty = true;
  });
  // A save is replayed at the size it records. An alternate screen saved
  // while the window was small would come back small; saved again after a
  // resize, it comes back the size it has now. The normal screen rewraps
  // from any size, so a resize alone does not save it again.
  term.onResize(() => {
    if (term.buffer.active.type === "alternate") entry.dirty = true;
  });
  terminals.set(paneId, entry);
  changed(paneId);
}

export function unregisterTerminal(paneId: string, term: Terminal) {
  if (terminals.get(paneId)?.term === term) {
    terminals.delete(paneId);
    changed(paneId);
  }
}

export function getTerminal(paneId: string): Terminal | undefined {
  return terminals.get(paneId)?.term;
}

export function isLent(paneId: string): boolean {
  return !!terminals.get(paneId)?.borrower;
}

export function isLentTo(paneId: string, host: HTMLElement): boolean {
  return terminals.get(paneId)?.borrower === host;
}

/**
 * Moves the pane's live terminal into `host` — the same xterm, buffer and
 * PTY, not a copy — and sizes it (and so the PTY) to fit there. Returns
 * the function that gives it back, or null when there is no terminal to
 * lend or it is already lent elsewhere.
 *
 * Lending works because the wall is drawn over its tab's splits: a
 * terminal drawn in the wall is never also wanted in its own pane, and the
 * PTY has one size, which belongs to whichever place is showing it.
 *
 * A lent terminal stays lent while the wall's tab is hidden, until the
 * wall goes and its pane shows and takes it back (`reclaimTerminal`).
 * Every move resizes the PTY, and Claude Code redraws on each resize; a
 * narrowing one leaves rewrapped fragments and reprinted banners behind.
 * Returning every agent whenever the wall hid resized all of them twice
 * per visit, which is what made tiles fill with debris.
 *
 * `fit: false` lends it at the size it has: a wall's mini tile scales the
 * terminal down to fit instead. Focus moves between agents often, and
 * resizing the PTY each time would reflow the agent's screen into a
 * thumbnail's few columns and back. Giving it back fits it to its own pane
 * only while that pane is on screen. Under the wall it is not: fitting
 * there sized the PTY to a split nobody saw, so an agent shown large and
 * then moved to the rail came back at its pane's width, three columns for
 * a much-split pane, and redrew once more for nothing. Its pane fits it
 * when it shows (`setPaneShown`).
 */
export function lendTerminal(
  paneId: string,
  host: HTMLElement,
  { fit = true }: { fit?: boolean } = {},
): (() => void) | null {
  const entry = terminals.get(paneId);
  const element = entry?.term.element;
  if (!entry || !element || entry.borrower) return null;
  entry.borrower = host;
  entry.keepsSize = !fit;
  host.appendChild(element);
  if (fit) entry.fit.fit();
  changed(paneId);
  return () => {
    // The pane may have closed, or remounted with a new terminal, since.
    if (terminals.get(paneId) !== entry || entry.borrower !== host) return;
    entry.borrower = null;
    entry.keepsSize = false;
    const el = entry.term.element;
    if (el) entry.home.appendChild(el);
    if (entry.shown) entry.fit.fit();
    changed(paneId);
  };
}

/**
 * Whether the pane's own container is on screen — not under its tab's
 * wall, not behind a zoomed pane, its tab active. Shown again with its
 * terminal at home, the terminal is fitted to it now: a give-back while it
 * was hidden left it at the size it had elsewhere, and a hidden pane is
 * moved off screen by a transform, so no resize will come to fit it.
 */
export function setPaneShown(paneId: string, shown: boolean) {
  const entry = terminals.get(paneId);
  if (!entry || entry.shown === shown) return;
  entry.shown = shown;
  if (shown && !entry.borrower) entry.fit.fit();
}

/**
 * Brings a lent terminal back to its own pane: the pane is on screen now.
 * The borrower's release then does nothing; the wall lends it again the
 * next time it is shown.
 */
export function reclaimTerminal(paneId: string) {
  const entry = terminals.get(paneId);
  if (!entry?.borrower) return;
  entry.borrower = null;
  entry.keepsSize = false;
  const el = entry.term.element;
  if (el) entry.home.appendChild(el);
  entry.fit.fit();
  changed(paneId);
}

/**
 * Refits the pane's terminal to wherever it is drawn now, unless it is
 * lent to a tile that keeps its size (`fit: false`): fitted to that tile,
 * it would take the tile's box, which is the terminal itself less its
 * scrollbar, and lose two columns each time. Whether it was fitted.
 */
export function fitTerminal(paneId: string): boolean {
  const entry = terminals.get(paneId);
  return !!entry && !entry.keepsSize && entry.fit.fit();
}

/**
 * The terminal holding keyboard focus, wherever it is drawn. In the wall,
 * that need not be the workspace's focused pane, so copy and paste look
 * here first.
 */
export function terminalWithFocus(): Terminal | undefined {
  const active = document.activeElement;
  return active ? terminalAt(active) : undefined;
}

/**
 * The pane whose terminal holds keyboard focus, wherever it is drawn —
 * the pane `terminalWithFocus` belongs to. On the agent wall, pane
 * actions act on it rather than on the tab's focused pane, for the same
 * reason copy and paste look here first; elsewhere they do not ask it
 * (see `actionPane`).
 */
export function paneWithFocus(): string | undefined {
  const active = document.activeElement;
  if (!active) return undefined;
  for (const [pane, { term }] of terminals) {
    if (term.element?.contains(active)) return pane;
  }
  return undefined;
}

/** The terminal drawn where `el` is, wherever that is: its pane or a wall tile. */
export function terminalAt(el: Element): Terminal | undefined {
  for (const { term } of terminals.values()) {
    if (term.element?.contains(el)) return term;
  }
  return undefined;
}

export function registeredPanes(): string[] {
  return [...terminals.keys()];
}

/**
 * Ahead of a saved scrollback, the size it was taken at: an OSC no
 * terminal acts on, so a version that does not know it, reading the file,
 * writes it into xterm harmlessly.
 */
const SIZE_RECORD = "\x1b]5379;mirador-size=";

/**
 * Persist the scrollback of every pane that changed since its last save
 * (30s tick + window blur). Serializing 10k lines blocks the renderer, so
 * idle panes — most of them, with many tabs open — are skipped.
 *
 * Each save starts with the size it was taken at (`scrollbackRecord`):
 * what the serializer writes is only right at that size. It marks a line
 * that wrapped by filling the row to its last column and letting the
 * terminal wrap, and places the cursor for that width, so replayed at
 * another width it leaves runs of dashes and misplaced text behind; and a
 * restored alternate screen, which xterm never rewraps, keeps whatever
 * width it was replayed at.
 */
export function saveAllScrollbacks(maxLines = 10_000) {
  for (const [paneId, entry] of terminals) {
    if (!entry.dirty) continue;
    try {
      const data = entry.serialize.serialize({ scrollback: maxLines });
      entry.dirty = false;
      if (data) void storeScrollback(paneId, scrollbackRecord(entry.term) + data);
    } catch {
      /* pane mid-teardown */
    }
  }
}

/**
 * `term` as a save reads it: no line longer than the terminal is wide. An
 * alternate screen narrowed under its content keeps the cells past the new
 * width (xterm never rewraps that screen, and clearing it erases only up to
 * the width), and the serializer writes each line to its own length. Those
 * cells were not on screen at the size the save records, and replayed at it
 * they wrapped onto the rows below.
 *
 * A double-width character cut by the edge — its first half in the last
 * column, its second past it — reads as a blank cell: written whole, it
 * would not fit, and would wrap to a row of its own.
 */
export function clippedToWidth(term: Terminal): Terminal {
  const bound = (target: object, key: string | symbol) => {
    const value: unknown = Reflect.get(target, key);
    return typeof value === "function" ? value.bind(target) : value;
  };
  const line = (l: IBufferLine | undefined, b: IBuffer) => {
    if (!l || l.length <= term.cols) return l;
    const edge = term.cols - 1;
    const cut = l.getCell(edge)?.getWidth() === 2;
    // The cut character's cell emptied, keeping its colours, and one column
    // wide: a zero-wide cell is, to the serializer, a wide character's
    // second half. On a cell of its own, since the serializer reuses the one
    // it passes; that one is filled too, with the same colours, because the
    // serializer sometimes reads colours from it and not from the return.
    const blank = (cell?: IBufferCell) => {
      l.getCell(edge, cell);
      const own = l.getCell(edge, b.getNullCell());
      const empty: Record<string | symbol, () => number | string> = {
        getWidth: () => 1,
        getChars: () => "",
        getCode: () => 0,
      };
      return own && new Proxy(own, { get: (c, k) => empty[k] ?? bound(c, k) });
    };
    return new Proxy(l, {
      get: (t, k) => {
        if (k === "length") return term.cols;
        if (k === "getCell" && cut) {
          return (x: number, cell?: IBufferCell) => (x === edge ? blank(cell) : t.getCell(x, cell));
        }
        return bound(t, k);
      },
    });
  };
  const buffer = (b: IBuffer) =>
    new Proxy(b, { get: (t, k) => (k === "getLine" ? (y: number) => line(t.getLine(y), t) : bound(t, k)) });
  const buffers = new Proxy(term.buffer, {
    get: (t, k) => (k === "normal" || k === "alternate" || k === "active" ? buffer(t[k]) : bound(t, k)),
  });
  return new Proxy(term, { get: (t, k) => (k === "buffer" ? buffers : bound(t, k)) });
}

/** The size a save was taken at: the terminal's. */
export interface SavedSize {
  cols: number;
  rows: number;
}

/** The record of the size `term` is saved at, to go ahead of the save. */
export function scrollbackRecord(term: Terminal): string {
  return `${SIZE_RECORD}${term.cols}x${term.rows}\x07`;
}

/** A saved scrollback, and the size it was taken at when the save says. */
export function readSavedScrollback(saved: string): { data: string; size: SavedSize | null } {
  const end = saved.startsWith(SIZE_RECORD) ? saved.indexOf("\x07") : -1;
  if (end < 0) return { data: saved, size: null };
  const data = saved.slice(end + 1);
  const match = /^(\d{1,4})x(\d{1,4})$/.exec(saved.slice(SIZE_RECORD.length, end));
  const cols = Number(match?.[1]);
  const rows = Number(match?.[2]);
  // A size no terminal had is no size: the data is replayed as it is.
  return match && cols >= 2 && rows >= 1 ? { data, size: { cols, rows } } : { data, size: null };
}

/**
 * Writes a save into `term` at the size it was taken at, both screens; with
 * no size, as it is, at the terminal's size now.
 */
export async function replayScrollback(term: Terminal, data: string, size: SavedSize | null) {
  if (size) term.resize(size.cols, size.rows);
  return new Promise<void>((resolve) => term.write(data, resolve));
}

/**
 * The last `lines` lines of content (default: one screenful), as plain
 * text. Content ends at the cursor row or the screen's last non-blank row,
 * whichever is lower. A shell's cursor is on its last line, and the blank
 * rows below are padding; but a full-screen TUI (Pi, OpenCode) parks its
 * cursor in its input box, and draws the rest of the box and a footer
 * below it — rows a read ending at the cursor dropped, so the screen check
 * found no input box and refused every message to it.
 *
 * With it, the cursor as [row, column], the row indexing the lines of
 * `text` — or null when its row is not among them. Whether an agent's
 * input box holds a draft is told by where its cursor is.
 */
export function readScreenText(
  paneId: string,
  lines?: number | null,
): { text: string; cursor: [number, number] | null; afterCursor: string } {
  const term = terminals.get(paneId)?.term;
  if (!term) return { text: "", cursor: null, afterCursor: "" };
  const buf = term.buffer.active;
  const cursorLine = buf.baseY + buf.cursorY;
  let lastLine = cursorLine;
  for (let i = buf.baseY + term.rows - 1; i > lastLine; i--) {
    if (buf.getLine(i)?.translateToString(true).trim()) {
      lastLine = i;
      break;
    }
  }
  const count = lines && lines > 0 ? lines : term.rows;
  let first = Math.max(0, lastLine + 1 - count);
  const out: string[] = [];
  for (let i = first; i <= lastLine; i++) {
    out.push(buf.getLine(i)?.translateToString(true) ?? "");
  }
  // Drop leading/trailing blank lines — agents care about content.
  while (out.length > 0 && out[0] === "") {
    out.shift();
    first++;
  }
  while (out.length > 0 && out[out.length - 1] === "") out.pop();
  const row = cursorLine - first;
  const cursor: [number, number] | null =
    row >= 0 && row < out.length ? [row, buf.cursorX] : null;
  // The cursor's row from the cursor on, dim cells blanked: an agent's
  // placeholder is dim, what the person typed is not (`has_draft`).
  let afterCursor = "";
  const line = cursor ? buf.getLine(cursorLine) : undefined;
  const cell = buf.getNullCell();
  for (let x = buf.cursorX; line && x < line.length; x++) {
    if (!line.getCell(x, cell) || cell.getWidth() === 0) continue;
    afterCursor += cell.isDim() ? " " : cell.getChars() || " ";
  }
  return { text: out.join("\n"), cursor, afterCursor };
}
