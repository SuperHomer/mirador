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

function eventAccel(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.metaKey) mods.push("meta");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (mods.length === 0) return null;

  let key = e.key.toLowerCase();
  if (key.startsWith("arrow")) key = key.slice(5);
  if (key === " ") key = "space";
  // Modifier-only presses aren't bindable.
  if (["shift", "control", "meta", "alt"].includes(key)) return null;
  return [...mods, key].join("+");
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
      const accel = eventAccel(e);
      if (!accel) return;
      const action = map.get(accel);
      if (action && runAction(action)) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [keybindings]);
}
