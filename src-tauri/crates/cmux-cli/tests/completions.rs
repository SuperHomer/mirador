//! `mira completions <shell>`: a script the shell can load, made without a
//! running Mirador, listing the commands a person types and not the ones
//! Mirador runs itself.

use std::process::Command;

/// Runs the real `mira completions <shell>`: its stdout, after checking it
/// exited 0.
fn completions(shell: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mira"))
        .args(["completions", shell])
        .output()
        .unwrap();
    assert!(out.status.success(), "exit 0: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn zsh_completes_the_public_commands_only() {
    let script = completions("zsh");
    assert!(script.starts_with("#compdef mira"), "a zsh completion file: {script:.80}");
    for command in ["diff", "run", "agent", "completions"] {
        assert!(script.contains(&format!("'{command}:")), "names `{command}`");
    }
    assert!(!script.contains("__hold"), "the session holder is Mirador's own");
    // completions() rebuilds the CLI without its hidden subcommands, so an
    // attribute of `Cli` can drop out of that copy unseen.
    assert!(script.contains("--version"), "the copy keeps the CLI's version");
}

#[test]
fn powershell_registers_a_completer() {
    assert!(completions("powershell").contains("Register-ArgumentCompleter"));
}
