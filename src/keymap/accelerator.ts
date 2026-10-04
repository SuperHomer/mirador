/**
 * Turning a configured accelerator ("mod+shift+d") into something a person
 * reads ("⌘⇧D", or "Ctrl+Shift+D" away from a Mac).
 *
 * The config's own spelling is deliberately platform-neutral so one
 * `mirador.json` works everywhere; this is the other end of that, and the
 * only place that decides what "mod" looks like.
 */

export const isMac = navigator.platform.toUpperCase().includes("MAC");

/** Apple's order for modifier symbols, which is not the config's order. */
const MAC_SYMBOLS: Record<string, string> = {
  ctrl: "⌃",
  alt: "⌥",
  shift: "⇧",
  meta: "⌘",
};
const MAC_ORDER = ["ctrl", "alt", "shift", "meta"];

const PC_NAMES: Record<string, string> = {
  ctrl: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  meta: "Win",
};
const PC_ORDER = ["ctrl", "alt", "shift", "meta"];

/** Keys whose name is not simply its character. */
const KEY_NAMES: Record<string, { mac: string; pc: string }> = {
  left: { mac: "←", pc: "Left" },
  right: { mac: "→", pc: "Right" },
  up: { mac: "↑", pc: "Up" },
  down: { mac: "↓", pc: "Down" },
  space: { mac: "Space", pc: "Space" },
  tab: { mac: "⇥", pc: "Tab" },
  enter: { mac: "↩", pc: "Enter" },
  escape: { mac: "⎋", pc: "Esc" },
  backspace: { mac: "⌫", pc: "Backspace" },
};

/**
 * "mod+shift+d" → "⌘⇧D". Unknown modifiers are dropped rather than shown
 * raw: a typo in `mirador.json` does not bind anything either, so printing
 * it would advertise a shortcut that cannot fire.
 */
export function formatAccel(accel: string): string {
  const parts = accel.toLowerCase().split("+").filter(Boolean);
  if (parts.length === 0) return "";
  const key = parts[parts.length - 1];

  // Same aliases `normalizeAccel` accepts, so what is shown matches what
  // the keymap actually listens for.
  const mods = new Set<string>();
  for (const raw of parts.slice(0, -1)) {
    switch (raw) {
      case "mod":
        mods.add(isMac ? "meta" : "ctrl");
        break;
      case "cmd":
      case "super":
        mods.add("meta");
        break;
      case "opt":
      case "option":
        mods.add("alt");
        break;
      case "ctrl":
      case "control":
        mods.add("ctrl");
        break;
      case "alt":
      case "shift":
      case "meta":
        mods.add(raw);
        break;
      default:
        break;
    }
  }

  const named = KEY_NAMES[key];
  const keyText = named
    ? isMac
      ? named.mac
      : named.pc
    : key.length === 1
      ? key.toUpperCase()
      : key.charAt(0).toUpperCase() + key.slice(1);

  if (isMac) {
    // Symbols need no separator — "⌘⇧D" is how macOS writes it.
    return MAC_ORDER.filter((m) => mods.has(m))
      .map((m) => MAC_SYMBOLS[m])
      .join("")
      .concat(keyText);
  }
  return [...PC_ORDER.filter((m) => mods.has(m)).map((m) => PC_NAMES[m]), keyText].join(
    "+",
  );
}

/**
 * action id → the accelerator to advertise for it.
 *
 * `keybindings` is a map keyed the other way, and an action can have more
 * than one binding. The pick is the fewest modifiers, then the shortest,
 * then alphabetical — a total order, because the config arrives as a JSON
 * object whose key order is not stable between runs and a shortcut that
 * changed on every launch would be worse than none.
 */
export function acceleratorsByAction(
  keybindings: Record<string, string> | undefined,
): Map<string, string> {
  const byAction = new Map<string, string[]>();
  for (const [accel, action] of Object.entries(keybindings ?? {})) {
    if (!action || action === "none") continue;
    const list = byAction.get(action);
    if (list) list.push(accel);
    else byAction.set(action, [accel]);
  }

  const best = new Map<string, string>();
  for (const [action, accels] of byAction) {
    accels.sort((a, b) => {
      const mods = a.split("+").length - b.split("+").length;
      if (mods !== 0) return mods;
      if (a.length !== b.length) return a.length - b.length;
      return a < b ? -1 : a > b ? 1 : 0;
    });
    best.set(action, accels[0]);
  }
  return best;
}
