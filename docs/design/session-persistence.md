# Session persistence: terminals that outlive the app

Status: **agreed**, not yet built. The open questions were settled on
2026-10-06 (see *Decisions* at the end); build order below.

## The problem

Quitting Mirador ends every process in it. `PtyManager` (in
`cmux-core/src/pty`) owns each pane's PTY master inside the app process; when
the app exits, the masters close, the kernel hangs up every session, and the
shells, dev servers, builds and agents go with them. Relaunching restores a
*picture* of that state — layout, cwd, saved scrollback, idle command panes,
Claude typed back in — but every process is new.

The ask is tmux's behavior: quit the app, the work keeps running; reopen it,
you are back in the same live shells.

## The proposal in one paragraph

Each terminal pane is owned by a small **holder process**, one per pane, in
the spirit of `dtach`/`abduco`. The holder owns the PTY and the child, keeps
recent output, and listens on a Unix socket. The app stops owning PTYs and
becomes a client: `PtyManager` keeps its interface and talks to holders
instead. Quitting the app detaches; holders keep running. Launching the app
reattaches to every holder the saved session names, replays what happened
while it was away, and falls back to today's restore for any pane whose
holder is gone (after a reboot, say).

## Decisions already made

- **Cmd+Q keeps sessions running.** A separate palette/menu action, *Quit and
  end all sessions*, kills them. Closing a pane always ends its holder.
- **macOS and Linux first.** Windows is a second phase (see the end).
- **Today's restore stays** as the fallback: it is what you get after a
  reboot, and it needs no holder.

## Holder or daemon?

| | One holder per pane | One daemon for all panes (tmux server) |
|---|---|---|
| A crash takes down | one pane | every pane |
| After an in-place update | each holder keeps its old code; only the small wire protocol has to stay compatible | the old daemon runs *all* panes on old code until it is restarted, which kills them |
| Processes | one per pane, idle while its shell is quiet (2.8 MB RSS measured, release build) | one |
| Complexity | protocol + spawn/reattach | the same, plus multiplexing and in-daemon state for every pane |

Per-pane holders keep the failure and version-skew blast radius to one pane.
The process count is the cost, and at this app's scale (tens of panes) it is
a non-issue.

Embedding real tmux was ruled out: macOS doesn't ship it, Windows has none,
and the user's own `~/.tmux.conf` would leak into Mirador's behavior.

## The holder

- **Binary:** the existing `mira` CLI gains a hidden `mira __hold` mode
  rather than a third binary. It is already bundled as a sidecar, and a
  running holder keeps executing the binary it started with — the same rule
  as the app itself — so updates never yank code out from under one.
- **Spawn:** the app runs `mira __hold --pane <id> --socket <path>
  [--cwd <dir>] [--cols N --rows N] [--command <line> | --ssh <host>]`.
  The holder calls `setsid()`, so it is outside the app's process group and
  session, and survives the app's exit. It opens the PTY and spawns the child
  exactly as `PtyManager::spawn` does today (same `MIRA_PANE`, same shell
  selection, same cwd).
- **Socket:** `$XDG_RUNTIME_DIR/mirador/holders/<pane>.sock`, falling back to
  `~/.mirador/holders/<pane>.sock`. Not the data directory: on macOS
  (`~/Library/Application Support/Mirador`) it puts a socket path at ~100 of
  the 104 bytes `sockaddr_un` allows. Not the temp directory: macOS sweeps it
  of files untouched for days, which would cut a long-lived session off from
  its socket. The directory must be owned by the user and `0700`, the socket
  `0600`. Anyone who can connect can type into the shell, so the directory
  permissions *are* the security boundary — same user only. A live socket is
  never taken over; one left by a dead holder is replaced.
- **One client at a time.** A new attach displaces the old one (a crashed
  app's half-open connection must not lock a pane out forever).
- **Exit:** when the child exits, the holder keeps the exit code and its
  buffer until a client has collected them, then exits. If no client comes
  within 24 hours, it exits anyway.

## The wire protocol

Length-prefixed frames over the socket. The first frame each way is a
handshake carrying a **protocol version**; everything after it is versioned
by that number, and new fields are additive only. This is the one interface
that has to stay compatible across releases, so it stays small:

| client → holder | holder → client |
|---|---|
| `Hello { version, cols, rows }` | `Hello { version, pid, exit_code }` |
| `Input(bytes)` | `Output(bytes)` |
| `Resize { cols, rows }` | `Replay(bytes)` — the catch-up, sent once after `Hello` |
| `Kill` | `Exited(code)` |
| `Detach` | |

Frames are `[u32 length][u8 type][payload]`. A type the receiver doesn't
know is skipped, and so are trailing bytes after the fields it does know,
so a newer peer can add both without breaking an older one.

**Flow control** needs no frame of its own. The holder writes `Output` to a
blocking socket, so a client that stops reading fills the socket buffer,
blocks the holder's writer, and through it the PTY and the child — the same
backpressure the app's own watermarks give today, without acknowledgements
in the protocol. Detached, the holder keeps reading into its buffer instead
(`PtyManager::unthrottled`): a shell must not block because nobody is
watching.

## What you see when you reattach

This is the hardest part and the one most worth arguing about. Two ways to
do it:

**A. Raw replay.** The holder keeps a ring buffer of the last N bytes of
output (4 MB). On attach, the app clears the pane, writes the
buffer into xterm, then sizes the PTY to the pane so full-screen programs
redraw. The holder applies the size the client attaches with; it does not
force a redraw when the size is unchanged. Whether real programs come back
right without one is what step 2 measures.

- Cheap: no parsing in the holder, near-zero CPU.
- Mode state rides along when it is inside the buffer (alternate screen,
  mouse tracking, bracketed paste).
- But the buffer can start mid-escape-sequence, and a mode set *before* the
  window is lost. A program that switched to the alternate screen an hour
  and 4 MB of output ago reattaches into the wrong screen until it redraws.

**B. Headless emulator in the holder.** The holder feeds output into a
terminal emulator library (`alacritty_terminal`, or the smaller `vt100`) and,
on attach, sends a serialized snapshot: scrollback, screen, cursor, modes.

- Exact, like tmux: what was on screen is what comes back, regardless of
  how long ago modes were set.
- Costs a real dependency, CPU on every byte of output in every pane, and a
  serializer whose output xterm.js has to agree with.

**Decided: ship A, with the two holes patched.** The holder trims the ring
at a safe boundary (never inside an escape sequence), and tracks a short list
of mode flags itself — alternate screen, mouse modes, bracketed paste,
application cursor keys — by watching for their set/reset sequences, which is
a few dozen lines, not an emulator. It prepends the current modes to the
replay. Then measure against Claude Code, `vim`, `htop` and a long build. If
real programs still come back wrong, B is the escalation, and the protocol
does not change: `Replay` just carries better bytes.

### Replay must not re-fire side effects

The app's OSC scanner turns output into notifications, cwd updates and
titles. Replayed bytes have already been seen once — or happened while the
app was closed. Replaying them through the scanner would re-raise every
`Notification` from the last 4 MB. So `Replay` goes straight to xterm and only
the *last* cwd and title in it are applied; notifications that arrived while
detached are summarized as one ("3 notifications while Mirador was closed")
rather than replayed one by one. Counted by the holder, which knows
whether anyone was attached; the replay alone cannot tell a notification
the app already showed before quitting from one it never saw. The count and
the last one ride in `HolderHello` as trailing fields.

## App changes

- **`PtyManager`** keeps its public surface (`spawn`, `write`, `resize`,
  `ack`, `close`, `set_sink`, `pids`, `is_running`), so `commands.rs`,
  `intel.rs` and `server.rs` don't change. Underneath, `spawn` launches a
  holder and connects; `close` sends `Kill`; `pids` reports the holder's
  child pid, so the intel poller, ports and Claude hook attribution keep
  working.
- **Session file** gains, per pane, the holder socket path. It is derived
  from the pane id, so this is mostly about knowing a holder *should* exist.
- **Launch:** for each pane in the session, try its socket. Alive →
  reattach (the existing `"reattached"` path in `attach_pane`). Gone → today's
  restore. Holders in the directory that no pane names get a tab of their
  own at the end. (The first draft killed them. But the realistic orphan is
  a crash, or an app that died before saving a pane it had just opened —
  exactly when the holder has work worth keeping — so they are reopened,
  and closing the tab ends them like any pane.)
- **Quit:** Cmd+Q sends `Detach` to every holder and exits. *Quit and end all
  sessions* sends `Kill` first.
- **Command panes** (`mira run`) keep running across a restart too; their exit
  code is delivered on reattach. `mira run --wait` from outside does not
  survive the app restarting — out of scope.
- **SSH panes** survive as well: the holder owns the `ssh` client.

## Things that will bite

- **The sandbox.** Holder sockets live under `$XDG_RUNTIME_DIR`, so a sandbox
  instance with its own `XDG_RUNTIME_DIR` never sees the real holders. That
  has to stay true. A shared fallback directory would let a sandbox attach
  the user's live shells.
- **Updates.** An update restarts the app on the new protocol version while
  every holder still speaks the old one. The app must keep speaking at least
  the previous version. Holders started after the update get the new one. If
  a version is ever truly incompatible, the pane must say so rather than
  silently restoring over a live process.
- **Restored panes must not act on their own** still holds. Reattaching is
  not acting: the process never stopped. The fallback restore keeps today's
  rules.
- **StrictMode double-mounts** already swap the output sink under a pending
  attach. A reattach that replays must be idempotent the same way: replay once
  per app run, not once per mount.

## Windows (phase 2)

The design carries over: the holder owns the ConPTY, the socket becomes a
named pipe, and the holder is spawned with `DETACHED_PROCESS` and outside
any job object the app is in, so it isn't killed with the app. What needs
work before it is worth promising is the process side: Windows has no process
groups, so `Kill` needs the tree walk `PtyManager::close` already does, and
the holder must be reachable after the app that created it is gone.

## Build order

Each step is a PR that leaves `main` releasable.

1. **Holder and protocol** in `cmux-core` plus `mira __hold`, with tests that
   spawn a holder around `sh`, attach, detach, reattach and check output,
   replay trimming, mode tracking and exit codes. No app change. CI runs
   these, which is more than any terminal code has had so far. — #78
2. **App on holders**, behind a config key (`persistSessions`, default
   *off*): spawn via holder, reattach on launch, replay without side effects,
   Cmd+Q detaches. — #79
3. ***Quit and end all sessions*** (palette, and `mira quit --end-sessions`),
   orphans reopened in tabs, the notification summary.
4. **The default flipped to on**, after a release of real use.
5. **Windows.**

## Decisions

Settled on 2026-10-06:

1. **Reattach shows raw replay with mode tracking (A).** It gets measured
   against Claude Code, `vim`, `htop` and a long build. A headless emulator
   (B) is the escalation if they come back wrong, with no protocol change.
2. **4 MB of replay per pane.** History older than that comes from the saved
   scrollback.
3. **An exited holder waits 24 hours** for the app to collect its output and
   exit code, then exits.
4. **Step 2 ships off by default** (`persistSessions: false`) for one
   release, so it is tried in a real app before it changes Cmd+Q for
   everyone.
