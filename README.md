# cmux-tauri

A cross-platform terminal for AI coding agent workflows — a [cmux](https://github.com/manaflow-ai/cmux)
rebuild on [Tauri 2](https://tauri.app) (Rust + React + xterm.js, PTYs via
wezterm's `portable-pty`), targeting macOS, Windows, and Linux.

## Features

- **Terminal core**: tabs, horizontal/vertical splits, WebGL rendering with
  fallback, flow-controlled PTY streaming (fast `cat`s can't freeze the UI)
- **Vertical sidebar** with per-tab git branch, PR status (`gh`), listening
  port chips, cwd, and notifications
- **Agent notifications**: panes get an attention ring and tabs light up on
  OSC 9/99/777 sequences or `cmux notify`; native notifications when the
  window is unfocused; notification panel on `mod+I`
- **Command panes** (`cmux run`): agent-launched commands run in a visible,
  interruptible pane; `--wait` returns clean output + exit code to the caller
- **Scriptable browser pane**: agents open pages, snapshot the DOM, click,
  fill, and eval — while you watch (`cmux browser …`)
- **Automation socket**: every action drivable via `cmux` CLI / Unix socket
- **Session persistence**: layout, cwds, scrollback, and browser URLs
  survive restarts and crashes
- **Config**: `~/.config/cmux/cmux.json` (hot-reloaded), Ghostty/wezterm
  theme import, configurable keybindings, command palette (`mod+K`)
- **Claude Code integration**: `cmux hooks setup` lights up tabs when your
  agent needs you and enables per-pane session resume

## Getting started

```bash
npm install
npm run tauri dev        # development
npm run tauri build      # release bundle (.app/.dmg)

# put the CLI on PATH (from src-tauri/):
cargo build -p cmux-cli && ./target/debug/cmux install
```

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

All rebindable in `cmux.json`.
