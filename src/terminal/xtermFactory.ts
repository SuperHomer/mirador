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
    // Off by default: on non-US Mac layouts Option is the third-level
    // shift that types [ ] { } | \ @ #, and treating it as Meta swallows
    // those characters (they arrive as ESC+digit). Users who want Alt-as-
    // Meta for readline word motions opt in with `macOptionIsMeta`.
    macOptionIsMeta: config.macOptionIsMeta,
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
  term.options.macOptionIsMeta = config.macOptionIsMeta;
}

/**
 * Prefer the WebGL renderer; fall back to xterm's DOM renderer when the
 * context can't be created (WebKitGTK blacklists) or is lost at runtime.
 * Returns the addon so the caller can release the context, or null when
 * the DOM renderer is in use; `onLost` fires if the context is lost later.
 */
export function attachRenderer(
  term: Terminal,
  onLost?: () => void,
): WebglAddon | null {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      console.warn("WebGL context lost; falling back to DOM renderer");
      webgl.dispose();
      onLost?.();
    });
    term.loadAddon(webgl);
    return webgl;
  } catch (err) {
    console.warn("WebGL renderer unavailable; using DOM renderer", err);
    return null;
  }
}
