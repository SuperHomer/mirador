import { create } from "zustand";
import { ITheme } from "@xterm/xterm";
import { ResolvedConfig, UiColors } from "../bindings";

interface ConfigStore {
  config: ResolvedConfig | null;
  setConfig: (c: ResolvedConfig) => void;
}

export const useConfigStore = create<ConfigStore>((set) => ({
  config: null,
  setConfig: (config) => {
    applyUiColors(config.colors.ui);
    set({ config });
  },
}));

/** Sets the theme's app colors as the `:root` tokens, on the document
 *  element so html, body and anything portaled outside `.app` take them
 *  too. styles.css's own values are what shows until the config loads. */
function applyUiColors(ui: UiColors) {
  const root = document.documentElement.style;
  for (const [field, color] of Object.entries(ui)) {
    root.setProperty(`--${field.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`, color);
  }
}

/** Whether the app opens the editor, a diff or the graph in a tab of its
 *  own, from mirador.json — before it is read, as its defaults do. */
export function opensInTab(what: "editor" | "diff" | "graph"): boolean {
  const config = useConfigStore.getState().config;
  const where = config
    ? { editor: config.editorOpensIn, diff: config.diffOpensIn, graph: config.graphOpensIn }[what]
    : what === "editor"
      ? "tab"
      : "split";
  return where === "tab";
}

const PALETTE_KEYS = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

export function toXtermTheme(config: ResolvedConfig): ITheme {
  const theme: ITheme = {
    background: config.colors.background,
    foreground: config.colors.foreground,
    cursor: config.colors.cursor,
    selectionBackground: config.colors.selectionBackground,
  };
  config.colors.palette.slice(0, 16).forEach((color, i) => {
    (theme as Record<string, string>)[PALETTE_KEYS[i]] = color;
  });
  return theme;
}

/** Warn (once per family) when a configured font isn't available. */
const warned = new Set<string>();
export function checkFontAvailable(family: string) {
  const first = family.split(",")[0].trim().replace(/["']/g, "");
  if (!first || warned.has(first) || !("fonts" in document)) return;
  warned.add(first);
  try {
    if (!document.fonts.check(`12px "${first}"`)) {
      console.warn(
        `mirador: font "${first}" not found — falling back through the font stack`,
      );
    }
  } catch {
    /* font check unsupported */
  }
}
