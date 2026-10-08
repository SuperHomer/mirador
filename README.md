# Mirador

A cross-platform terminal for AI coding agent workflows — a lookout tower
over your agents. Inspired by [cmux](https://github.com/manaflow-ai/cmux)
and rebuilt on [Tauri 2](https://tauri.app) (Rust + React + xterm.js, PTYs
via wezterm's `portable-pty`) to run on macOS, Windows, and Linux.

The CLI command is **`mira`** ("look!").

**[superhomer.github.io/mirador](https://superhomer.github.io/mirador/)** —
downloads, the feature tour and the keybinding reference.

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
- **Diff panes** (`mira diff`): GitHub-style review of uncommitted work,
  a commit, or a branch — file tree, hunks, and your terminal's own theme.
  A header picker switches between the repository's git worktrees
- **Agent wall** (`mira agent wall`, `mod+shift+A`): every Claude Code
  agent's live terminal in one grid, each with its role, model and whether
  it is working, idle, or waiting on you — type into any of them from there.
  **Agent roles** (`agentRoles` in the config) start Claude Code with their
  own model and system prompt (`mira agent new --role reviewer "…"`)
- **Commit graph** (`mira graph`): every branch's history with lane lines,
  refs and tags; clicking a commit opens its diff in a pane it reuses
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
- **Claude Code integration**: lights up tabs when your agent needs you,
  enables per-pane session resume, and adds a `/mira-diff` skill for
  reviewing the turn's work. Mirador offers to set it up on first launch
  when it finds Claude Code (or: palette → *Set Up Claude Code
  Integration*, or `mira install && mira hooks setup`)
- **Updates**: checks on launch and offers an in-app install; payloads are
  signed, so a tampered release cannot install even though the app carries
  no Apple signature. After one lands, a **What's New** pane opens itself
  once with that release's notes (`What's New` in the palette reopens it)

## Install (macOS)

Grab `Mirador-macOS-arm64.dmg` from the
[latest release](https://github.com/SuperHomer/mirador/releases/latest),
open it and drag **Mirador.app** to `/Applications`. Or build it yourself:

```bash
npm install
npm run tauri build          # builds Mirador.app + .dmg (and the mira CLI)
```

The bundle lands in `src-tauri/target/release/bundle/`. If you use Claude
Code, the first launch offers to set up the integration — `mira` on your
PATH, the hooks and `/mira-diff` — in one click. To do it by hand instead
(the CLI ships inside the app):

```bash
/Applications/Mirador.app/Contents/MacOS/mira install   # → ~/.local/bin/mira
mira hooks setup                                        # Claude Code integration
```

The build is unsigned (no Developer ID), so the first launch needs
right-click → **Open** to get past Gatekeeper. macOS remembers the choice.

## Install (Windows)

Grab `Mirador-Windows-x64-setup.exe` from the
[latest release](https://github.com/SuperHomer/mirador/releases/latest) —
it installs per user, so no admin prompt — or build it yourself:

```powershell
npm install
npm run tauri build     # → src-tauri\target\release\bundle\{nsis,msi}\
```

Building needs Rust (MSVC toolchain), the **Desktop development with C++**
workload from the Visual Studio Build Tools, and Node 20+. WebView2 ships
with Windows 11; on Windows 10 the installer fetches it.

`mira.exe` sits next to `mirador.exe` in the install directory. With Claude
Code installed, the first launch offers to set everything up; by hand, this
puts it on your PATH (adding the directory to your *user* PATH — open a new
terminal afterwards) and installs the hooks:

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

## Shell setup

Mirador renders whatever your shell prints, so the niceties people expect
from a modern terminal — inline suggestions, syntax highlighting, fuzzy
history — come from the shell, not from here. Two worth having:

**zsh** (macOS, Linux):

```bash
brew install zsh-autosuggestions
echo 'source /opt/homebrew/share/zsh-autosuggestions/zsh-autosuggestions.zsh' >> ~/.zshrc
```

Fish-style greyed-out completions drawn from your history, accepted with →.
Panes are login shells, so a new one picks it up with no restart. The path
above is Homebrew's; distro packages put it elsewhere.

**PowerShell** (Windows): PSReadLine ships with PowerShell 7 and does the
same thing once prediction is switched on.

```powershell
Set-PSReadLineOption -PredictionSource History
```

Add it to your `$PROFILE` to keep it.

Mirador deliberately implements none of this itself. A terminal emulator
cannot reliably tell where your input line starts or where the cursor sits
within it, so anything it drew would be guesswork — and it would fight a
shell that already does the job properly.

## Releasing

Releases are signed for the in-app updater with a minisign keypair that is
**not** the Apple code signature (there isn't one). The private key lives in
the `TAURI_SIGNING_PRIVATE_KEY` repository secret and nowhere else that
matters.

**Losing it means every installed copy can never update again** — a new key
produces payloads the installed public key rejects, so the only way back is
asking users to reinstall by hand. Keep a copy somewhere durable.

A release is cut from a draft: write the notes into
`gh release create vX.Y.Z --draft`, then push the `vX.Y.Z` tag.
[release.yml](.github/workflows/release.yml) builds both platforms, attaches
the installers and one manifest per platform —
`latest-darwin-aarch64.json`, `latest-windows-x86_64.json` — and only then
publishes the draft, so no updater ever sees a release without its manifest.
The app asks for
`latest-{{target}}-{{arch}}.json` under `/releases/latest/download/`, which
GitHub resolves to the newest release, so no workflow rewrites any config.

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
| ⌘G | Ctrl+Shift+G | diff pane for this repo |
| ⌘⇧A | Ctrl+Shift+A | agent wall |
| ⌘⌥arrows | Ctrl+Alt+arrows | focus pane by direction |
| ⌘1…9 | Alt+1…9 | jump to tab |
| ⌘K | Ctrl+Shift+K | command palette |
| ⌘I | Ctrl+Shift+I | notifications panel |
| ⌘B | Ctrl+Shift+B | toggle sidebar |
| ⌘C / ⌘V (menu) | Ctrl+Shift+C / Ctrl+Shift+V | copy / paste |

All rebindable in `mirador.json`.

On macOS, Option types the character your layout puts there — `[ ] { } |`
on Swiss, German, French and other non-US layouts — rather than acting as
Meta. If you'd rather have readline's Alt+B / Alt+F word motions and don't
need Option for characters:

```jsonc
// ~/.config/mirador/mirador.json
{ "macOptionIsMeta": true }
```

## Where new tabs open

New tabs open in your home directory. Point them somewhere else with
`defaultCwd`:

```jsonc
// ~/.config/mirador/mirador.json
{ "defaultCwd": "~/Workspace" }
```

`~` and `~/…` expand; anything else must be an absolute path. A path that
isn't a directory is ignored (with a line on stderr) and the home directory
is used instead. Like `shell`, the key is read per tab, so an edit applies
to the next one without a restart.

Splits are unaffected — `mod+D` keeps inheriting the directory of the pane
you split, which is almost always what you want. `defaultCwd` fills in only
where there is nothing to inherit: a new tab, the first pane on a fresh
install, and the tab Mirador recreates when you close the last one. A
restored session keeps each pane's own saved directory.

## Agent roles and the agent wall

A role is a named way to start Claude Code — its own model, and a system
prompt appended to Claude Code's own:

```jsonc
// ~/.config/mirador/mirador.json
{
  "agentRoles": [
    { "name": "planner",  "model": "opus",   "prompt": "You plan. Never edit files." },
    { "name": "coder",    "model": "sonnet" },
    { "name": "reviewer", "model": "haiku",  "prompt": "Review the diff; be terse." }
  ]
}
```

Each role becomes a *New Agent: <role>* palette entry and a `mira agent new
--role <role> [task…]` target. The agent opens in a tab of its own, as a
shell with `claude --model … --append-system-prompt … --name <role>` typed
into it, in the directory of the pane it was started from. Exiting Claude
leaves the shell; restarting Mirador resumes the conversation on the
role's model.

The **agent wall** (`mod+shift+A`, the palette's *Agent Wall*, or `mira agent
wall`) draws every Claude Code agent in a grid — roles, and any `claude`
you started by hand. The tiles are the agents' real terminals, moved there
while the wall is on screen, so you can type into one to answer a
permission prompt without leaving the wall. Each tile shows the role, the
model, and what the agent is doing: **working**, **needs you** (with what
for), or **idle**. Click a tile's header to go to the agent's own tab.

In the sidebar, every tab is listed as usual, and below them, under an
**Agents** heading, the agents of the project you are looking at, each on
one line: ✳ and the role (or *Claude*, for one started without a role).
Clicking a tab shows the whole tab — Claude beside your dev server, say;
clicking an agent shows that agent's pane alone, filling its tab, while
the other panes keep running untouched. Click the tab again for the split.
A project is a git repository, worktrees included, or a directory outside
one. Switch to a tab of another project and the section switches to its
agents; every project's tabs show how many agents it has (✳ 2), filled in
when one of them needs you. The tab shortcuts number rows in the order
shown.

Any pane can be shown alone that way: *Toggle Pane Zoom* in the palette,
or `mira zoom [pane]` (`--off` for the whole tab again).

The wall shows the same project's agents, or all of them: the switch in
its header picks. A tab moves there when Claude starts in it and back when it exits.

Status comes from the Claude Code hooks, so it needs the integration set up
(an install from an earlier version gains the extra hooks on next launch).

## Terminals that keep running when you quit

Quitting Mirador doesn't end what runs in it: shells, builds, dev servers and
agents keep going, and the next launch reattaches to them — the same
processes, with what they printed while Mirador was closed.

- **Closing a pane** ends it and everything in it, as before.
- **Quit and End All Sessions** in the palette (or `mira quit --end-sessions`)
  ends every session, then quits. Cmd+Q keeps them.
- **Notifications** raised while Mirador was closed arrive as one when it
  reopens.
- **A session with no pane to come back to** — after a crash, say — reopens
  in a tab of its own, never silently killed.

Each pane runs in a small holder process (`mira __hold`, about 3 MB) that
owns its terminal. After a reboot there is nothing to reattach, and panes are
restored the usual way. To have Cmd+Q end everything as it used to:

```jsonc
// ~/.config/mirador/mirador.json  (%APPDATA%\mirador\mirador.json on Windows)
{ "persistSessions": false }
```

The key is read when a pane opens, so it applies to new panes; sessions that
are already running still reattach on the next launch rather than being
stranded. Design:
[docs/design/session-persistence.md](docs/design/session-persistence.md).

## What's New after an update

When the running version differs from the one last launched, Mirador opens a
read-only pane with that release's notes, in a tab of its own that does
*not* steal focus — the pane you were typing in keeps the cursor. The
palette's **What's New** reopens it any time.

Notes are the GitHub release body for the running tag, fetched once and
cached under the app's data directory, so reopening works offline. A fresh
install shows nothing (there is no "new" yet), and so does a failed
fetch — release notes are never worth an error dialog in a terminal.

The markdown is parsed in Rust
([`notes.rs`](src-tauri/crates/cmux-core/src/notes.rs)) into blocks of plain
text that the pane maps to elements. Nothing in this path renders HTML:
notes are remote text displayed in the webview that holds the IPC bridge, so
raw HTML in a release body shows as the characters it is made of, and only
`http(s)` links are navigable.
