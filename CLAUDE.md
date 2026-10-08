# Working on Mirador

A cross-platform terminal (Tauri 2 + React + xterm.js) built for watching AI
coding agents work. Panes are PTYs; a `mira` CLI and Unix socket drive every
action from outside.

Read these first, and don't duplicate them here:

- [`README.md`](README.md) — install, config keys, keybindings, releasing
- [`docs/AGENTS.md`](docs/AGENTS.md) — the `mira` CLI and socket protocol

This file is for what the code and those docs don't tell you.

## Layout

```
src/                        React frontend (the host webview)
  terminal/ diff/ graph/ browser/ whatsnew/ agents/   one directory per pane type
  layout/SplitLayer.tsx     picks which pane component renders a pane id
  bindings.ts               every Tauri command + its DTOs, hand-written
  keymap/                   accelerators, the actions table, the palette's source
src-tauri/
  src/                      app crate: commands.rs, server.rs (socket), intel.rs,
                            whatsnew.rs, update.rs, runs.rs (command-pane runs),
                            browser.rs + browser_bridge.js (injected into pages)
  crates/cmux-core/         PTY, git, diff, graph, notes, config, session, layout
  crates/cmux-protocol/     every DTO crossing Rust↔TS or the socket
  crates/cmux-cli/          the `mira` binary
web/                        the marketing site (separate vite app, own tsconfig)
```

**Logic lives in Rust, the frontend renders.** Parsing, git, and anything
testable belongs in `cmux-core`, which has a test suite CI runs. There is no
JavaScript test runner, so logic put in TypeScript is logic that cannot be
tested — that is the reason `notes.rs` (markdown) and `graph.rs` (lane
placement) are in Rust rather than in the components that draw them.

## Build and check

Run what CI runs — the cargo commands **from `src-tauri`**:

```bash
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test --workspace
npm run build               # repo root; the script is `tsc && vite build`
cd web && npm run build     # only if web/ changed; `tsc -b && vite build`
```

`cargo clippy --manifest-path src-tauri/Cargo.toml` checks **only** the root
package. `cmux-cli` and friends are skipped, and a type error in them sails
through a clean-looking run. Always `--workspace`.

**CI's Rust is whatever stable is current that day**, not what you have.
The workflows install `stable` at run time, so a clippy release with a new
lint fails CI on code that passes locally — PR #75 went red on
`needless_borrows_for_generic_args`, new between the 1.97 on the dev
machine and the 1.99 CI pulled. `rustup update stable` before trusting a
local clippy run. And when CI does fail on a lint, re-run the whole
workspace locally on the new toolchain: cargo stops at the first crate
that fails, so the crates after it were never linted.

**Only Windows CI lints the app crate for Windows.** From macOS,
`cargo clippy --target x86_64-pc-windows-msvc` works for `cmux-core` and
`cmux-protocol` but not the app crate, whose `ring` dependency needs Windows
C headers. So unix-only code there can break the Windows build unseen: a
field or helper read only under `#[cfg(unix)]` is dead code on Windows, and
`-D warnings` makes that an error (#79). Mark it
`#[cfg_attr(not(unix), allow(dead_code))]`, as `pty::Signal` does, and keep
the logic itself in `cmux-core`, where the cross-check reaches.

## Verifying a change

**CI compiles and unit-tests but never mounts the app.** `tsc`, `vite build`,
clippy and the Rust suite all pass without a single pane ever opening, so
whole classes of breakage ride a green suite: a React version mismatch that
stops the tree mounting, a panic on a background thread, a pane drawn
off-window, CSS that makes a row unshrinkable.

Anything touching a pane, the PTY, or startup needs a **second instance**:

```bash
npm run build                                   # a debug binary embeds no frontend
python3 -m http.server 1420 --directory dist &  # ...it loads this instead
SBX=/tmp/msbx-home   # sandbox $HOME, with the real dotfiles symlinked in
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin SHELL=/bin/zsh HOME=$SBX \
  XDG_RUNTIME_DIR=/tmp/msbx XDG_CONFIG_HOME=$SBX/.config-sbx \
  ./src-tauri/target/debug/mirador &
```

Why each part matters:

- **A debug build with `build.devUrl` set embeds no assets** and loads
  `localhost:1420`. With nothing there the window is blank, React never
  mounts, no PTY spawns — and the log is empty, which makes it look mysterious.
- **`SHELL=/bin/zsh` must be set** under `env -i`. A Finder-launched app has
  it; without it the shell falls back to bash and reads the wrong dotfiles.
- **Keep `XDG_RUNTIME_DIR` short** (`/tmp/msbx`): a Unix socket path over
  ~104 chars fails with `SUN_LEN` and the instance comes up with no socket.
- **Give `XDG_CONFIG_HOME` its own directory.** The dev and release apps
  otherwise share a socket and data directory, so only one gets automation —
  and the sandbox would overwrite the discovery file your real instance uses.

State is all environment-derived: session + scrollback under
`$HOME/Library/Application Support/Mirador` (macOS), socket in
`$XDG_RUNTIME_DIR`, config and `socket.json` under `$XDG_CONFIG_HOME/mirador`.

**A symlinked dotfile is the real one.** The sandbox `$HOME` links
`.zshrc`, `.local` and the rest to the user's, so anything deleted or
written *through* those links happens to the real files — a cleanup of
`$SBX/.local/bin/mira` once deleted the user's own `~/.local/bin/mira`.
Never `rm` under a linked path, and keep the app's own writes away from
them: don't link `.claude` (the app repairs and sets up hooks there; give
the sandbox an empty one), and don't click *Set up* in a sandbox whose
`.local` is linked (it writes `~/.local/bin/mira`).

**Sandbox holders outlive the sandbox.** `persistSessions` is on by
default, so every sandbox pane is a `mira __hold` process that keeps running
after the instance quits — that is the feature. Unless persistence is what
you are testing, turn it off in the sandbox's own config
(`$SBX/.config-sbx/mirador/mirador.json`: `{ "persistSessions": false }`).
Holders live under `$XDG_RUNTIME_DIR`, so a sandbox never sees the real
app's; end them with `mira quit --end-sessions` against the sandbox, or by
that path (`pkill -f "mira __hold .*--socket /tmp/msbx/"`) — never with a
bare `pkill -f "mira __hold"`, which would end the user's own sessions.

**Don't seed the sandbox with the real `session.json`.** It recreates your
real panes in the sandbox, and some restored panes act on launch — the
sandbox would be working on your live state, not a copy.

**Drive it with the same environment.** `mira` finds its socket through
`$XDG_CONFIG_HOME/mirador/socket.json`, so a bare `mira` from your own shell
talks to the *real* instance — and `send-input` there types into whatever
pane id you named, possibly the session running you. Use the debug CLI with
the sandbox's variables (a function, not a `$VAR` — zsh does not word-split
an unquoted variable):

```bash
smira() { XDG_RUNTIME_DIR=/tmp/msbx XDG_CONFIG_HOME=$SBX/.config-sbx ./src-tauri/target/debug/mira "$@"; }
```

Then check the three things only a running app shows (`smira list-tabs` gives
pane ids):

```bash
smira send-input --pane <id> $'echo ok\n' && smira read-screen --pane <id>  # PTY + xterm
smira diff --tab        # a pane that mounts proves the React bundle loaded
smira browser open https://example.com --tab && smira browser eval --pane <id> 'document.readyState'
```

A `read-screen` that times out with "pane not mounted?" is the signature of a
render throw. And `send-input` cannot stand in for a keypress on an idle
restored pane — its PTY does not exist yet; that path needs a human.

A release binary embeds `dist` and needs no server, but it embeds whatever
`dist` was on disk at build time — rebuild the frontend first or you are
testing a stale bundle.

**Automated tooling cannot see the UI.** The app draws in the host webview,
which no DOM tooling reaches — browser automation only gets at browser panes'
*child* webviews — and screen capture needs a macOS screen-recording grant
that a CLI process typically does not have. So an agent working here is
blind to the result of its own visual change, and should say so rather than
implying otherwise: measure what can be measured, launch a sandbox instance
with the change in it, and ask a human to look. For a visual bug, remove
*every* candidate cause at once rather than tuning the one you suspect —
guessing one at a time has made things worse here before.

The `web/` site is the exception — `npm run preview` in `web/` (port 4173)
and inspect the real DOM, with the browser tools or `mira browser eval`. Note
it builds with a `/mirador/` base, so serving `dist` at the root 404s every
asset. Query what renders, not the bundle: a string grepped out of the built
JS once passed while the visible listing beside it was still wrong.

Browser panes are the other place you can measure: `mira browser eval` gives
`readyState`, `innerWidth/innerHeight` and body length, which splits "not
loaded" from "loaded but drawn in the wrong place". Don't trust
`screenX/screenY/outerWidth/outerHeight` there — WKWebView in wry reports no
window rect, and a browser-pane offset bug once came from `window.screenY`.

## Adding a pane type

Panes are a pane id plus fields on `PaneMeta`. The whole chain, in order:

1. `src-tauri/crates/cmux-core/src/state.rs` — a field on `PaneMeta` (e.g. `graph_repo`)
2. `cmux-protocol` — a `…Pane` DTO and a `Vec` of them on `WorkspaceSnapshot`
   (plus `Vec::new()` in `Workspace::snapshot`, which the compiler will demand)
3. `src-tauri/src/commands.rs` — build that Vec in `workspace_snapshot`, and
   an `open_*` command that splits or opens a tab
4. `src-tauri/src/lib.rs` — register commands in `generate_handler!`
5. `src/bindings.ts` — the DTO and invoke wrappers
6. `src/layout/SplitLayer.tsx` — dispatch on the snapshot list
7. `src/keymap/actions.ts` — a palette entry; `src/styles.css` — styles
8. `cmux-protocol` `Request` + `server.rs` + `cmux-cli` — socket/CLI parity

Every existing pane type has CLI parity. A new one without it is an
inconsistency reviewers will notice.

## Things that bite

- **A running app keeps executing the binary it started with.** After an
  in-place update the disk and the process disagree about the version.
- **Restored panes must not act on their own.** Command panes and SSH panes
  come back *idle*; relaunching must never re-run `npm test` or silently
  reopen an SSH session. A keypress does it. (Agent panes are the
  exception — see `agents::restore_pane`.)
- **A terminal is drawn in one place.** The agent wall shows agents by
  *moving* each pane's xterm element into its tile (`lendTerminal` in
  `terminal/registry.ts`) and back when the wall's tab is hidden — not a
  second xterm on the same output. That is only sound because one tab is on
  screen at a time, and the PTY has one size: whoever shows the terminal
  sizes it. So the wall leaves out agents in its own tab, and anything new
  that wants to draw a pane's terminal must borrow it the same way.
- **React StrictMode double-mounts in dev** and has twice broken the terminal
  by swapping the output sink out from under a pending attach.
- **A flex row's only shrinkable item absorbs all overflow** and collapses to
  nothing. Bit the diff header (label shrank to `1…`) and the graph row.
- **Tauri versions come in pairs, and Dependabot can't group them.** `tauri
  build` aborts with `Found version mismatched Tauri packages` unless npm
  `@tauri-apps/api` matches cargo `tauri`'s minor (and `plugin-opener`
  matches `tauri-plugin-opener`). It compares `api`, not `cli`, so a
  Dependabot PR for the crate alone can never go green; combine it by hand
  (`npm install @tauri-apps/api@X.Y.Z` + `cargo update -p tauri --precise
  X.Y.Z`). A tauri bump also moves wry/tao and can orphan a
  `[patch.crates-io]` pin — cargo prints `patch … was not used`, but that is
  not a lint, and clippy still exits 0.
- **The intel poller** (`intel.rs`, every 2s) writes `cwd`, branch, repo root
  and ports into `PaneMeta`, but OSC 7 latches `cwd_from_shell` and wins
  permanently. It has to: PowerShell's `cd` moves its own location and never
  the process working directory the poller reads, so on Windows the two
  disagree forever. Mirador injects a shell-integration script to get OSC 7
  out of PowerShell at all.

## Security boundaries

The frontend in `src/` runs in the **privileged webview** that holds the IPC
bridge. Anything remote that reaches it is untrusted:

- **Never render remote content as HTML.** Release notes are parsed in Rust
  into typed blocks the components map to elements; there is no
  `dangerouslySetInnerHTML` anywhere, and raw HTML in a note shows as text.
- **Only `http(s)` links are navigable**, and they open in the real browser
  via the opener plugin.
- **Remote bytes are fetched by Rust, not the page.** Release-note images are
  downloaded, host-checked (`github.com`, `*.githubusercontent.com`), size-
  capped, and format-checked *from the bytes* — SVG is refused because it can
  carry script. They reach the page as raw IPC bytes turned into a blob.
- **Browser panes get no IPC.** They load remote origins, and no capability
  grants those IPC (`capabilities/default.json` has no `remote` entry — keep
  it that way). Automation results leave the page by navigating to a
  `mira-result://` URL that Rust intercepts and cancels. Don't give that
  bridge a shortcut back into Tauri commands.
- Updater payloads are verified against the public key in `tauri.conf.json`.
  There is no Apple signature; the minisign key is the only thing protecting
  an install.

## Conventions

**Commits** explain *why*, in prose, at length — read `git log` before
writing one. Record the mechanism, the alternative not taken, and what was
actually verified. A one-line summary of a non-trivial change will look out
of place.

**Pull requests** carry the same weight: the design decision, what a reviewer
should argue with, and a verification section separating what tests prove
from what a running app proved.

**Big features reach the site.** `web/` is how people find out what
Mirador does, and it goes stale silently: nothing fails when a feature
ships without it. A feature a user would notice — a new pane type, a new
`mira` command, a change to what quitting or restoring does — updates the
site in the same PR or the one right after: its card in
`web/src/components/Features.tsx`, `CliShowcase.tsx` for a new command,
and a showcase section of its own when it is a headline (as the diff pane
and the commit graph have). A changed feature corrects its card — the
"Session persistence" card described the old restore for two releases
after processes started surviving the quit. Shortcuts go through
`BothKeys` in `web/src/components/Keys.tsx`, which shows the macOS and the
Windows/Linux key side by side: the page is static and gets shared, so it
never guesses the reader's OS. Check the rendered page, not the bundle
(see *Verifying a change*).

**Releases** go straight to `main` as a `Release vX.Y.Z` commit touching four
files — `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`
and the lockfile. Then a *draft* release with its notes, then the tag:

```bash
gh release create vX.Y.Z --draft --title "vX.Y.Z — …" --notes-file notes.md
git tag vX.Y.Z && git push origin vX.Y.Z
```

The tag push runs `release.yml`: it checks the tag matches
`tauri.conf.json` and that the draft exists, builds both platforms, attaches
everything, and publishes the draft last. Never publish it by hand — that
reopens the window where `/releases/latest/` names a release with no
`latest-<target>-<arch>.json` and every running app's update check 404s,
which is what the old publish-then-build order did. A failed build leaves
an invisible draft; re-run the failed jobs.

Two things about that, both learned the hard way:

- **The release body is user-facing UI.** The What's New pane renders it after
  an update, and the update manifests carry it verbatim — read from the
  draft at attach time, so edits after publishing reach the pane but never
  the manifests. The workflow refuses a tag with no draft for that reason.
  Images must be on an allowed host — attaching a screenshot to the release
  itself works, since the asset URL is on `github.com`.
- A PR from this repo has the signing secret, so it always takes the signed
  build path. Only Dependabot PRs and forks exercise the keyless one; to test
  that, run `env -u TAURI_SIGNING_PRIVATE_KEY npm run tauri build` locally.
  Broken, it fails hard: "A public key has been found, but no private key."

Gate a release on the sandbox checks under *Verifying a change*, run on the
release commit, not on CI alone — a React/react-dom mismatch that rendered
nothing and three releases of off-window browser panes all shipped green.
