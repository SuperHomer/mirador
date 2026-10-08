// Live Terminal instances by pane id, for automation round-trips
// (read-screen), scrollback persistence, and the agent wall, which borrows
// a pane's terminal to draw it somewhere else.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { storeScrollback } from "../bindings";

interface Entry {
  term: Terminal;
  serialize: SerializeAddon;
  fit: FitAddon;
  /** The pane's own container, where the terminal lives when not lent. */
  home: HTMLElement;
  /** The element the terminal is lent to, if it is. */
  borrower: HTMLElement | null;
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
  serialize: SerializeAddon,
  fit: FitAddon,
  home: HTMLElement,
) {
  const entry: Entry = {
    term,
    serialize,
    fit,
    home,
    borrower: null,
    dirty: false,
  };
  term.onWriteParsed(() => {
    entry.dirty = true;
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

/**
 * Moves the pane's live terminal into `host` — the same xterm, buffer and
 * PTY, not a copy — and sizes it (and so the PTY) to fit there. Returns
 * the function that gives it back, or null when there is no terminal to
 * lend or it is already lent elsewhere.
 *
 * Lending works because only one tab is on screen at a time: a terminal
 * drawn in the wall is never also wanted in its own tab, and the PTY has
 * one size, which belongs to whichever place is showing it.
 */
export function lendTerminal(
  paneId: string,
  host: HTMLElement,
): (() => void) | null {
  const entry = terminals.get(paneId);
  const element = entry?.term.element;
  if (!entry || !element || entry.borrower) return null;
  entry.borrower = host;
  host.appendChild(element);
  entry.fit.fit();
  changed(paneId);
  return () => {
    // The pane may have closed, or remounted with a new terminal, since.
    if (terminals.get(paneId) !== entry || entry.borrower !== host) return;
    entry.borrower = null;
    const el = entry.term.element;
    if (el) entry.home.appendChild(el);
    entry.fit.fit();
    changed(paneId);
  };
}

/** Refits the pane's terminal to wherever it is drawn now. */
export function fitTerminal(paneId: string) {
  terminals.get(paneId)?.fit.fit();
}

/**
 * The terminal holding keyboard focus, wherever it is drawn. In the wall,
 * that is not the workspace's focused pane (the wall is), so copy and
 * paste look here first.
 */
export function terminalWithFocus(): Terminal | undefined {
  const active = document.activeElement;
  if (!active) return undefined;
  for (const { term } of terminals.values()) {
    if (term.element?.contains(active)) return term;
  }
  return undefined;
}

export function registeredPanes(): string[] {
  return [...terminals.keys()];
}

/**
 * Persist the scrollback of every pane that changed since its last save
 * (30s tick + window blur). Serializing 10k lines blocks the renderer, so
 * idle panes — most of them, with many tabs open — are skipped.
 */
export function saveAllScrollbacks(maxLines = 10_000) {
  for (const [paneId, entry] of terminals) {
    if (!entry.dirty) continue;
    try {
      const data = entry.serialize.serialize({ scrollback: maxLines });
      entry.dirty = false;
      if (data) void storeScrollback(paneId, data);
    } catch {
      /* pane mid-teardown */
    }
  }
}

/**
 * The last `lines` lines of content (default: one screenful), as plain
 * text. Content ends at the cursor row — rows below it are unused padding.
 */
export function readScreenText(paneId: string, lines?: number | null): string {
  const term = terminals.get(paneId)?.term;
  if (!term) return "";
  const buf = term.buffer.active;
  const lastLine = buf.baseY + buf.cursorY;
  const count = lines && lines > 0 ? lines : term.rows;
  const out: string[] = [];
  for (let i = Math.max(0, lastLine + 1 - count); i <= lastLine; i++) {
    out.push(buf.getLine(i)?.translateToString(true) ?? "");
  }
  // Drop leading/trailing blank lines — agents care about content.
  while (out.length > 0 && out[0] === "") out.shift();
  while (out.length > 0 && out[out.length - 1] === "") out.pop();
  return out.join("\n");
}
