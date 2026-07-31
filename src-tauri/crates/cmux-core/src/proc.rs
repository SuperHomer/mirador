//! Background subprocesses.
//!
//! On Windows a GUI process that spawns a console program flashes a console
//! window on screen — once a minute for the PR poll, on every pane close for
//! `taskkill`. `CREATE_NO_WINDOW` suppresses it, so every background command
//! Mirador runs goes through here. (Programs the *user* asked for run in a
//! PTY instead and are unaffected.)

use std::process::Command;

pub fn command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
