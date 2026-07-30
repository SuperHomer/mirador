//! Which program a pane runs, and how a command string is handed to it.
//!
//! Panes are interactive shells; command panes (`mira run`, the 🤖 chip)
//! hand one command line to the same shell non-interactively. Both honor
//! the `shell` / `shellArgs` config keys, so anything from `nu` to Git
//! Bash works — the platform defaults just pick something sane:
//! the login `$SHELL` on unix, PowerShell 7 → Windows PowerShell → `cmd`
//! on Windows.

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use portable_pty::CommandBuilder;

/// How a shell wants a one-off command line handed to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `sh`, `bash`, `zsh`, `fish`, … — POSIX `-lc <cmd>`.
    Posix,
    /// `pwsh` / `powershell` — `-Command <cmd>`.
    PowerShell,
    /// `cmd.exe` — `/C <cmd>`.
    Cmd,
}

impl Kind {
    fn of(program: &str) -> Kind {
        // Windows paths are backslash-separated whichever host parses them
        // (this also runs in tests on macOS), so normalize before splitting.
        let program = program.replace('\\', "/");
        let stem = Path::new(&program)
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match stem.as_str() {
            "pwsh" | "powershell" => Kind::PowerShell,
            "cmd" => Kind::Cmd,
            // Unknown shells are assumed POSIX-ish: `-c` is near-universal.
            _ => Kind::Posix,
        }
    }
}

/// The configured shell (`shell` + `shellArgs` in mirador.json), if any.
/// Read per spawn so an edited config applies to the next pane.
fn configured() -> Option<(String, Vec<String>)> {
    let (cfg, _) = crate::config::load();
    let shell = cfg.shell?;
    if shell.trim().is_empty() {
        return None;
    }
    Some((shell, cfg.shell_args.unwrap_or_default()))
}

#[cfg(not(windows))]
fn platform_default() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

/// PowerShell 7 if it is installed, else the Windows PowerShell that ships
/// with the OS, else `cmd.exe` (which always exists).
#[cfg(windows)]
fn platform_default() -> String {
    if let Some(pwsh) = find_program("pwsh.exe") {
        return pwsh.to_string_lossy().into_owned();
    }
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let windows_powershell = PathBuf::from(&system_root)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    if windows_powershell.exists() {
        return windows_powershell.to_string_lossy().into_owned();
    }
    std::env::var("ComSpec").unwrap_or_else(|_| format!(r"{system_root}\System32\cmd.exe"))
}

/// Looks `exe` up on PATH, plus the default PowerShell 7 install location
/// (a fresh `winget install` does not refresh an already-running Mirador's
/// environment).
#[cfg(windows)]
fn find_program(exe: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let program_files = std::env::var("ProgramFiles").ok()?;
    let candidate = PathBuf::from(program_files).join(r"PowerShell\7").join(exe);
    candidate.is_file().then_some(candidate)
}

/// The shell process with its environment and cwd set, plus how to talk to
/// it and whether the user configured it themselves.
fn base(cwd: Option<&str>) -> (CommandBuilder, Kind, bool) {
    let user_configured = configured();
    let configured_by_user = user_configured.is_some();
    let (program, extra) = user_configured.unwrap_or_else(|| (platform_default(), Vec::new()));
    let kind = Kind::of(&program);
    let mut cmd = CommandBuilder::new(&program);
    for arg in extra {
        cmd.arg(arg);
    }
    // Harmless on Windows, and picked up by anything MSYS/Cygwin-based.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    match cwd {
        Some(dir) if Path::new(dir).is_dir() => cmd.cwd(dir),
        _ => {
            if let Some(home) = crate::config::home_dir() {
                cmd.cwd(home);
            }
        }
    }
    (cmd, kind, configured_by_user)
}

/// The interactive shell for a new pane.
pub fn interactive(cwd: Option<&str>) -> CommandBuilder {
    let (mut cmd, kind, configured_by_user) = base(cwd);
    // A configured shell gets its configured arguments and nothing else —
    // second-guessing a user's `shellArgs` only breaks things.
    if configured_by_user {
        return cmd;
    }
    match kind {
        // Login shell so the user's profile (PATH, prompt) loads.
        Kind::Posix => cmd.arg("-l"),
        Kind::Cmd => {}
        Kind::PowerShell => {
            cmd.arg("-NoLogo");
            // Report the working directory on every prompt (OSC 7) — the
            // only reliable cwd source for PowerShell, whose `cd` does not
            // move the *process* working directory.
            if let Some(script) = shell_integration_script() {
                cmd.arg("-NoExit");
                cmd.arg("-Command");
                cmd.arg(format!(". '{}'", script.replace('\'', "''")));
            }
        }
    }
    cmd
}

/// The same shell, running one command line non-interactively.
pub fn run_command(command: &str, cwd: Option<&str>) -> CommandBuilder {
    let (mut cmd, kind, _) = base(cwd);
    match kind {
        Kind::Posix => {
            cmd.arg("-lc");
            cmd.arg(command);
        }
        Kind::Cmd => {
            cmd.arg("/D");
            cmd.arg("/C");
            cmd.arg(command);
        }
        Kind::PowerShell => {
            cmd.arg("-NoLogo");
            cmd.arg("-NoProfile");
            cmd.arg("-Command");
            // PowerShell's own exit code does not track the last native
            // command's; forwarding $LASTEXITCODE is what makes
            // `mira run --wait` report the real exit code.
            cmd.arg(format!("{command}; exit $LASTEXITCODE"));
        }
    }
    cmd
}

/// Writes (refreshing on every launch) the PowerShell prompt hook and
/// returns its path. `None` if it can't be written — the pane still opens,
/// it just won't report its cwd.
#[cfg(windows)]
fn shell_integration_script() -> Option<String> {
    const SCRIPT: &str = include_str!("shell-integration.ps1");
    let path = crate::config::config_dir().join("shell-integration.ps1");
    let _ = std::fs::create_dir_all(crate::config::config_dir());
    std::fs::write(&path, SCRIPT).ok()?;
    Some(path.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
fn shell_integration_script() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_is_detected_from_the_program_name() {
        assert_eq!(Kind::of(r"C:\Program Files\PowerShell\7\pwsh.exe"), Kind::PowerShell);
        assert_eq!(Kind::of(r"C:\Windows\System32\cmd.exe"), Kind::Cmd);
        assert_eq!(Kind::of("/bin/zsh"), Kind::Posix);
        assert_eq!(Kind::of(r"C:\Program Files\Git\bin\bash.exe"), Kind::Posix);
    }

    #[test]
    fn run_command_passes_the_command_through() {
        let dbg = format!("{:?}", run_command("npm test", None));
        assert!(dbg.contains("npm test"), "got: {dbg}");
    }
}
