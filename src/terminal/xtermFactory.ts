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
  // Hyperlinks a program prints (OSC 8: text with an address behind it,
  // which is how Claude Code prints its links) open the same way. With no
  // handler, xterm's default asked on every click with window.confirm
  // ("This link could potentially be dangerous") and then opened nothing:
  // it opens the address with window.open, and the app has no new-window
  // handler, so wry refuses the window. WKWebView does not even show the
  // confirm. The text need not be the address, so the address shows on
  // hover.
  //
  // Unlike a plain URL, which WebLinksAddon's pattern bounds, the address
  // here is whatever the program sent: quotes, spaces, backslashes, bidi
  // controls, any length. So it is parsed once, and both the opener and
  // the tooltip get the parser's spelling of it, never the raw string:
  // what the tooltip shows is what opens.
  term.options.linkHandler = {
    activate: (event, uri) => {
      event.preventDefault();
      const url = webUrl(uri);
      if (url) void openUrl(url);
    },
    hover: (_event, uri) => {
      const url = webUrl(uri);
      if (url && term.element) term.element.title = url;
    },
    leave: () => {
      term.element?.removeAttribute("title");
    },
  };

  return term;
}

/**
 * `uri` as a click may open it: http or https only, as everywhere in the
 * app (xterm passes no other scheme while allowNonHttpProtocols is off;
 * this keeps that true whatever its default becomes), spelled as the URL
 * parser writes it. `href` percent-encodes quotes, spaces, angle brackets
 * and bidi controls, turns backslashes into slashes and an IDN host into
 * punycode. Null for anything else.
 */
function webUrl(uri: string): string | null {
  try {
    const url = new URL(uri);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
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
