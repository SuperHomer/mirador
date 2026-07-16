import { create } from "zustand";
import { ITheme } from "@xterm/xterm";
import { ResolvedConfig } from "../bindings";

interface ConfigStore {
  config: ResolvedConfig | null;
  setConfig: (c: ResolvedConfig) => void;
}

export const useConfigStore = create<ConfigStore>((set) => ({
  config: null,
  setConfig: (config) => set({ config }),
}));

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
        `cmux: font "${first}" not found — falling back through the font stack`,
      );
    }
  } catch {
    /* font check unsupported */
  }
}
