import { useEffect } from "react";
import { useConfigStore } from "../state/configStore";
import { runAction } from "./actions";

const isMac = navigator.platform.toUpperCase().includes("MAC");

/**
 * Normalizes a KeyboardEvent or accelerator string ("mod+shift+d") to a
 * canonical form for lookup. "mod" = Cmd on macOS, Ctrl elsewhere.
 */
function normalizeAccel(accel: string): string {
  const parts = accel.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  const mods = new Set(parts.slice(0, -1));
  if (mods.delete("mod")) mods.add(isMac ? "meta" : "ctrl");
  if (mods.delete("cmd")) mods.add("meta");
  if (mods.delete("super")) mods.add("meta");
  if (mods.delete("opt") || mods.delete("option")) mods.add("alt");
  const ordered = ["ctrl", "meta", "alt", "shift"].filter((m) => mods.has(m));
  return [...ordered, key].join("+");
}

/** The character-independent name of a physical key, or null if it has none. */
function physicalKey(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].toLowerCase();
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  const arrow = /^Arrow(.+)$/.exec(code);
  if (arrow) return arrow[1].toLowerCase();
  if (code === "Space") return "space";
  if (code === "Tab") return "tab";
  if (code === "BracketLeft") return "[";
  if (code === "BracketRight") return "]";
  return null;
}

/**
 * Accelerators this event could match. Two, because `e.key` is the
 * character the layout produced: on a non-US keyboard Ctrl+Alt is AltGr,
 * so Ctrl+Alt+D can arrive as some symbol rather than "d", and the binding
 * silently never fires. `e.code` names the physical key regardless of
 * layout, so it is checked as well.
 */
function eventAccels(e: KeyboardEvent): string[] {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.metaKey) mods.push("meta");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (mods.length === 0) return [];

  let key = e.key.toLowerCase();
  if (key.startsWith("arrow")) key = key.slice(5);
  if (key === " ") key = "space";
  // Modifier-only presses aren't bindable.
  if (["shift", "control", "meta", "alt"].includes(key)) return [];

  const accels = [[...mods, key].join("+")];
  const physical = physicalKey(e.code);
  if (physical && physical !== key) {
    accels.push([...mods, physical].join("+"));
  }
  return accels;
}

export function useKeymap() {
  const keybindings = useConfigStore((s) => s.config?.keybindings);

  useEffect(() => {
    if (!keybindings) return;
    const map = new Map<string, string>();
    for (const [accel, action] of Object.entries(keybindings)) {
      map.set(normalizeAccel(accel), action);
    }

    const onKeyDown = (e: KeyboardEvent) => {
      for (const accel of eventAccels(e)) {
        const action = map.get(accel);
        if (action && runAction(action)) {
          e.preventDefault();
          e.stopPropagation();
          return;
        }
      }
    };

    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [keybindings]);
}
