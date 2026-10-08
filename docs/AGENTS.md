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

## 4. Verify web changes in the browser pane

```bash
mira browser open http://localhost:3000
mira browser snapshot          # element list with stable ids:
# [2] <button> "Submit" type=submit
# [3] <input> "email" type=text value=""
mira browser click 2           # snapshot id — or a CSS selector: "#submit"
mira browser fill 3 "a@b.c"    # React-safe native setters
mira browser eval "document.title"
mira browser navigate /checkout
mira browser back|forward|reload
```

Snapshots cap at 400 elements. The page gets **no** IPC access to Mirador —
automation results travel over an intercepted navigation, so a malicious
page cannot drive your terminal.

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
mira agent roles                              # agentRoles from mirador.json
mira agent new --role reviewer "review the auth change"   # new tab
mira agent new --split "fix the flaky test"   # plain claude, split of this pane
mira agent list                               # pane, status, role, model
mira agent wall                               # the grid of every agent
```

`agent new` opens a shell and types `claude` into it with the role's
`--model`, `--append-system-prompt` and `--name`, and the task as the first
message — so `/exit` leaves a prompt, and the session resumes like any other
(on the role's model). The arguments are quoted for the pane's shell and put
on one line: they are typed, so a newline would submit half a command.

`agent list` reads what the hooks reported: `working`, `needs-you` (with the
message, e.g. the tool awaiting permission), `idle`, or `-` before the first
hook. `--json` adds the cwd, branch and when the status last changed.

## 7. Workspace control

```bash
mira list-tabs                 # tabs + panes, focus markers (--json for data)
mira new-tab --command "htop"
mira split --dir column --command "npm run dev"
mira focus <pane>
mira zoom <pane>               # that pane alone, filling its tab
mira zoom <pane> --off         # the whole tab again
mira close-pane <pane>
mira quit                      # with persistSessions, terminals keep running
mira quit --end-sessions       # ...or end every one of them first
```

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
agent_wall zoom_pane browser_open
browser_navigate browser_snapshot browser_click browser_fill browser_eval
browser_history ssh_open ssh_hosts ssh_forward diff_open`. Requests are
snake_case-tagged (`"cmd"`); responses are `{id, ok, data|error}`.

## Other agents (Codex, OpenCode, Gemini CLI…)

Anything that can run a shell command integrates: call `mira notify` from
the agent's finished/attention hooks, and prefer `mira run` for commands
worth watching. Wire their equivalents of Stop/Notification hooks to
`mira notify --title "<agent>" "<message>"`.
