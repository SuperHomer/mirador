# Agent cookbook

How AI coding agents (Claude Code, Codex, OpenCode, Gemini CLI, …) plug
into Mirador. Everything below works over the `mira` CLI, which talks to
the running app through a local socket — a Unix socket on macOS/Linux, a
named pipe on Windows. The discovery file points at it:
`~/.config/mirador/socket.json` (`%APPDATA%\mirador\socket.json`).
`mira --json …` gives machine-readable output everywhere.

## 1. Light up the tab when you need the human

Any of these fires a notification: the pane gets an attention ring, the
tab lights up with an unread badge, and a native OS notification appears
if the window is unfocused.

```bash
mira notify "tests are green"                 # OSC escape — works over SSH too
mira notify --title "Claude" "need approval"
printf '\033]777;notify;Title;Body\033\\'      # raw OSC 777
printf '\033]9;Body\033\\'                     # iTerm2-style OSC 9
```

`mira notify` prints an escape sequence, so it reaches Mirador through any
nesting (tmux, ssh). Use `--socket` to target the app directly instead.

## 2. Claude Code hooks (recommended)

```bash
mira hooks setup      # idempotent; edits ~/.claude/settings.json
mira hooks remove     # uninstall
```

Mirador offers this on first launch when it finds `~/.claude` (once — "Not
now" is remembered; the palette's *Set Up Claude Code Integration* runs it
later). The hooks call `mira` by its absolute path, so they work whether or
not `mira` is on PATH; if the app moves, the next launch points them at the
new location.

This wires these hooks, all calling `mira claude-hook`, and installs the
`/mira-diff` skill in `~/.claude/skills/mira-diff/`:

- **Notification** → the pane running that Claude session lights up with
  Claude's message (needs-permission, idle, …)
- **Stop** → "finished responding" notification when a turn completes;
  **StopFailure** → "stopped on an error" when an API error ends it
- **SessionStart** → records the Claude session id on the pane
- **UserPromptSubmit**, **PostToolUse**, **PostToolUseFailure** → the
  agent wall's status: working, or back to work after a permission prompt.
  The tool hooks fire on every tool call; the app answers them without a
  lookup unless some agent is waiting on you.

A launch that finds hooks from an earlier version, which installed fewer
events, adds the missing ones — calling the same `mira` the hooks already
call.

`/mira-diff` is the human's side of `mira diff`: typing it in a Claude Code
pane opens the same review surface, with the same arguments. It sets
`disable-model-invocation`, because an agent that wants a diff pane should
run `mira diff` rather than reach for a slash command. Both actions only
touch a file carrying their own marker, so a `/mira-diff` you wrote yourself
is left alone by `setup` and by `remove`.

A skill rather than a `commands/mira-diff.md`: Claude Code merged custom
commands into skills and both spellings still produce `/mira-diff`, but
skills are where new work goes. Its one shell line needs Claude Code
2.1.228 or newer to run.

The hook resolves *which pane* its Claude session runs in from its
terminal: on unix, the pane whose tty it runs on, so five parallel agents
notify five different tabs correctly. A session hosted by Claude Code's
background daemon (`claude --bg`) runs on the daemon's terminal instead, and
belongs to the pane running `claude attach <job>` for it. The inherited
`MIRA_PANE` is used only where there are no ttys (Windows) — the daemon
passes on the `MIRA_PANE` of whichever pane started it to every session it
hosts, so trusting it put every tab's session on the same pane.

**Session resume**: because session ids are recorded per pane, restarting
Mirador brings each agent pane back as the shell it was, with `claude
--resume <id>` (or `claude attach <job>`) typed in for you — no keypress,
and `/exit` leaves you at a prompt. A session you exited before quitting is
forgotten, so it does not come back. On Windows, which cannot yet see Claude
exit, the pane waits for a keypress instead.

## 3. Run commands the human can watch (and interrupt)

```bash
PANE=$(mira run "npm run dev")          # visible command pane, 🤖 chip
mira run --wait "npm test"              # blocks; prints clean output;
echo $?                                  # exits with the command's code
mira run --wait --quiet "npm test"      # human-watching variant
mira runs                                # history: status, duration, command
mira read-screen --pane "$PANE"         # what's on that pane's screen
mira send-input --pane "$PANE" $'\003'  # Ctrl-C it
```

`run --wait` makes Mirador a drop-in observable executor: same output, same
exit code as running it yourself — but the human sees every line live and
can Ctrl-C in the pane (you'll observe the interrupted exit). Completion
fires a notification automatically.

## 4. Test the page you are building, in the browser pane

The browser pane is a real browser the human watches while you drive it.
Use it to check a web change the way a person would: load the page, use
it, wait for it, read its errors.

```bash
mira run "npm run dev"                 # the dev server, in a pane the human sees
mira browser open localhost:5173       # returns once its page has loaded
mira browser snapshot                  # what is on the page, with ids to act on:
# Sign in — http://localhost:5173/
# [1] <h1> "Sign in"
# [2] <input> "email" type=email value=""
# [3] <input> "password" type=password value=""
# [4] <button> "Sign in" type=submit
mira browser fill 2 "ada@example.com"  # sets a value at once
mira browser type 3 "hunter2"          # key by key, for pages that listen to keys
mira browser press Enter               # submits the form, as a browser does
mira browser wait --url /dashboard     # instead of sleeping
mira browser wait --gone .spinner
mira browser text main                 # what the page says (a snapshot lists
                                       # only what you can act on)
mira browser console --errors          # what went wrong on this page
mira browser eval "document.title"     # anything else, as JSON
mira browser screenshot                # a PNG of what the pane shows; prints its path
```

The commands. All but `open` take `--pane <id>`; without it they go to
the active tab's browser pane, else the one used last:

- `open <url> [--tab]`, `navigate <url>`, `back`, `forward`, `reload`.
  `open` waits for the pane to be drawn (up to 5 s; it fails if it never
  is) and then for its page to load (up to 10 s more; past that it
  returns the pane anyway). `navigate` and the others return at once.
  `localhost:5173`, `127.0.0.1:8000` and other loopback hosts open over
  http; any other host without a scheme gets https; `/path`, `?query` and
  `#hash` are taken against the current page.
- `snapshot`: title, URL and up to 400 elements, each with an id that
  stays the same on that page. A select lists its options.
- `click <target>`, `fill <target> <value>`, `type <target> <text>`: a
  target is a snapshot id or a CSS selector. `fill` sets a value at once
  (a select takes an option's value or text). `type` sends keydown,
  keypress, input and keyup per character, typing at the cursor and
  replacing a selection as a person would; a field or editable element
  (`contenteditable`) it has to focus gets the cursor at the end of its
  text, so there it appends. `change` comes
  only when the field loses focus.
- `press <key> [--target <t>]` (a field or editable element it focuses
  gets the cursor at the end): Enter, Tab, Escape, Backspace, Delete,
  Space, the arrows, Home, End, PageUp, PageDown, a letter or digit, with
  `Shift+`, `Control+`, `Alt+`, `Meta+`. Enter in a form's field submits it
  by the browser's rule (through its default button, if it has one); Tab
  moves focus.
- `text [selector]`: the visible text of an element, or of the page, as
  laid out; long text stops at 100,000 characters, saying how many more.
- `wait <selector>`, `wait --text <text>`, `wait --url <part>`,
  `wait --gone <selector>`, each with `--timeout <secs>` (default 10). Met,
  it says so in a line ("#result is shown"); on a timeout it says what it
  waited for and what the page showed instead.
- `console [--errors] [--clear]`: console output, uncaught errors,
  unhandled rejections and failed loads, oldest first, or "(no errors)" /
  "(no console output)". Exits 0 whatever it shows.
- `eval <js>`: the result as JSON; a Promise is awaited.
- `screenshot [--out <path>]`: a PNG of the part of the page the pane
  shows, at screen resolution, written to `--out` or else to a new file
  in the temp directory; prints the path. A pane that is hidden — on the
  board, in another tab, the window minimised — is captured as its page
  is now. macOS and Windows only.

What to know:

- The key and mouse events are synthetic (`isTrusted` is false). A few
  pages ignore them.
- The console log belongs to the page: a navigation or a reload starts it
  empty, so read it before moving on. It keeps the last 500 entries.
- An answer is at most 1 MB, and a command waits up to 10 s for it
  (`wait`: its `--timeout`).
- The URL a pane reports is the last page that finished loading: a
  single-page app's `pushState` routes do not change it. `eval
  location.href` does see them.
- A pane opens web pages only: http, https and `about:blank`, never
  Mirador's own (`tauri://`, its dev server). The page gets **no** access
  to Mirador's commands, so a malicious page cannot drive your terminal.

## 5. Show the human what you changed

```bash
mira diff                      # uncommitted changes, in a reviewable pane
mira diff --staged             # the index against HEAD
mira diff 0e47a2c              # one commit
mira diff main...HEAD          # everything on this branch
mira diff --tab                # new tab instead of a split
mira diff --list-worktrees     # this repository's checkouts
mira diff --worktree feature/x # review another checkout than this pane's
```

A diff pane is the review surface for a turn's work: file tree on the
left, hunks on the right, and the repository taken from the pane you ran
it in — so no path argument. Untracked files are included in the
uncommitted view, because a file git has never seen is still work you
just did. Working-tree diffs re-run whenever the pane regains focus, so
the view never lies about the current state.

When a repository has several worktrees, `--list-worktrees` names them and
marks the one the calling pane sits in; `--worktree` takes either a branch
name or a path and reviews that checkout instead. The diff pane also has a
picker in its header, so a human can move between checkouts without
reopening anything. A bare or stale (prunable) worktree is refused, since
neither has files to diff.

Because the repository comes from the calling pane, `mira diff` refuses to
run outside one: from another terminal it has no way to tell which repo you
mean, and guessing would show the human someone else's work. Inside the app,
the command palette's "New Diff Pane" is the same action.

Opening one when you finish a turn beats asking the human to scroll your
transcript.

```bash
mira graph                     # the commit graph, in a reviewable pane
mira graph --tab               # new tab instead of a split
```

The graph pane draws every ref's history with branch lines, and clicking a
commit opens its diff — it keeps one diff pane and retargets it, so walking
history does not bury the graph under a pane per commit. Like `mira diff`,
it takes the repository from the pane it ran in and refuses outside one.

## 6. Agents with roles, and the wall

```bash
mira agent roles                              # the roles and ways this directory sees
mira agent edit [--tab]                       # a pane to edit them, and teams
mira agent new --role reviewer "review the auth change"   # a pane beside this one
mira agent new --tab "fix the flaky test"     # plain claude, in a tab of its own
mira agent list                               # pane, status, harness, name, model
mira agent wall                               # this tab's agents, in a grid
mira agent activity [--pane <id>]             # its tasks and subagents
```

A role is a Markdown file — its frontmatter says how it starts, its body is
its prompt — and may follow a *way of working*, a playbook in `ways/` that
any role can share:

```markdown
<!-- .mirador/roles/reviewer.md -->
---
harness: claude        # or codex; Claude Code when absent
model: haiku
effort: high           # how hard the model thinks; its default when absent
way: our-way           # .mirador/ways/our-way.md, appended after this prompt
description: Reviews diffs, never edits
---
You review. Never edit files.
```

Roles come from the nearest `.mirador/roles/` at or above the pane's
directory (the repository's, committed and shared), then the config
directory's `roles/` (yours, everywhere), then `agentRoles` in mirador.json;
the same name means the first of those. Ways of working resolve the same
way. `agent roles` prints what the current directory sees, with each
role's source, and warns about anything it could not use — a file that
does not parse, an unknown field, a way nobody wrote; `--json` gives the
whole catalog, including `problem` on a role that cannot start.

`effort` goes to Claude Code as `--effort` (`low`, `medium`, `high`,
`xhigh`, `max`) and to Codex as `-c model_reasoning_effort="…"` (`low`,
`medium`, `high`, `xhigh`, `max`, `ultra` — Codex itself refuses one the
model does not support, and only some models take `ultra`). Lower case
only: any other value is the role's `problem`, which names the accepted
ones, and the role is listed greyed out until it is fixed.

A Claude Code role may also list what it is allowed: `mcp: [names]`,
`plugins: [names]`, `skills: [names]` (only those; `[]` is none; absent is
everything) and `tools: { allow: [...], deny: [...] }` in Claude Code's
permission syntax. They become one settings file passed with `--settings`
(`allowedMcpServers`, `enabledPlugins`, `skillOverrides`, `permissions`),
kept with the pane so a resume passes it again. `skills` and `plugins`
first ask `claude` itself what exists, in the agent's directory; an
unknown name stops the launch with the list. A repository's role may not
`allow` tools, and a Codex role may not use any of these yet — both are
reported as the role's `problem`.

`agent new` splits the pane it runs in (the focused pane, from outside
Mirador), opens a shell there and types the agent into it, with the task as
the first message — so `/exit` leaves a prompt. Claude Code gets the role's
`--model`, `--effort`, `--name`, and its prompt (the role's, then its
way's) through `--append-system-prompt-file`; the session resumes like any
other, on the role's model and effort, and keeps that prompt. Codex gets
`--model`, `model_reasoning_effort` and the prompt as
`-c developer_instructions="…"`; it has no session Mirador can resume.
It also gets `-c` overrides for a permissions profile, `mirador`, that
extends the one Codex would run on — your `default_permissions`, else
`:workspace` in a trusted project and `:read-only` elsewhere — with
Mirador's socket and nothing else (network only through Codex's proxy),
so its `mira` reaches the app. They are left out when your Codex config
uses `sandbox_mode` (Codex then ignores profiles), gives full access, or
cannot be read or parsed.
A `-c` override makes Codex run in its embedded mode rather than on its
shared background server, which it says once at startup ("Running without
the shared background server"); the agent works the same, but `codex
agents` does not list it. A team member goes by its member name in `agent
list` and the app (`member`), a Codex one included. The
arguments are quoted for the pane's shell and put on one line: they are
typed, so a newline would submit half a command.

`agent list` and the wall cover every agent, not only Claude Code: on macOS
and Linux a pane whose terminal runs `codex`, `opencode`, `pi`, `gemini`,
`amp`, `cursor-agent`, `copilot`, `aider`, `goose`, `qwen`, `crush` or
`droid` (directly, or under `node`/`bun`/`python`) is an agent for as long
as that process runs, with the `--model`/`-m` it was started with. Their
subcommands that serve rather than converse (`codex mcp-server`, `opencode
serve`, …) are not. Only Claude Code has hooks, so only its agents have a
status and resume after a restart.

A Claude Code agent goes by its session's name, as `claude agents` lists
it: read from Claude Code's own session file (`~/.claude/sessions/<pid>.json`,
matched by the process on the pane's terminal, the hooks' session, or the
job a `claude attach` names), whether given with `--name` or `/rename` or
made up by Claude Code. Without one it goes by its role. The same file's
busy/idle is the status of an agent whose hooks have not reported one.

`agent activity` (or right-clicking the agent in the app) reads a Claude
Code agent's transcripts: the session's tokens, context and task list
(`TaskCreate`/`TaskUpdate`, or `TodoWrite`), then each subagent under
`<session>/subagents/` with its state (`running`, `done`, or `stopped`
when the Claude Code hosting it is gone), tasks done/total, tokens,
context and run time. The hooks tell Mirador where the transcript is
(`transcript_path`), and only a `<session_id>.jsonl` path is accepted.

For Claude Code, `agent list` reads what the hooks reported: `working`, `needs-you` (with the
message, e.g. the tool awaiting permission), `idle`, or `-` before the first
hook. `--json` adds the cwd, branch and when the status last changed.

### Teams

```bash
mira team list --defined                 # teams this directory can start
mira team start squad "add OAuth"        # a tab of its own; the task goes to the lead
mira team list                           # running teams: each member's state and pane
mira team add reviewer [--name gate]     # another member, beside this pane
mira team restart [member…]              # bring back exited or closed members
```

A team is `teams/<name>.md` (the repository's `.mirador/teams`, then the
config directory's): frontmatter `members` — each a `role`, with an
optional `name`, `count` (named `<name>-1`, `-2`, …) and `lead` — plus
`way`, `layout` (`grid`, `columns`, `rows`) and `description`; the body is
its charter. Each member's prompt is its role's, then a section naming the
team, its teammates and the `mira` commands to find them (by absolute
path: the agent's shell may not have `mira` on its PATH), the charter, and
the team's way of working unless the role already follows it. A Claude
member's session is named after the member.

The running team is kept with its tab (`team` on a tab of the workspace
snapshot, persisted in the session). A member is `running` while its pane
runs an agent, `exited` when the pane is back to a shell, `closed` when
the pane is gone. `team restart` types an exited member's command into its
pane only if the shell is idle (on macOS and Linux it checks the
terminal's processes; anything else on it is `busy`), and gives a closed
member a new pane beside a teammate. `team add` and `team restart` act on
the team of the calling pane's tab.

Built in: Mirador ships the `omakase` team, its roles `omakase-planner`,
`omakase-worker` (Claude Code, `claude-opus-5-5`) and `omakase-reviewer`
(Codex, `gpt-6-luna`), and the `omakase` way of working
(`src-tauri/crates/cmux-core/builtin/`). They are listed with source
`builtin`, below `agentRoles` and every file: a file or entry of the same
name replaces one. The reviewer reports back with `mira agent send planner`
— nobody can `wait` on a Codex agent — and Codex's default sandbox asks
before letting it reach Mirador's socket.

### Handoffs

```bash
mira agent send <name|pane> <message…> [--force] [--wait [--timeout N]]
mira agent wait <name|pane> [--timeout N]     # 0 idle, 2 needs you, 3 exited, 124 timeout
```

`send` resolves a name among the agents of the calling pane's tab — a
team member's name first, then a Claude session's, then a role's; the
first kind that matches must match exactly one pane — or takes any pane id.
It writes the message as a bracketed paste (`ESC[200~ … ESC[201~`), every
control character but newline and tab removed so nothing in it can end
the paste, then Enter as a separate write. When the caller is an agent the
message opens with `[message from <name>, via mira agent send]`. Refused:
a pane that runs no agent, the caller's own pane, a pane whose terminal has
not started, and an agent whose status is `needsYou` unless `--force`.

`wait` polls `agent list` once a second and ends on a status that changed
after it started, or after it saw the agent working — so a wait begun
right after a `send` does not end on the idle from before the message. It
reads status from Claude Code's hooks; an agent without them only ends a
wait by exiting.

## 7. Workspace control

```bash
mira list-tabs                 # tabs + panes, focus markers (--json for data)
mira new-tab --command "htop"
mira split --dir column --command "npm run dev"
mira focus <pane>
mira zoom <pane>               # that pane alone, filling its tab
mira zoom <pane> --off         # the whole tab again
mira close-pane <pane>
mira close-tab <tab>           # the tab and every pane in it
mira view                      # which view is shown: tabs or board
mira view board                # every tab on one board
mira view tabs                 # back to tabs
mira quit                      # with persistSessions, terminals keep running
mira quit --end-sessions       # ...or end every one of them first
```

The board shows every tab as a frame and every pane as a card; where a
card sits on the board and how its tab is split are separate layouts, and
neither changes the other. A pane created with `mira` lands in a free spot
of its tab's frame, and a new tab's frame lands at the centre of the view
if that is free, else in the free place beside a frame nearest to it,
else right of every frame. A person can repack every frame into rows,
in tab order, with Arrange, so a frame's place on the board is not
something to rely on.

## 8. Remote workspaces (SSH)

```bash
mira ssh hosts                     # aliases from ~/.ssh/config
mira ssh open prod                 # remote pane running `ssh -tt prod`
mira ssh open "-p 2222 user@box"   # or a full destination with options
mira ssh open prod --tab
mira ssh forward 3000              # remote localhost:3000 -> your localhost:3000
mira ssh unforward 3000
```

The pane runs the *system* ssh, so agent auth, 2FA prompts, and ProxyJump
behave exactly as in any terminal. On macOS/Linux a shared ControlMaster
connection backs the forwards, so they need no second login; Windows'
OpenSSH has no ControlMaster, so each forward is its own `ssh -N -L`
process and authenticates again.

Forwarding is what makes a remote dev server reviewable: `mira ssh forward
3000` then `mira browser open http://localhost:3000` and the browser pane
renders the remote app.

Disconnects leave the pane idle with `[press any key to reconnect]`;
restarting Mirador restores remote panes idle too — it never re-opens an
SSH session behind your back.

## 9. Raw socket protocol

Newline-delimited JSON on the socket named in the discovery file:

```
{"id":1,"cmd":"run","command":"npm test","wait":true}
{"id":1,"ok":true,"data":{"paneId":"…","exitCode":0,"output":"…"}}
```

Verbs: `list_tabs new_tab split_pane close_pane focus_pane send_input
read_screen notify run list_runs agent_session agent_new agent_list
agent_wall zoom_pane browser_open browser_navigate browser_snapshot
browser_click browser_fill browser_type browser_press browser_text browser_eval
browser_history browser_wait browser_console browser_screenshot ssh_open ssh_hosts
ssh_forward diff_open`. Requests are
snake_case-tagged (`"cmd"`); responses are `{id, ok, data|error}`.

## Other agents (Codex, OpenCode, Gemini CLI…)

They count as agents with no setup — see section 6. For notifications,
anything that can run a shell command integrates: call `mira notify` from
the agent's finished/attention hooks, and prefer `mira run` for commands
worth watching. Wire their equivalents of Stop/Notification hooks to
`mira notify --title "<agent>" "<message>"`.
