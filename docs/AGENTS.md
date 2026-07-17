# Agent cookbook

How AI coding agents (Claude Code, Codex, OpenCode, Gemini CLI, …) plug
into cmux. Everything below works over the `cmux` CLI, which talks to the
running app through a Unix socket (`~/.config/cmux/socket.json` points to
it). `cmux --json …` gives machine-readable output everywhere.

## 1. Light up the tab when you need the human

Any of these fires a notification: the pane gets an attention ring, the
tab lights up with an unread badge, and a native OS notification appears
if the window is unfocused.

```bash
cmux notify "tests are green"                 # OSC escape — works over SSH too
cmux notify --title "Claude" "need approval"
printf '\033]777;notify;Title;Body\033\\'      # raw OSC 777
printf '\033]9;Body\033\\'                     # iTerm2-style OSC 9
```

`cmux notify` prints an escape sequence, so it reaches cmux through any
nesting (tmux, ssh). Use `--socket` to target the app directly instead.

## 2. Claude Code hooks (recommended)

```bash
cmux hooks setup      # idempotent; edits ~/.claude/settings.json
cmux hooks remove     # uninstall
```

This wires three hooks, all calling `cmux claude-hook`:

- **Notification** → the pane running that Claude session lights up with
  Claude's message (needs-permission, idle, …)
- **Stop** → "finished responding" notification when a turn completes
- **SessionStart** → records the Claude session id on the pane

The hook resolves *which pane* its Claude session runs in via its parent
process tty, so five parallel agents notify five different tabs correctly.

**Session resume**: because session ids are recorded per pane, restarting
cmux restores each agent pane idle with `[press any key to rerun:
claude --resume <id>]` — one keypress and the conversation continues.

## 3. Run commands the human can watch (and interrupt)

```bash
PANE=$(cmux run "npm run dev")          # visible command pane, 🤖 chip
cmux run --wait "npm test"              # blocks; prints clean output;
echo $?                                  # exits with the command's code
cmux run --wait --quiet "npm test"      # human-watching variant
cmux runs                                # history: status, duration, command
cmux read-screen --pane "$PANE"         # what's on that pane's screen
cmux send-input --pane "$PANE" $'\003'  # Ctrl-C it
```

`run --wait` makes cmux a drop-in observable executor: same output, same
exit code as running it yourself — but the human sees every line live and
can Ctrl-C in the pane (you'll observe the interrupted exit). Completion
fires a notification automatically.

## 4. Verify web changes in the browser pane

```bash
cmux browser open http://localhost:3000
cmux browser snapshot          # element list with stable ids:
# [2] <button> "Submit" type=submit
# [3] <input> "email" type=text value=""
cmux browser click 2           # snapshot id — or a CSS selector: "#submit"
cmux browser fill 3 "a@b.c"    # React-safe native setters
cmux browser eval "document.title"
cmux browser navigate /checkout
cmux browser back|forward|reload
```

Snapshots cap at 400 elements. The page gets **no** IPC access to cmux —
automation results travel over an intercepted navigation, so a malicious
page cannot drive your terminal.

## 5. Workspace control

```bash
cmux list-tabs                 # tabs + panes, focus markers (--json for data)
cmux new-tab --command "htop"
cmux split --dir column --command "npm run dev"
cmux focus <pane>
cmux close-pane <pane>
```

## 6. Raw socket protocol

Newline-delimited JSON on the Unix socket from
`~/.config/cmux/socket.json`:

```
{"id":1,"cmd":"run","command":"npm test","wait":true}
{"id":1,"ok":true,"data":{"paneId":"…","exitCode":0,"output":"…"}}
```

Verbs: `list_tabs new_tab split_pane close_pane focus_pane send_input
read_screen notify run list_runs agent_session browser_open
browser_navigate browser_snapshot browser_click browser_fill browser_eval
browser_history`. Requests are snake_case-tagged (`"cmd"`); responses are
`{id, ok, data|error}`.

## Other agents (Codex, OpenCode, Gemini CLI…)

Anything that can run a shell command integrates: call `cmux notify` from
the agent's finished/attention hooks, and prefer `cmux run` for commands
worth watching. Wire their equivalents of Stop/Notification hooks to
`cmux notify --title "<agent>" "<message>"`.
