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
- **Agent wall** (`mira agent wall`, `mod+shift+A`): a tab's splits swapped
  for its agents' live terminals in one grid — Claude Code, Codex,
  OpenCode, Pi, Gemini CLI, Amp, Aider and more, found by their process —
  each with its role, model and (Claude Code) whether it is working, idle,
  or waiting on you; type into any of them from there. The same key
  switches back, and each tab remembers its view across restarts. The
  sidebar's **Splits · Wall · Tasks · Board** switch turns it over to the
  tab's task board.
  **Agent roles** (`.mirador/roles/*.md` in the repository, or your own)
  start Claude Code or Codex with their own model, prompt and shared way of
  working (`mira agent new --role reviewer "…"`). **Teams**
  (`.mirador/teams/*.md`) start several in a tab of their own, each told
  its teammates, and stay tracked across restarts (`mira team start`)
- **Task board**, the agent wall's Tasks side (**Tasks** in the sidebar's
  view switch, the palette's *Agents: Task Board*, or `mira agent board`): what
  each agent was given and where it stands, in Needs you, To do, Doing and Done, for this team or every
  tab. Mirador adds a card whenever it gives an agent a task (`team start`,
  `agent new`, `mira agent send`) and, for Claude Code and Codex, moves it
  as the agent's turns end; agents write their own with `mira task add` and
  move them with `mira task move`. A card's title opens its history — who
  sent what, how each turn ended and how long it took — and `mira agent
  history` prints the same in the terminal. Kept across restarts, 30 days
  deep. Its **Trail** shows what each agent actually did, turn by turn:
  every command with how it ended, every file it edited, every other tool
  it called, each opening to its full input and output — read from Claude
  Code's transcript or Codex's session file, never copied
  (`mira agent trail`; docs/AGENTS.md)
- **Board** (`mira view board`, `mod+shift+O`, or **Board** in the
  sidebar's view switch): every tab as a frame and every pane as a card on one
  canvas — a tab showing its agent wall shows the wall in its frame instead,
  its agents' live tiles or its task board. Scroll over the focused card to scroll its content, at any zoom
  (a browser card pans away from 100%, where its page is not drawn);
  scroll anywhere else, other cards included, or drag empty board, a gap
  or a card's header, to pan, or hold ⌥ (Alt) to pan over the focused
  card too — anywhere but a browser page at 100%, which keeps its own
  scroll; pinch or Ctrl+scroll to zoom; double-click a card's header
  to fly to it at 100%. A frame's **+** adds a pane to it, **New frame** a
  tab. In the
  corner, **−** and **+** step the zoom (10% to 400%), double-clicking the
  percentage types one in, **Fit** shows everything, and **Arrange** packs
  the frames into rows, in tab order, to the window's shape. Typing
  goes to the focused pane at any zoom, and from 50% up the pointer
  reaches inside cards too: click, select text, use the mouse in a
  program. Below 50% the board is an overview, where a click focuses a
  card (the wheel scrolls it once it is focused). A browser page shows
  only at 100%, while its card is still, wholly in view and nothing
  (palette, panel, menu) is open over it. Away from that, a card shows a recent picture
  of its page rather than its URL, refreshed after it navigates and every
  few seconds while the card and the window are visible. That is on
  macOS; Windows tries, and keeps the last picture (or the URL) when it
  can't; elsewhere the card shows the URL
- **Commit graph** (`mira graph`): every branch's history with lane lines,
  refs and tags; clicking a commit opens its diff in a pane it reuses
- **Scriptable browser pane**: an agent tests the page it is building —
  opens it, snapshots it, clicks, fills, types and presses keys, waits for
  it, reads its console errors — while you watch (`mira browser …`)
- **Remote workspaces**: `mira ssh open <host>` panes run the system ssh
  (agent auth, 2FA, ProxyJump all work); ControlMaster-backed port
  forwarding brings remote dev servers to your browser pane
- **Automation socket**: every action drivable via the `mira` CLI / Unix socket
- **Session persistence**: layout, cwds, scrollback, and browser URLs
  survive restarts and crashes
- **Config**: `~/.config/mirador/mirador.json` (hot-reloaded), Ghostty/wezterm
  theme import (`themeSource` takes `auto`, `ghostty`, `wezterm`, `builtin`
  or `none`, or a named theme: `devaslife`, `catppuccin-latte`,
  `tokyo-night`), configurable keybindings, command palette (`mod+K`)
- **Claude Code integration**: lights up tabs when your agent needs you,
  enables per-pane session resume, and adds a `/mira-diff` skill for
  reviewing the turn's work. Mirador offers to set it up on first launch
  when it finds Claude Code (or: palette → *Set Up Claude Code
  Integration*, or `mira install && mira hooks setup`)
- **Updates**: checks on launch and offers an in-app install; payloads are
  signed, so a tampered release cannot install even though the app carries
  no Apple signature. After one lands, a **What's New** pane opens itself
  once with that release's notes (`App: What's New` in the palette reopens it)

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

Release builds are signed with a Developer ID and notarized by Apple, so
the first launch only asks to confirm opening an app downloaded from the
internet — click **Open**. A build you make yourself is only ad-hoc
signed; macOS won't stop it, since a local build is never quarantined.

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
| ⌘⇧A | Ctrl+Shift+A | agent wall on / off |
| ⌘⇧O | Ctrl+Shift+O | board / tabs view |
| ⌘0 | Ctrl+Shift+0 | board: show everything |
| ⌘⌥arrows | Ctrl+Alt+arrows | focus pane by direction |
| ⌘1…9 | Alt+1…9 | jump to tab |
| ⌘K | Ctrl+Shift+K | command palette |
| ⌘I | Ctrl+Shift+I | Inbox |
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

## Where the editor, diffs and the graph open

Opened from the app — the palette, a frame's + menu, the wall — the roles,
ways and teams editor gets a tab of its own, and a diff or the commit
graph a split beside the focused pane. Three keys change that, each
`"tab"` or `"split"`, the values of a custom command's `target`:

```jsonc
// ~/.config/mirador/mirador.json
{ "editorOpensIn": "split", "diffOpensIn": "tab", "graphOpensIn": "tab" }
```

Any other value is ignored for the default (`editorOpensIn` `"tab"`, the
other two `"split"`), and an edit applies at once. In a tab, the editor or
a graph already open on the same repository (for the editor outside a
repository, the same directory) is shown again instead of a second one; a
diff always opens a new pane, and so does a split. The `mira` commands
ignore these keys and keep their own `--tab` flag — which reuses the same
way: `mira graph --tab` and `mira agent edit --tab` focus the graph or
editor already open on that repository and print its pane, and open one
only when there is none.

## Agent roles and the agent wall

A role is a named way to start an agent — Claude Code or Codex, its own
model and effort, and its own instructions — written as a Markdown file: the
frontmatter says how it starts, the body is its prompt.

```markdown
<!-- .mirador/roles/reviewer.md -->
---
model: haiku
effort: high
way: our-way
description: Reviews diffs, never edits
---
You review the diff. Never edit files; be terse.
```

```markdown
<!-- .mirador/roles/coder.md -->
---
harness: codex
model: gpt-5
way: our-way
---
You implement what the plan says, one step at a time.
```

`effort` is how hard the model thinks: Claude Code gets it as `--effort`
(`low`, `medium`, `high`, `xhigh`, `max`), Codex as
`model_reasoning_effort` (the same, plus `ultra`, which only some models
take). Absent, the harness's default. Any other value, upper case
included, greys the role out with the values it takes.

A Codex role can also name one of your Codex **profiles** —
`profile: yolo` starts it with `--profile yolo` — for what a role cannot
say itself, such as an approval policy or the sandbox. Only in a role of
your own: a profile can turn the sandbox off, so `.mirador/roles` may not
pick one.

```toml
# ~/.codex/yolo.config.toml — Codex 0.164 keeps each profile in its own file
approval_policy = "never"
sandbox_mode = "danger-full-access"   # --dangerously-bypass-approvals-and-sandbox
```

A **way of working** is a playbook any role can follow — how your team
branches, tests, commits and hands off — written once in `ways/` and
appended after the prompt of every role that names it:

```markdown
<!-- .mirador/ways/our-way.md -->
---
description: Small steps, tests first
---
1. Write the failing test first, then the code.
2. One branch per task; never push — a human does.
```

A Claude Code role can also be held to **only what it needs** — the MCP
servers, plugins and skills it may use, and permission rules:

```markdown
<!-- .mirador/roles/charts.md -->
---
model: haiku
mcp: [github]            # only this MCP server, from wherever it is configured
plugins: []              # no plugins
skills: [dataviz]        # only this skill; `pdf` also names `anthropic-skills:pdf`
tools:
  deny: ["Bash(git push *)"]
---
You draw charts from the data you are given.
```

A field left out changes nothing; an empty list means none. Mirador gives
them to Claude Code as a settings file (`--settings`), and passes the same
file again when the conversation resumes after a restart — if that file is
gone, the pane comes back as a plain shell rather than an agent with every
tool. For `skills` and `plugins` it first asks Claude Code, in the agent's
directory, which ones exist (a `claude -p` that is stopped before it calls
a model), so every other one can be switched off; a name that matches
nothing stops the launch and lists what there is. Skills left out are
hidden from the agent, not from you: `/name` still works when you type it.

**A repository's role can take tools away, never hand them out.**
`tools.allow` is refused in `.mirador/roles` — rules given at launch are
Claude Code's highest-priority settings, and would skip the trust prompt
it shows before a project's own settings apply. Put `allow` in a role of
your own. Codex roles cannot be limited yet: a Codex role with any of
these fields does not start.

**Edit them in the app:** *Agents: Edit Roles, Ways & Teams* in the palette (or
`mira agent edit`) opens a pane — from the app in a tab of its own (see
`editorOpensIn`), from `mira agent edit` beside the pane it ran in — listing every
role, way of working and team its directory sees — marked `repo`, `yours`
or `mirador.json` — with what is wrong with any of them. Pick one to edit
its file; `+` starts a new one from a template, in the repository or in
yours. It checks as you type and will not save a file that does not parse
(⌘S / Ctrl+S saves); *Start* launches the role or team as it is saved.
It only ever touches `.md` files in those `roles/`, `ways/` and `teams/`
directories. `agentRoles` entries show read-only: they live in
mirador.json.

Roles and ways live in two places, and the same name means the first (a
few also ship with Mirador, below both — see *Teams*):

1. **The repository's** `.mirador/roles/` and `.mirador/ways/` — the
   nearest `.mirador` at or above the pane's directory. Commit them and
   everyone working on the project gets the same team.
2. **Yours**: `roles/` and `ways/` beside mirador.json
   (`~/.config/mirador/`, `%APPDATA%\mirador` on Windows), for every
   project.

`agentRoles` in mirador.json still works — `{ "name": "planner", "model":
"opus", "prompt": "You plan." }` — below both. `mira agent roles` lists
what the current directory sees and where each came from, and warns about
anything it could not use: a file that does not parse, a field that is not
one, a way nobody wrote.

Each role is an entry under the palette's *Agents: New Agent…* — Enter on it
lists plain Claude Code and the roles, to search in turn (Esc or
Backspace goes back; typing a role's name at the top finds it too, below
the commands) — an entry in the wall's **+ Agent** menu, and a `mira
agent new --role <role> [task…]` target — the roles of the directory you
are in. Teams (*Agents: Start Team…*) and `~/.ssh/config` hosts (*Pane: SSH…*) are
listed the same way. The agent opens in a pane
beside the one you are in — it joins the tab you are working in, and its
row appears under **Agents** — as a shell with the agent typed into it, in
that pane's directory: `claude --model … --effort …
--append-system-prompt-file … --name <role>`, or `codex --model … -c
model_reasoning_effort=… --profile … -c developer_instructions=…`. (`mira
agent new --tab` gives it a tab of its own instead.) Codex also gets a
permissions profile — the one it would run on anyway, plus Mirador's socket
and nothing else — so `mira agent send` and `wait` work from inside its
sandbox; not when your Codex config uses `sandbox_mode`, gives full access,
or cannot be read. Exiting the agent
leaves the shell; restarting Mirador resumes a Claude conversation on the
role's model and effort, with its prompt.

### Teams

A **team** is roles started together, in a tab of their own, each told who
its teammates are — `teams/<name>.md` beside `roles/` and `ways/` (the
repository's `.mirador/teams`, then yours):

```markdown
<!-- .mirador/teams/squad.md -->
---
description: Plan, build, review
way: our-way             # every member follows it, besides its role's own
layout: grid             # or columns, rows
members:
  - role: planner
    lead: true           # the team's task goes to it
  - role: dev
    count: 2             # dev-1 and dev-2
  - role: reviewer
    name: gate           # called gate instead of reviewer
---
The planner splits the work into tasks; devs build; nothing merges
until gate has read the diff.
```

`mira team start squad "add OAuth"` (or the palette's *Agents: Start Team…* ›
*squad*) opens the tab with every member in an even grid, its `⚑` mark in
the sidebar. Each member gets its role's prompt, then the charter, the
team's way of working, and its teammates by name with the commands that
show their panes and screens; each Claude session is named after its
member. The task goes to the lead. Every member's launch is worked out
before any pane opens, so a team that cannot start — a missing role, a
role that cannot start, two members of the same name — opens nothing.

**One team ships with Mirador: `omakase`** — a planner and a worker on
Claude Code (Opus 5.5) and a reviewer on Codex (`gpt-6-luna`), working in
the Rails doctrine's taste: the codebase's conventions, the simplest thing
that works, small steps that ship with their tests, nothing pushed. The
planner splits the task, puts each step on the task board, and hands the
worker one step at a time (`--wait`), then asks the reviewer, who answers
it with APPROVE or CHANGES; the planner's edit tools are off. Start it with the palette's *Agents: Start Team…* ›
*omakase*, or `mira team start omakase "…"`. Its roles
(`omakase-planner`, `-worker`, `-reviewer`), its way of working and the
team are *built-in*: the same for everyone, read-only, and replaced by any
file of the same name — the editor's *Copy to yours* makes one to change.
The reviewer's `mira` reaches Mirador through Codex's sandbox (see roles,
above); Claude Code's hooks (`mira hooks setup`) are what let the planner wait
on the worker. Codex shows one startup warning for a role's agent —
"Running without the shared background server" — because its prompt goes
in as a `-c developer_instructions=…` override, which Codex runs in its
embedded mode: the session is not on Codex's shared server (`codex
agents` does not list it), and nothing in Mirador depends on that.

Mirador keeps track of the team with its tab, across restarts:

```bash
mira team list               # each member: running, exited, or closed
mira team restart            # start the exited and closed ones again
mira team restart dev-2      # …or just these
mira team add reviewer       # another member, beside this pane: reviewer-2
mira team list --defined     # the teams this directory can start
```

Members hand each other work through Mirador — the prompt each one gets
says how:

```bash
mira agent send reviewer-1 "auth is done, please review src/auth.rs"
mira agent send gate --wait "ready to merge?"   # …and block until gate has answered
mira agent wait dev-2                           # block until dev-2 has finished its turn
```

`agent send` types the message into the teammate's terminal as its next
prompt, headed with who it is from, as one bracketed paste — a message of
several lines arrives whole. A name is a teammate's in your tab (a team
member's, a Claude session's, a role's); a pane id reaches any pane. It
refuses a pane running no agent, where the message would run as a shell
command, and an agent waiting on you (a permission prompt), where it would
answer the prompt — `--force` overrides that one. `agent wait` exits 0 once
the agent has worked and stopped, 2 if it stopped to ask you something, 3
if it exited, 124 on `--timeout`; it needs Claude Code's hooks (`mira hooks
setup`) to see an agent stop.

`team restart` types a member's command back into its pane when its shell
is idle — never into a shell running something else, which it reports as
`busy` — and opens a new pane beside a teammate for one whose pane was
closed. Run `team add` and `team restart` from a pane of the team's tab.

The **agent wall** is a way of showing a tab: in place of its splits, the
tab's agents in a grid — roles, and any agent you started by hand:
`claude`, `codex`, `opencode`, `pi`, `gemini`, `amp`, `cursor-agent`,
`copilot`, `aider`, `goose`, `qwen`, `crush` or `droid`, found in the
pane's process table (macOS and Linux; on Windows only Claude Code's hooks
make an agent). `mod+shift+A`, the palette's *Agents: Show Wall*
and `mira agent wall` (which prints `wall on` or `wall off`)
each switch the current tab between the two, and so does the sidebar's
**Splits · Wall · Tasks · Board** switch, at its top. Each tab keeps its view across restarts, and the
board draws a walled tab's wall in its frame, in place of its cards. The tiles are the
agents' real terminals, moved there while the wall is on screen, so you
can type into one to answer a permission prompt without leaving the wall;
the keyboard goes to the focused agent's tile when the wall comes up. A
new agent from the wall's **+ Agent** — a role of the repository, or
plain Claude Code or Codex, with an optional first message that also
becomes its card — splits the tab under the wall and gets a tile. A tab
with no agents yet shows its roles and teams there instead, ready to
start. The wall's toolbar shows all its agents or only those that
need you, are working or are idle, and switches between **Focus** and
**Grid**. In Grid it sets how many agents a page holds (4, 6 or 9: the
`agentsPerTab` key), and in Focus it pins the agent shown large or lets
it follow whoever needs you. Focus shows that agent's terminal large and
the others down a rail: those that need you and those at work as live
thumbnails (drawn smaller, never resized), the idle one line each; click
one to pin it. Following moves only once nobody has typed into the large
one for two seconds. Grid shows every agent's terminal at full size, a
page of 4, 6 or 9 at a time, with a tab per page once there are more
than one (each with a pip per agent, and a count when one needs you);
each tile's **Focus ›** shows it large. Switching between the two keeps
the keyboard on the agent that had it. Tiles stand in the team's order,
never reshuffled by status. Each tile shows the role, the model, and what the agent is
doing: **working**, **needs you** (with what for), or **idle**. Click a
tile's header to see that agent alone in its tab, which leaves the wall.
The sidebar's switch turns the wall over to the tab's task board in
place of the tiles, and back; the wall comes up again on the side it was
left on, across restarts too.

Every agent ends in a **status bar** — its tile on the wall, and its
pane in the splits: its harness, model and effort, how full its context
is (a meter against the model's window, when that is known), the tokens
and time it has used, and, for Claude Code, a **Subagents** button that
slides its subagents over the right of its terminal, which stays live
beside it: each with its type, model, tasks done (as `n/m` and a
percentage), tokens and run time. On the wall the agent's own task
progress is in the tile's header. The palette's *Agents: Show Activity* opens
all of it for the focused agent in a panel. It is read from Claude Code's transcripts, which the
hooks point Mirador to, so it needs the integration set up; the CLI
equivalent is `mira agent activity`. A Codex agent's activity is its
model, tokens, context and time, read from the session file Codex
writes; it has no task list or subagents to show. There is no cost
figure: tokens are what the transcripts record, and a price would be an
estimate.

In the sidebar, every tab is listed as usual, and directly under the tab
you are on, the agents running in it, each with its status (a "!" square
when it needs you, with what it asks on a second line; a pulsing dot while
it works; a ring when idle), how long it has been so, and its name — the
Claude Code session's, as `claude agents` lists it (given with `--name` or
`/rename`, or made up by Claude Code), else its role, else the harness.
Clicking a tab shows the whole tab — Claude beside your dev server, say;
clicking an agent shows the tab's wall with that agent large (or, with
`agentClick` set to `"pane"`, the agent's pane alone, filling its tab).
Each tab's agents are its own: switch tabs and the tree moves with you,
and every other tab shows a pip per agent with its status. The tab
shortcuts (`mod+1…9`, next and previous tab) go over tabs only, by the
number beside each. At the sidebar's foot, **Inbox** counts the agents that need you,
across every tab, and opens the notifications; **Commands** opens the
palette.

Any pane can be shown alone that way: *Pane: Toggle Zoom* in the palette,
or `mira zoom [pane]` (`--off` for the whole tab again).

Two keys in `mirador.json` shape the wall. `agentClick` is what clicking
an agent in the sidebar does: `"wall"` (the default) shows its tab's wall
in Focus, with that agent large, and `"pane"` shows the agent's pane
alone, filling its tab. `agentsPerTab` is how many agents a page of the
wall's Grid layout holds: `4` (the default), `6` or `9`. Any other value
of either is ignored for its default.

```jsonc
// ~/.config/mirador/mirador.json
{ "agentClick": "pane", "agentsPerTab": 6 }
```


Status comes from the Claude Code hooks, so it needs the integration set up
(an install from an earlier version gains the extra hooks on next launch).

## Terminals that keep running when you quit

Quitting Mirador doesn't end what runs in it: shells, builds, dev servers and
agents keep going, and the next launch reattaches to them — the same
processes, with what they printed while Mirador was closed.

- **Closing a pane** ends it and everything in it, as before.
- **App: Quit and End All Sessions** in the palette (or `mira quit --end-sessions`)
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
palette's **App: What's New** reopens it any time.

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
