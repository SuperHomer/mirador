//! What a pane's zsh gets from Mirador: `mira`'s completions, when the
//! person's own setup turned on completion, and nothing else. Skipped
//! unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};

const LONG: Duration = Duration::from_secs(45);

/// A sandbox whose `.zshrc` is `zshrc`, with the sandbox's `mira` on the
/// PATH as a person's would be, launched: its first pane, once it answers.
fn pane_with(sb: &mut Sandbox, zshrc: &str) -> String {
    std::fs::write(sb.home.join(".zshrc"), format!("path=(${{E2E_MIRA:h}} $path)\n{zshrc}")).unwrap();
    sb.launch();
    let pane = sb.first_pane();
    sb.wait_for("the pane to mount", LONG, || sb.screen(&pane));
    pane
}

/// The person's .zshrc runs compinit; by the first prompt `mira` completes,
/// and ZDOTDIR, which Mirador pointed at its own `.zshenv`, is theirs
/// again (unset, as it was). Their options and aliases don't reach the
/// hook that loads it: not `ksh_arrays`, not an alias named `mira` — one
/// in `.zshenv`, which is read before the hook is defined.
#[test]
fn a_zsh_pane_completes_mira_once_the_zshrc_runs_compinit() {
    e2e!();
    let mut sb = Sandbox::new("zcomp");
    std::fs::write(sb.home.join(".zshenv"), "alias mira='echo aliased'\n").unwrap();
    let pane = pane_with(&mut sb, "autoload -Uz compinit && compinit -u\nsetopt ksh_arrays\n");
    sb.type_into(&pane, "print -r -- comp=${_comps[mira]} zdotdir=$ZDOTDIR mz=$MIRADOR_ZDOTDIR.\r");
    sb.wait_for_screen(&pane, "mira's completion, and ZDOTDIR restored", LONG, |s| {
        s.lines().any(|l| l.trim() == "comp=_mira zdotdir= mz=.")
    });
}

/// No compinit, no completion system: Mirador does not start one.
#[test]
fn a_zsh_pane_without_compinit_is_left_without_one() {
    e2e!();
    let mut sb = Sandbox::new("znocomp");
    let pane = pane_with(&mut sb, "");
    sb.type_into(&pane, "echo ok comps=${+_comps} compdef=$+functions[compdef].\r");
    sb.wait_for_screen(&pane, "a working shell with no completion system", LONG, |s| {
        s.lines().any(|l| l.trim() == "ok comps=0 compdef=0.")
    });
}
