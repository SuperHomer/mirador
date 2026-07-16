// Live Terminal instances by pane id, for automation round-trips
// (read-screen) that need direct buffer access.
import { Terminal } from "@xterm/xterm";

const terminals = new Map<string, Terminal>();

export function registerTerminal(paneId: string, term: Terminal) {
  terminals.set(paneId, term);
}

export function unregisterTerminal(paneId: string, term: Terminal) {
  if (terminals.get(paneId) === term) terminals.delete(paneId);
}

export function getTerminal(paneId: string): Terminal | undefined {
  return terminals.get(paneId);
}

export function registeredPanes(): string[] {
  return [...terminals.keys()];
}

/**
 * The last `lines` lines of content (default: one screenful), as plain
 * text. Content ends at the cursor row — rows below it are unused padding.
 */
export function readScreenText(paneId: string, lines?: number | null): string {
  const term = terminals.get(paneId);
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
