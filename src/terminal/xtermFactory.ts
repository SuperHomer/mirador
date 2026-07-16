import { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ResolvedConfig } from "../bindings";
import { toXtermTheme, checkFontAvailable } from "../state/configStore";
import "@xterm/xterm/css/xterm.css";

export function createTerminal(config: ResolvedConfig): Terminal {
  checkFontAvailable(config.fontFamily);
  const term = new Terminal({
    cursorBlink: true,
    allowProposedApi: true,
    scrollback: config.scrollback,
    fontFamily: config.fontFamily,
    fontSize: config.fontSize,
    macOptionIsMeta: true,
    theme: toXtermTheme(config),
  });

  // OSC 52: let programs in the terminal read/write the system clipboard.
  term.loadAddon(new ClipboardAddon());
  // Clickable URLs, opened in the system browser (not the webview).
  term.loadAddon(
    new WebLinksAddon((event, uri) => {
      event.preventDefault();
      void openUrl(uri);
    }),
  );

  return term;
}

/** Applies a hot-reloaded config to a live terminal. */
export function applyConfig(term: Terminal, config: ResolvedConfig): void {
  checkFontAvailable(config.fontFamily);
  term.options.theme = toXtermTheme(config);
  term.options.fontFamily = config.fontFamily;
  term.options.fontSize = config.fontSize;
  term.options.scrollback = config.scrollback;
}

/**
 * Prefer the WebGL renderer; fall back to xterm's DOM renderer when the
 * context can't be created (WebKitGTK blacklists) or is lost at runtime.
 */
export function attachRenderer(term: Terminal): void {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      console.warn("WebGL context lost; falling back to DOM renderer");
      webgl.dispose();
    });
    term.loadAddon(webgl);
  } catch (err) {
    console.warn("WebGL renderer unavailable; using DOM renderer", err);
  }
}
