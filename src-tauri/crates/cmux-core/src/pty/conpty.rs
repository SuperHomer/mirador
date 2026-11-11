//! Which ConPTY panes run under on Windows: the `conpty` key.
//!
//! portable-pty asks Windows for `conpty.dll` by name and falls back to the
//! system's ConPTY in kernel32 only when none is found. A load by name
//! searches the program's own folder and PATH, so a `conpty.dll` in any
//! folder there wins: WezTerm puts one on PATH, with its own
//! `OpenConsole.exe` beside it. Which ConPTY a machine got has depended on
//! what else was installed, and the two do not draw a resized screen the
//! same way. The key makes it a choice:
//!
//! - `auto`, the default: as before, the first `conpty.dll` Windows finds,
//!   else the system's.
//! - `system`: the system's. No `conpty.dll` outside System32 is loaded,
//!   on PATH or beside the program.
//! - a folder: the `conpty.dll` in it, with the `OpenConsole.exe` it
//!   starts beside it, as WezTerm and Windows Terminal ship them.
//!
//! A value that names nothing usable runs panes under the system's, not
//! `auto`: the search `auto` does could find the very folder that failed.
//!
//! portable-pty loads its ConPTY with a process's first PTY and keeps it,
//! so [`prepare`] runs before that. A session holder is a process of its
//! own for each pane, so a new pane takes the key as it is then. Panes
//! the app runs itself (`persistSessions` off) share the app's, settled by
//! its first pane: there, a change applies after a restart.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The ConPTY library, as portable-pty asks for it.
pub const DLL: &str = "conpty.dll";
/// The console host a `conpty.dll` starts, from its own folder.
pub const HOST: &str = "OpenConsole.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conpty {
    Auto,
    System,
    Folder(PathBuf),
}

/// What the key's value asks for. Unset, empty or `auto` is [`Conpty::Auto`].
/// A folder has to be an absolute path holding both [`DLL`] and [`HOST`]:
/// a `conpty.dll` without its host fails to start a console, and every pane
/// with it. Anything else is an error saying why.
pub fn parse(value: Option<&str>) -> Result<Conpty, String> {
    match value.map(str::trim).unwrap_or("") {
        "" | "auto" => Ok(Conpty::Auto),
        "system" => Ok(Conpty::System),
        path => match folder_problem(Path::new(path)) {
            None => Ok(Conpty::Folder(PathBuf::from(path))),
            Some(problem) => Err(format!("conpty {path}: {problem}")),
        },
    }
}

/// The ConPTY a value gives panes, with what to report when it is not the
/// one asked for: a value that names nothing usable gives the system's.
pub fn effective(value: Option<&str>) -> (Conpty, Option<String>) {
    match parse(value) {
        Ok(choice) => (choice, None),
        Err(e) => (Conpty::System, Some(format!("{e}; panes run under Windows' own ConPTY"))),
    }
}

/// Why `dir` is not a ConPTY folder, or `None` when it is one. Short, as
/// the Settings pane shows it beside the folder.
pub fn folder_problem(dir: &Path) -> Option<&'static str> {
    if !dir.is_absolute() {
        Some("not auto, system or the absolute path of a folder")
    } else if !dir.join(DLL).is_file() {
        Some("no conpty.dll in it")
    } else if !dir.join(HOST).is_file() {
        Some("no OpenConsole.exe beside its conpty.dll")
    } else {
        None
    }
}

/// The folders on `path`, a PATH value, that hold a [`DLL`], in PATH order
/// and each once: what `auto` would load is among them, and the Settings
/// pane offers them, each checked by [`folder_problem`] there.
pub fn on_path(path: &OsStr) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() || !dir.join(DLL).is_file() {
            continue;
        }
        if !found.iter().any(|seen| same_folder(seen, &dir)) {
            found.push(dir);
        }
    }
    found
}

/// The same folder written twice on PATH: Windows paths compare without
/// case, and a trailing separator changes nothing.
fn same_folder(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// Settles which ConPTY this process's PTYs run under, once, before the
/// first: portable-pty loads it with the first PTY and keeps it for the
/// life of the process. Whatever cannot be honoured is reported.
pub fn prepare() {
    #[cfg(windows)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let (cfg, _) = crate::config::load();
            let (choice, fallback) = effective(cfg.conpty.as_deref());
            if let Some(why) = fallback {
                eprintln!("mirador: {why}");
            }
            if let Err(e) = settle(&choice) {
                eprintln!("mirador: {e}");
            }
        });
    }
}

/// Applies `choice`. A folder whose conpty.dll does not load (damaged, or
/// built for another architecture) gives the system's, as a value naming
/// nothing usable does, and for the same reason: left as it was, the next
/// load by name would search PATH and could take another conpty.dll.
/// Every way it falls short is in the error, to report.
#[cfg(windows)]
fn settle(choice: &Conpty) -> Result<(), String> {
    match apply(choice) {
        Err(e) if matches!(choice, Conpty::Folder(_)) => match apply(&Conpty::System) {
            Ok(()) => Err(format!("{e}; panes run under Windows' own ConPTY")),
            Err(f) => Err(format!("{e}; and falling back to Windows' own failed: {f}")),
        },
        other => other,
    }
}

#[cfg(windows)]
fn wide(s: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Whether this process already has a module named conpty.dll, which a
/// load by that name is handed whatever the choice.
#[cfg(windows)]
fn conpty_loaded() -> bool {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    let name = wide(OsStr::new(DLL));
    // SAFETY: `name` is a NUL-terminated wide string that outlives the call.
    !unsafe { GetModuleHandleW(name.as_ptr()) }.is_null()
}

#[cfg(windows)]
fn apply(choice: &Conpty) -> Result<(), String> {
    use windows_sys::Win32::System::LibraryLoader::{
        LoadLibraryExW, SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_WITH_ALTERED_SEARCH_PATH,
    };
    if *choice != Conpty::Auto && conpty_loaded() {
        return Err("conpty: a conpty.dll is already loaded in this process; panes run under it".into());
    }
    match choice {
        Conpty::Auto => Ok(()),
        // A load by name searches the program's folder and PATH among
        // others. From here on this process searches System32 alone,
        // where no conpty.dll is, so portable-pty falls back to
        // kernel32's ConPTY, Windows' own; were Windows ever to put a
        // conpty.dll in System32, that would be its own too. It is the
        // whole process's search: in a holder, a process for one pane's
        // PTY; in the app, which opens PTYs itself only with
        // persistSessions off or when a holder fails, for the rest of its
        // run. A dll loaded by its full path is not affected.
        Conpty::System => {
            // SAFETY: plain flags, no pointers.
            if unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) } != 0 {
                Ok(())
            } else {
                Err(format!("conpty system: {}", std::io::Error::last_os_error()))
            }
        }
        // Loaded here by its full path, it is the module named conpty.dll
        // in this process, and a later load by that name is handed the
        // module already loaded, wherever it came from. Never freed: the
        // process's PTYs use it until it exits. The altered search makes
        // the dlls it needs in turn resolve from its own folder first.
        Conpty::Folder(dir) => {
            use windows_sys::Win32::System::Diagnostics::Debug::{
                SetThreadErrorMode, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX,
            };
            let file = wide(dir.join(DLL).as_os_str());
            // A file that is no dll for this process makes the loader ask
            // with a dialog, and a holder has nobody to answer it: this
            // thread fails such a load quietly instead, for this load only.
            let mut before = 0;
            // SAFETY: `before` is a valid place for the old mode; `file` is
            // a NUL-terminated wide string that outlives the call.
            let module = unsafe {
                SetThreadErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX, &mut before);
                let module = LoadLibraryExW(file.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH);
                let error = std::io::Error::last_os_error();
                SetThreadErrorMode(before, std::ptr::null_mut());
                (module, error)
            };
            match module {
                (m, error) if m.is_null() => Err(format!("conpty: loading {}: {error}", dir.join(DLL).display())),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the temp directory, so paths are absolute on
    /// every platform, with the named files in it.
    fn folder(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-conpty-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        dir
    }

    #[test]
    fn unset_empty_and_auto_are_auto_and_system_is_system() {
        assert_eq!(parse(None), Ok(Conpty::Auto));
        assert_eq!(parse(Some("")), Ok(Conpty::Auto));
        assert_eq!(parse(Some(" auto ")), Ok(Conpty::Auto));
        assert_eq!(parse(Some("system")), Ok(Conpty::System));
    }

    #[test]
    fn a_folder_counts_only_with_the_dll_and_its_host() {
        let both = folder("both", &[DLL, HOST]);
        assert_eq!(parse(both.to_str()), Ok(Conpty::Folder(both.clone())));

        let dll_only = folder("dll-only", &[DLL]);
        assert_eq!(folder_problem(&dll_only), Some("no OpenConsole.exe beside its conpty.dll"));
        let err = parse(dll_only.to_str()).unwrap_err();
        assert!(err.contains(HOST) && err.contains(dll_only.to_str().unwrap()), "{err}");

        let empty = folder("empty", &[]);
        assert_eq!(folder_problem(&empty), Some("no conpty.dll in it"));
        assert!(parse(empty.to_str()).is_err());

        assert_eq!(folder_problem(Path::new("WezTerm")), Some("not auto, system or the absolute path of a folder"));
        assert!(parse(Some("WezTerm")).is_err());
        for dir in [both, dll_only, empty] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn on_path_finds_each_folder_with_a_dll_once_in_path_order() {
        let first = folder("path-first", &[DLL, HOST]);
        let none = folder("path-none", &[HOST]);
        let second = folder("path-second", &[DLL]);
        let path =
            std::env::join_paths([first.clone(), none.clone(), second.clone(), first.clone()]).unwrap();
        assert_eq!(on_path(&path), vec![first.clone(), second.clone()]);
        assert!(on_path(OsStr::new("")).is_empty());
        for dir in [first, none, second] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_value_that_names_nothing_usable_gives_the_system_conpty_with_why() {
        assert_eq!(effective(Some("system")), (Conpty::System, None));
        assert_eq!(effective(None), (Conpty::Auto, None));
        let half = folder("effective-half", &[DLL]);
        let (choice, why) = effective(half.to_str());
        assert_eq!(choice, Conpty::System, "not auto, which could load that same conpty.dll from PATH");
        let why = why.unwrap();
        assert!(why.contains(HOST) && why.contains("Windows' own"), "{why}");
        let _ = std::fs::remove_dir_all(half);
    }

    /// The probe the Windows tests below run in a process of their own: it
    /// settles `MIRADOR_CONPTY_PROBE` as `prepare` would, prints what that
    /// reported, then loads conpty.dll by name, as portable-pty does, and
    /// prints the file it got. Run alone, without the variable, it does
    /// nothing.
    #[cfg(windows)]
    #[test]
    #[ignore = "run in a process of its own by the probe tests"]
    fn probe_conpty_load_by_name() {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, LoadLibraryW};
        let Ok(value) = std::env::var("MIRADOR_CONPTY_PROBE") else { return };
        if let Err(e) = settle(&parse(Some(&value)).unwrap()) {
            println!("\nsettle: {e}");
        }
        let name = wide(OsStr::new(DLL));
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call.
        let module = unsafe { LoadLibraryW(name.as_ptr()) };
        if module.is_null() {
            println!("\nprobe: none");
            return;
        }
        let mut file = [0u16; 1024];
        // SAFETY: `file` is writable for its whole length, which is what is passed.
        let n = unsafe { GetModuleFileNameW(module, file.as_mut_ptr(), file.len() as u32) } as usize;
        println!("\nprobe: {}", String::from_utf16_lossy(&file[..n]));
    }

    /// A copy of this test binary in a folder of its own, with a valid dll
    /// named conpty.dll beside it: what a load by name finds first, as a
    /// ConPTY bundled next to mira.exe, or one on PATH, would be. The
    /// loader takes a dll by its file name, so any valid dll does.
    #[cfg(windows)]
    fn probe_binary(name: &str) -> (PathBuf, PathBuf) {
        let dir = folder(name, &[]);
        let exe = dir.join("probe.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let system32 = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32");
        std::fs::copy(system32.join("version.dll"), dir.join(DLL)).unwrap();
        (dir, exe)
    }

    /// What the probe in `exe` reported for `value`, and the conpty.dll a
    /// load by name then got, as the file system resolves it (the loader
    /// may spell a path differently). A probe that does not finish, as one
    /// stopped by a loader dialog would not, fails the test.
    #[cfg(windows)]
    fn run_probe(exe: &Path, value: &str) -> (Option<String>, Option<PathBuf>) {
        use std::io::Read;
        let mut child = std::process::Command::new(exe)
            .args(["--exact", "pty::conpty::tests::probe_conpty_load_by_name", "--ignored", "--nocapture", "--test-threads=1"])
            .env("MIRADOR_CONPTY_PROBE", value)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("the probe for {value} did not finish: stopped by a loader dialog?");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        let line = |key: &str| out.lines().find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()));
        let file = line("probe: ").unwrap_or_else(|| panic!("no probe line: {out}"));
        (line("settle: "), std::fs::canonicalize(file).ok())
    }

    /// `system` must not take a conpty.dll beside the program, which a load
    /// by name searches first. `auto` loads it, so the probe can see one.
    #[cfg(windows)]
    #[test]
    fn system_loads_no_conpty_dll_beside_the_program() {
        let (dir, exe) = probe_binary("beside");
        let beside = std::fs::canonicalize(dir.join(DLL)).unwrap();
        assert_eq!(run_probe(&exe, "auto"), (None, Some(beside.clone())), "auto loads the conpty.dll beside the program");
        let (reported, got) = run_probe(&exe, "system");
        assert_eq!(reported, None);
        assert_ne!(got, Some(beside), "system loaded the conpty.dll beside the program");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A folder with both files whose conpty.dll is no dll passes `parse`,
    /// then fails to load. It must give the system's, as a value naming
    /// nothing usable does: left as it was, the next load by name took the
    /// conpty.dll beside the program. And the failed load asks no dialog.
    #[cfg(windows)]
    #[test]
    fn a_folder_whose_conpty_dll_does_not_load_gives_the_system_conpty() {
        let (dir, exe) = probe_binary("unloadable");
        let broken = folder("unloadable-folder", &[HOST]);
        std::fs::write(broken.join(DLL), b"not a dll").unwrap();
        let (reported, got) = run_probe(&exe, broken.to_str().unwrap());
        assert_ne!(got, std::fs::canonicalize(dir.join(DLL)).ok(), "the next load by name took another conpty.dll");
        let reported = reported.expect("the failed load is reported");
        assert!(reported.contains("loading") && reported.contains("Windows' own"), "{reported}");
        for d in [dir, broken] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_folder_written_twice_differently_is_the_same() {
        assert!(same_folder(Path::new("C:\\Program Files\\WezTerm\\"), Path::new("c:\\program files\\wezterm")));
        assert!(!same_folder(Path::new("C:\\Program Files\\WezTerm"), Path::new("C:\\Program Files\\Other")));
    }
}
