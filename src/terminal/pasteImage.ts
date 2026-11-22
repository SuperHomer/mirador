// An image pasted into a pane is typed as a path, the way a dropped file
// is: Rust saves it as a file, and the quoted path goes in as a paste, so
// an agent like Claude Code attaches it.
import type { Terminal } from "@xterm/xterm";
import { pasteImage } from "../bindings";
import { reportFailure } from "../state/uiStore";

const isWindows = navigator.platform.toUpperCase().startsWith("WIN");

/**
 * A dropped or pasted path, quoted to stay in one piece. Elsewhere than
 * Windows, POSIX single quotes, each `'` closed and escaped: one literal
 * word. On Windows, double quotes (a name cannot contain `"`) only keep a
 * path with spaces together; the shell still expands what is inside — `$`
 * and backticks in PowerShell and Git Bash, `%…%` (and `!…!` with delayed
 * expansion) in cmd. Those are rare in paths, and quoting each shell its
 * own way would mean knowing which shell each pane runs.
 */
export function quotePath(path: string): string {
  if (isWindows) return `"${path}"`;
  return `'${path.replaceAll("'", `'\\''`)}'`;
}

/**
 * Saves `image` for `pane` and types its path into `term`. Rust decides
 * where the file goes from the pane, and refuses what is not an image or
 * is too large; that shows as any failed action does.
 */
export function typeImagePath(term: Terminal, pane: string, image: Blob) {
  void image
    .arrayBuffer()
    .then((bytes) => pasteImage(pane, bytes))
    .then((path) => term.paste(quotePath(path) + " "))
    .catch((e) => reportFailure("Paste image", e));
}

/**
 * Pastes an image into `pane` when the clipboard holds one and no text;
 * any other paste is left to xterm, exactly as before. An image copied
 * from a web page usually comes with text too, and pastes that text: a
 * paste with text in it always pastes the text.
 *
 * On `term.element`, in the capture phase: before xterm's own handler on
 * its textarea, and on the element that moves with the terminal when the
 * agent wall borrows it. Returns the listener's removal.
 */
export function listenForImagePaste(term: Terminal, pane: string): () => void {
  const el = term.element;
  if (!el) return () => {};
  const onPaste = (e: ClipboardEvent) => {
    const data = e.clipboardData;
    if (!data || data.getData("text/plain")) return;
    const image = Array.from(data.files).find((f) => f.type.startsWith("image/"));
    if (!image) return;
    e.preventDefault();
    e.stopPropagation();
    typeImagePath(term, pane, image);
  };
  el.addEventListener("paste", onPaste, true);
  return () => el.removeEventListener("paste", onPaste, true);
}

/**
 * The paste action's (Ctrl+Shift+V's) way in: the clipboard's text if it
 * has any, as before, and only without text an image from it.
 */
export async function pasteFromClipboard(term: Terminal, pane: string | undefined) {
  // Through xterm, not straight to the PTY: it applies bracketed paste (a
  // multi-line paste must not run line by line) and newline normalization,
  // and its input path keeps the paste in order with typing.
  const text = await navigator.clipboard.readText().catch(() => "");
  if (text) {
    term.paste(text);
    return;
  }
  if (!pane) return;
  const items = await navigator.clipboard.read().catch(() => []);
  for (const item of items) {
    const type = item.types.find((t) => t.startsWith("image/"));
    if (type) {
      typeImagePath(term, pane, await item.getType(type));
      return;
    }
  }
}
