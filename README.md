# Mirador

A cross-platform terminal for AI coding agent workflows — a lookout tower
over your agents. Inspired by [cmux](https://github.com/manaflow-ai/cmux)
and rebuilt on [Tauri 2](https://tauri.app) (Rust + React + xterm.js, PTYs
via wezterm's `portable-pty`) to run on macOS, Windows, and Linux.

The CLI command is **`mira`** ("look!").

## Features

- **Terminal core**: tabs, horizontal/vertical splits, WebGL rendering with
  fallback, flow-controlled PTY streaming (fast `cat`s can't freeze the UI)
- **Vertical sidebar** with per-tab git branch, PR status (`gh`), listening
  port chips, cwd, and notifications
- **Agent notifications**: panes get an attention ring and tabs light up on
  OSC 9/99/777 sequences or `mira notify`; native notifications when the
  window is unfocused; notification panel on `mod+I`
- **Command panes** (`mira run`): agent-launched commands run in a visible,
  interruptible pane; `--wait` returns clean output + exit code to the caller
- **Scriptable browser pane**: agents open pages, snapshot the DOM, click,
  fill, and eval — while you watch (`mira browser …`)
- **Remote workspaces**: `mira ssh open <host>` panes run the system ssh
  (agent auth, 2FA, ProxyJump all work); ControlMaster-backed port
  forwarding brings remote dev servers to your browser pane
- **Automation socket**: every action drivable via the `mira` CLI / Unix socket
- **Session persistence**: layout, cwds, scrollback, and browser URLs
  survive restarts and crashes
- **Config**: `~/.config/mirador/mirador.json` (hot-reloaded), Ghostty/wezterm
  theme import, configurable keybindings, command palette (`mod+K`)
- **Claude Code integration**: `mira hooks setup` lights up tabs when your
  agent needs you and enables per-pane session resume

## Install (macOS)

```bash
npm install
npm run tauri build          # builds Mirador.app + .dmg (and the mira CLI)
```

The bundle lands in `src-tauri/target/release/bundle/`. Drag
**Mirador.app** to `/Applications`, then put the CLI on your PATH — it
ships inside the app:

```bash
/Applications/Mirador.app/Contents/MacOS/mira install   # → ~/.local/bin/mira
mira hooks setup                                        # Claude Code integration
```

The build is unsigned (no Developer ID), so the first launch needs
right-click → **Open** to get past Gatekeeper. macOS remembers the choice.

## Development

```bash
npm run tauri dev            # hot-reloading dev build
npm run build:cli            # release CLI only → src-tauri/target/release/mira
```

## License

MIT — see [LICENSE](LICENSE).

Mirador is an independent implementation inspired by
[cmux](https://github.com/manaflow-ai/cmux); no cmux code is used.

## Platform status

macOS is built and verified end to end. The Rust core and CLI type-check
for Windows, but the app has not been built or tested there yet: the
automation socket is Unix-only (Windows needs a named-pipe listener) and
ConPTY needs a pass. Linux should build, but WebKitGTK rendering and the
browser pane's child-webview positioning are unverified.

## Agents

See [docs/AGENTS.md](docs/AGENTS.md) for the full cookbook: notifications,
observable command execution, browser automation, hooks, and the socket
protocol.

## Default keys (`mod` = ⌘ on macOS, Ctrl elsewhere)

| Keys | Action |
|---|---|
| mod+T / mod+W | new tab / close pane |
| mod+D / mod+shift+D | split right / down |
| mod+alt+arrows | focus pane by direction |
| mod+1…9 | jump to tab |
| mod+K | command palette |
| mod+I | notifications panel |
| mod+B | toggle sidebar |

All rebindable in `mirador.json`.
