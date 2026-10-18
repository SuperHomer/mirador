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

**A test path like `/u/x` is relative on Windows** — it has no drive.
Anything that checks `is_absolute()` refuses it there, so the test
passes on macOS and fails only in Windows CI (#108). Build test paths
from `std::env::temp_dir()`. And the cross-check above lints tests but
never runs them: only CI finds this, and cargo stops at the first
failing crate, so the crates after it go untested until it is fixed.

## Verifying a change

**CI compiles and unit-tests but never mounts the app.** `tsc`, `vite build`,
clippy and the Rust suite all pass without a single pane ever opening, so
whole classes of breakage ride a green suite: a React version mismatch that
stops the tree mounting, a panic on a background thread, a pane drawn
off-window, CSS that makes a row unshrinkable.

Anything touching a pane, the PTY, or startup needs a **second instance**:

```bash
npm run build                                   # a debug binary embeds no frontend
python3 scripts/serve-dist.py dist &            # ...it loads this instead
SBX=/tmp/msbx-home   # sandbox $HOME, with the real dotfiles symlinked in
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin SHELL=/bin/zsh HOME=$SBX \
  XDG_RUNTIME_DIR=/tmp/msbx XDG_CONFIG_HOME=$SBX/.config-sbx \
  ./src-tauri/target/debug/mirador &
```

Why each part matters:

- **A debug build with `build.devUrl` set embeds no assets** and loads
  `localhost:1420`. With nothing there the window is blank, React never
  mounts, no PTY spawns — and the log is empty, which makes it look mysterious.
- **Serve `dist` uncached** — the script, not `python3 -m http.server`.
  WebKit caches `index.html` from that server and goes on loading the
  bundle it names after a rebuild, without a single request: a sandbox ran
  a two-hour-old frontend while fixes were being measured against it. If
  the server log shows no `GET /` at launch, the page did not reload.
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

**`target/debug/mira` can be an old build.** The app crate bundles `mira`
as a sidecar (`externalBin`), and building it copies
`src-tauri/binaries/mira-<triple>` over `target/debug/mira` — and that
staged copy is refreshed only by `npm run build:cli`, which only
`tauri build` runs. So after a plain `cargo build`, the debug CLI is the
fresh one or the stale sidecar depending on which was written last: hooks
pointed at it once sent a request without a field added days before, and
the feature looked broken. Run `npm run build:cli` before
`cargo build --workspace` when the CLI changed, and check the binary
(`strings target/debug/mira | grep <new field>`) before blaming the code.

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

**Most of that is now a test suite: `crates/cmux-e2e`.** It does the
above in code — a sandbox per test (its own `HOME`, config and runtime
directories, an empty `.zshrc`), the real debug app, requests over the
sandbox's own socket — and fakes the agents: `fake-agent`, linked into the
sandbox's PATH as `claude` and `codex`, logs how it was started, records
its session through the real `mira claude-hook`, and echoes input. As
`claude` it also answers the skills probe (`E2E_SKILLS`), turns each
message — a bracketed paste is one — into a turn with `prompt-submit` and
`stop` hooks (`E2E_TURN_MS`), and raises a permission prompt on `!ask`. So a
test needs no login and costs nothing, and the process table sees a real
`claude` (a shell script would show as `/bin/sh`).

```bash
npm run build && npm run build:cli && (cd src-tauri && cargo build --workspace)
cd src-tauri && MIRA_E2E=1 cargo test -p cmux-e2e -- --test-threads=1
```

Without `MIRA_E2E=1` its tests skip, so `cargo test --workspace` is
unchanged; `.github/workflows/e2e.yml` runs it on macOS for PRs that touch
the app. When you check something by hand in a sandbox, write it down there
instead: wait with `Sandbox::wait_for` on a condition, never a fixed sleep,
and prove a new test fails by putting the bug back once. A failing test
keeps its sandbox (`$TMPDIR/me2e-*`) with the app's and the agents' logs.
It cannot see the window — anything visual still needs a human.

`tests/live.rs` goes further, by hand only: the release bundle, with the
real `claude` (on `claude-haiku-5-5`) and `codex` (on `gpt-6-luna`), up to
a two-member team handing off a job. They spend real tokens, so CI never
runs them — they skip without `MIRA_E2E_LIVE=1`. They use your logins:
`CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`, billed to your Claude
plan) or else `ANTHROPIC_API_KEY`, and a copy of `~/.codex/auth.json`; a
failed run's kept sandbox is scrubbed of them.

```bash
npm run tauri build     # the bundle they run
cd src-tauri && MIRA_E2E=1 MIRA_E2E_LIVE=1 cargo test -p cmux-e2e --test live -- --test-threads=1
```

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
4. `src-tauri/src/lib.rs` — register commands in `generate_handler!`, and
   name each in `src/app_commands.rs`, the app's ACL manifest: one missing
   there is refused at run time, in the app's own UI. `cargo test` names it.
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
- **Browser panes get no IPC.** Tauri's plugins inject their scripts, and
  so `window.__TAURI_INTERNALS__`, into every webview, browser panes
  included: a page in a pane can *try* any command. Two things refuse it.
  The capability (`capabilities/default.json`) names the host webview,
  `"webviews": ["main"]` — not `"windows"`, which also covered the panes,
  child webviews of that window. And a pane never loads the app's own
  origins (`cmux_core::pane_origin`: `tauri:`, `asset:`, `ipc:`, their
  `*.localhost` forms on Windows, a debug build's devUrl) — not typed, not
  by the page, not by a redirect — because Tauri trusts those as *local*.
  And the app has an ACL manifest (`src/app_commands.rs`, granted as the
  `app-commands` set), without which Tauri never checks the ACL for the
  app's own commands from a local page. In v0.1.28 and before, a page
  could send its own pane to `tauri://localhost`, and that pane held every
  command. `tests/browser.rs` (`a_page_in_a_pane_gets_no_command_of_the_app`,
  `a_pane_never_shows_the_apps_own_pages`) pins both. Automation results
  are the return value of a script Rust evaluates in the page
  (`eval_with_callback`); the page only answers. Don't give that bridge a
  shortcut back into Tauri commands, and never give a capability `remote`.
- Updater payloads are verified against the public key in `tauri.conf.json`,
  not against the Apple signature: the minisign key is what protects an
  in-place update. Release builds also carry a Developer ID signature and
  Apple's notarization, which is what lets a *download* past Gatekeeper.

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

Three things about that, all learned the hard way:

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
- **Only tag builds are Developer ID signed and notarized.** The Apple
  certificate and notary key reach `build-macos.yml` on a tag, or on a
  manual run with `gh workflow run build-macos.yml --ref <branch> -f
  notarize=true`, and nowhere else. PR builds are ad-hoc signed
  (`signingIdentity: "-"`). So a PR's green build says nothing about
  notarization; anything touching signing, entitlements or the bundle's
  contents needs that manual run. Then download its DMG, set
  `com.apple.quarantine` on it, and `spctl --assess`. A local build is
  never quarantined, so Gatekeeper never looks at it: v0.1.25 shipped a
  bundle nothing had sealed, every download was reported "damaged", and
  every local launch worked. The workflow's verify step now fails such a
  build.

Gate a release on the sandbox checks under *Verifying a change*, run on the
release commit, not on CI alone — a React/react-dom mismatch that rendered
nothing and three releases of off-window browser panes all shipped green.
