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

## Install (Windows)

Grab `Mirador_<version>_x64-setup.exe` from the
[latest release](https://github.com/yoanmarti/mirador/releases) — it
installs per user, so no admin prompt — or build it yourself:

```powershell
npm install
npm run tauri build     # → src-tauri\target\release\bundle\{nsis,msi}\
```

Building needs Rust (MSVC toolchain), the **Desktop development with C++**
workload from the Visual Studio Build Tools, and Node 20+. WebView2 ships
with Windows 11; on Windows 10 the installer fetches it.

`mira.exe` sits next to `mirador.exe` in the install directory. Put it on
your PATH (this adds the directory to your *user* PATH — open a new
terminal afterwards):

```powershell
& "$env:LOCALAPPDATA\Mirador\mira.exe" install
mira hooks setup        # Claude Code integration
```

**PowerShell or Git Bash?** Either — panes are ConPTY sessions, so any
shell works. Out of the box Mirador picks PowerShell 7 (`pwsh.exe`) if you
have it, else Windows PowerShell, else `cmd.exe`, and it teaches PowerShell
to report its working directory so the sidebar's cwd, git branch and PR
status light up. Nothing needs Git Bash. If you want it anyway:

```jsonc
// %APPDATA%\mirador\mirador.json
{ "shell": "C:\\Program Files\\Git\\bin\\bash.exe", "shellArgs": ["-l"] }
```

`mira run "npm test"` hands the command to whichever shell that is, so
quoting follows your shell's rules.

Two Windows caveats: notifications only appear once the app is installed
(Windows toasts need a Start Menu entry — a bare `mirador.exe` stays
silent), and `mira ssh forward` opens its own `ssh -N -L` connection
because Windows' OpenSSH has no ControlMaster, so a password-based host
asks to authenticate a second time.

## Development

```bash
npm run tauri dev            # hot-reloading dev build
npm run build:cli            # release CLI only → src-tauri/target/release/mira
```

Windows is verified by CI ([build-windows.yml](.github/workflows/build-windows.yml)):
clippy, the Rust test suite, and the installers, on every push and release.
Cross-checking Windows from macOS works too:

```bash
rustup target add x86_64-pc-windows-msvc
cargo check --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml \
  -p cmux-core -p cmux-cli
```

## License

MIT — see [LICENSE](LICENSE).

Mirador is an independent implementation inspired by
[cmux](https://github.com/manaflow-ai/cmux); no cmux code is used.

## Platform status

macOS is built and verified end to end. Windows is implemented and built by
CI — ConPTY panes, a named-pipe automation socket, Win32 cwd/port
detection, per-user installers — but has not yet been driven by hand, so
treat it as beta. Linux should build, but WebKitGTK rendering and the
browser pane's child-webview positioning are unverified.

## Agents

See [docs/AGENTS.md](docs/AGENTS.md) for the full cookbook: notifications,
observable command execution, browser automation, hooks, and the socket
protocol.

## Default keys

Plain Ctrl belongs to the program in your pane (Ctrl+C interrupts, Ctrl+D
is EOF), so outside macOS the app's own keys live on Ctrl+Shift. Ctrl+Alt
is avoided for letters — it *is* AltGr on international keyboards — so the
pairs macOS spells with Shift take a second letter instead.

| macOS | Windows / Linux | Action |
|---|---|---|
| ⌘T / ⌘W | Ctrl+Shift+T / Ctrl+Shift+W | new tab / close pane |
| ⌘⇧W | Ctrl+Shift+Q | close tab |
| ⌘D / ⌘⇧D | Ctrl+Shift+D / Ctrl+Shift+E | split right / down |
| ⌘⌥arrows | Ctrl+Alt+arrows | focus pane by direction |
| ⌘1…9 | Alt+1…9 | jump to tab |
| ⌘K | Ctrl+Shift+K | command palette |
| ⌘I | Ctrl+Shift+I | notifications panel |
| ⌘B | Ctrl+Shift+B | toggle sidebar |
| ⌘C / ⌘V (menu) | Ctrl+Shift+C / Ctrl+Shift+V | copy / paste |

All rebindable in `mirador.json`.
