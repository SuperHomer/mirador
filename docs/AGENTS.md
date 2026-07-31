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

This wires three hooks, all calling `mira claude-hook`:

- **Notification** → the pane running that Claude session lights up with
  Claude's message (needs-permission, idle, …)
- **Stop** → "finished responding" notification when a turn completes
- **SessionStart** → records the Claude session id on the pane

The hook resolves *which pane* its Claude session runs in from `MIRA_PANE`,
which every process inside a pane inherits, so five parallel agents notify
five different tabs correctly. (On unix it falls back to the parent
process's tty when the environment was lost — over `sudo`, say.)

**Session resume**: because session ids are recorded per pane, restarting
Mirador restores each agent pane idle with `[press any key to rerun:
claude --resume <id>]` — one keypress and the conversation continues.

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

## 5. Workspace control

```bash
mira list-tabs                 # tabs + panes, focus markers (--json for data)
mira new-tab --command "htop"
mira split --dir column --command "npm run dev"
mira focus <pane>
mira close-pane <pane>
```

## 6. Remote workspaces (SSH)

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

## 7. Raw socket protocol

Newline-delimited JSON on the socket named in the discovery file:

```
{"id":1,"cmd":"run","command":"npm test","wait":true}
{"id":1,"ok":true,"data":{"paneId":"…","exitCode":0,"output":"…"}}
```

Verbs: `list_tabs new_tab split_pane close_pane focus_pane send_input
read_screen notify run list_runs agent_session browser_open
browser_navigate browser_snapshot browser_click browser_fill browser_eval
browser_history ssh_open ssh_hosts ssh_forward`. Requests are
snake_case-tagged (`"cmd"`); responses are `{id, ok, data|error}`.

## Other agents (Codex, OpenCode, Gemini CLI…)

Anything that can run a shell command integrates: call `mira notify` from
the agent's finished/attention hooks, and prefer `mira run` for commands
worth watching. Wire their equivalents of Stop/Notification hooks to
`mira notify --title "<agent>" "<message>"`.
