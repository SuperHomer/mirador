//! Opening a pane's directory in the person's own editor: mirador.json's
//! `externalEditor`, VS Code's `code` unless set.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// The editor when `externalEditor` is unset or blank.
pub const DEFAULT: &str = "code";

/// The program and its arguments for opening `dir`: `editor` split plainly
/// on whitespace ("subl -n" is `subl` with `-n`), the directory last and
/// whole, spaces and all. A program whose path has a space needs a
/// wrapper script: there is no quoting to keep it one word.
pub fn command_line(editor: &str, dir: &Path) -> Result<(String, Vec<OsString>), String> {
    let mut words = editor.split_whitespace();
    let program = words.next().ok_or("externalEditor is empty: no editor to open")?;
    let mut args: Vec<OsString> = words.map(OsString::from).collect();
    args.push(dir.as_os_str().to_owned());
    Ok((program.to_string(), args))
}

/// `program` as found in `path`'s directories, in order, trying each of
/// `exts` after a bare name (`""` for the name as it is); a name with an
/// extension of its own (`code.cmd`) is looked for as it is. A program
/// given with a directory is taken as it is.
///
/// On Windows the extensions matter: VS Code puts `code.cmd` on PATH, and
/// beside it a `code` shell script for Git Bash, which Windows cannot run.
pub fn find_on_path(program: &str, path: &OsStr, exts: &[&str]) -> Option<PathBuf> {
    if Path::new(program).components().count() > 1 {
        return Some(PathBuf::from(program));
    }
    let exts = if Path::new(program).extension().is_some() { &[""][..] } else { exts };
    std::env::split_paths(path).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

/// What a program named without an extension may be on disk.
#[cfg(windows)]
const EXTS: &[&str] = &[".exe", ".cmd", ".bat", ".com"];
#[cfg(not(windows))]
const EXTS: &[&str] = &[""];

/// Starts `editor` on `dir` and returns once it has started, not once it
/// exits.
///
/// It is looked up on the user's PATH: launched from Finder, the app's own
/// is launchd's, where neither `code` nor anything installed by Homebrew
/// is, so the interactive shell's is used, as command panes use it. On
/// Windows the lookup also finds `code.cmd`, which `Command` runs through
/// `cmd.exe` itself, escaping the arguments, when given the whole path.
pub fn open(editor: &str, dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory here", dir.display()));
    }
    let (program, args) = command_line(editor, dir)?;
    #[cfg(unix)]
    let path = crate::pty::shell::interactive_path().map(OsString::from);
    #[cfg(not(unix))]
    let path: Option<OsString> = None;
    let path = path.or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let exe = find_on_path(&program, &path, EXTS)
        .ok_or_else(|| format!("{program} is not on your PATH: set externalEditor in Settings"))?;
    let mut cmd = crate::proc::command(&exe.to_string_lossy());
    cmd.args(&args)
        .current_dir(dir)
        .env("PATH", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("could not start {program}: {e}"))?;
    // Reaped when it exits, which `code` and the like do at once, handing
    // the window to the editor already running.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_split_plainly_and_the_directory_comes_last_whole() {
        let dir = std::env::temp_dir().join("a project");
        let (program, args) = command_line("subl  -n --wait", &dir).unwrap();
        assert_eq!(program, "subl");
        assert_eq!(args, vec![OsString::from("-n"), "--wait".into(), dir.clone().into_os_string()]);

        let (program, args) = command_line(" code ", &dir).unwrap();
        assert_eq!((program.as_str(), args), ("code", vec![dir.into_os_string()]));
    }

    #[test]
    fn an_empty_command_is_refused() {
        assert!(command_line("  ", &std::env::temp_dir()).is_err());
    }

    #[test]
    fn a_program_is_found_by_its_extensions_in_path_order() {
        let root = std::env::temp_dir().join(format!("mira-editor-path-{}", std::process::id()));
        let (first, second, empty) = (root.join("first"), root.join("second"), root.join("empty"));
        for d in [&first, &second, &empty] {
            std::fs::create_dir_all(d).unwrap();
        }
        // VS Code's bin on Windows: a shell script and the .cmd beside it.
        std::fs::write(second.join("code"), "").unwrap();
        std::fs::write(second.join("code.cmd"), "").unwrap();
        std::fs::write(first.join("zed"), "").unwrap();
        std::fs::write(second.join("zed"), "").unwrap();
        // A directory with the program's name is not the program.
        std::fs::create_dir_all(empty.join("subl")).unwrap();
        let path = std::env::join_paths([&empty, &first, &second]).unwrap();

        let windows = [".exe", ".cmd", ".bat", ".com"];
        assert_eq!(find_on_path("code", &path, &windows), Some(second.join("code.cmd")));
        // Named with its extension, as it is.
        assert_eq!(find_on_path("code.cmd", &path, &windows), Some(second.join("code.cmd")));
        assert_eq!(find_on_path("code", &path, &[""]), Some(second.join("code")));
        assert_eq!(find_on_path("zed", &path, &[""]), Some(first.join("zed")));
        assert_eq!(find_on_path("subl", &path, &[""]), None);
        assert_eq!(find_on_path("cursor", &path, &[""]), None);
        // With a directory, as it is.
        let given = first.join("zed");
        assert_eq!(find_on_path(&given.to_string_lossy(), &path, &windows), Some(given));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_directory_that_is_not_there_is_refused_before_anything_starts() {
        let gone = std::env::temp_dir().join(format!("mira-editor-gone-{}", std::process::id()));
        let err = open("code", &gone).unwrap_err();
        assert!(err.contains("not a directory"), "{err}");
    }
}
