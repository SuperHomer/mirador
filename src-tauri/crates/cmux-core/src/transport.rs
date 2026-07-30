//! Cross-platform local IPC for the automation socket: a Unix domain socket
//! on macOS/Linux, a named pipe on Windows. Both are same-user-only byte
//! streams, so the server and the `mira` CLI share one code path and the
//! protocol above them stays platform-agnostic.

use std::io::{self, Read, Write};
use std::path::Path;

/// A connected automation client (or the CLI's end of one).
pub trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// True if something is already serving this endpoint — i.e. another
/// Mirador owns automation. A dead endpoint (crashed instance) says false,
/// which is what lets `bind` clean up after a crash.
pub fn is_live(endpoint: &Path) -> bool {
    connect(endpoint).is_ok()
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub struct Listener(UnixListener);

    pub fn bind(endpoint: &Path) -> io::Result<Listener> {
        // A stale socket file from a crashed instance blocks bind.
        if endpoint.exists() && !super::is_live(endpoint) {
            let _ = std::fs::remove_file(endpoint);
        }
        if let Some(parent) = endpoint.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let listener = UnixListener::bind(endpoint)?;
        // Owner-only: no other local user can drive this terminal.
        let _ = std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600));
        Ok(Listener(listener))
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Box<dyn Stream>> {
            let (stream, _) = self.0.accept()?;
            Ok(Box::new(stream))
        }
    }

    pub fn connect(endpoint: &Path) -> io::Result<Box<dyn Stream>> {
        Ok(Box::new(UnixStream::connect(endpoint)?))
    }

    pub fn cleanup(endpoint: &Path) {
        let _ = std::fs::remove_file(endpoint);
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    const BUFFER: u32 = 64 * 1024;

    fn wide(path: &Path) -> Vec<u16> {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// One pipe instance. The instance a client connects to is consumed by
    /// that connection, so the listener always keeps the *next* one open —
    /// otherwise a client arriving between two `accept` calls would find no
    /// pipe and fail.
    fn create_instance(name: &[u16], first: bool) -> io::Result<HANDLE> {
        // The default security descriptor grants access to the creating
        // user and SYSTEM only, and remote clients are rejected outright.
        let mut open_mode = PIPE_ACCESS_DUPLEX;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }

    pub struct Listener {
        name: Vec<u16>,
        /// The idle instance waiting for the next client.
        pending: Mutex<Option<HANDLE>>,
    }

    // The pending HANDLE is owned solely by this listener and only touched
    // under the mutex.
    unsafe impl Send for Listener {}
    unsafe impl Sync for Listener {}

    pub fn bind(endpoint: &Path) -> io::Result<Listener> {
        let name = wide(endpoint);
        // FILE_FLAG_FIRST_PIPE_INSTANCE fails if another process already
        // serves this name — the race-free equivalent of the unix
        // "is the stale socket still live?" check.
        let handle = create_instance(&name, true)?;
        Ok(Listener {
            name,
            pending: Mutex::new(Some(handle)),
        })
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Box<dyn Stream>> {
            let handle = {
                let mut pending = self.pending.lock().unwrap();
                match pending.take() {
                    Some(h) => h,
                    None => create_instance(&self.name, false)?,
                }
            };
            let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            // A client that connected between CreateNamedPipeW and
            // ConnectNamedPipe reports ERROR_PIPE_CONNECTED — still a win.
            if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                let err = io::Error::last_os_error();
                unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                return Err(err);
            }
            // Have the next instance listening before we hand this one off.
            if let Ok(next) = create_instance(&self.name, false) {
                *self.pending.lock().unwrap() = Some(next);
            }
            let file = unsafe { File::from_raw_handle(handle as _) };
            Ok(Box::new(file))
        }
    }

    pub fn connect(endpoint: &Path) -> io::Result<Box<dyn Stream>> {
        let mut last: Option<io::Error> = None;
        for _ in 0..5 {
            let err = match OpenOptions::new().read(true).write(true).open(endpoint) {
                Ok(file) => return Ok(Box::new(file)),
                Err(e) => e,
            };
            // No pipe at all: Mirador isn't running — say so immediately
            // instead of making the CLI feel slow.
            if err.kind() == io::ErrorKind::NotFound {
                return Err(err);
            }
            // "All instances busy" is transient (the server is mid-handoff);
            // WaitNamedPipeW blocks until one frees up.
            if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) {
                let name = wide(endpoint);
                unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
            } else {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            last = Some(err);
        }
        Err(last.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "named pipe unavailable")
        }))
    }

    /// Named pipes vanish with their last handle — nothing to unlink.
    pub fn cleanup(_endpoint: &Path) {}
}

/// Starts serving the endpoint. Fails if another process already owns it.
pub fn bind(endpoint: &Path) -> io::Result<Listener> {
    imp::bind(endpoint).map(Listener)
}

/// Connects to a running Mirador's automation endpoint.
pub fn connect(endpoint: &Path) -> io::Result<Box<dyn Stream>> {
    imp::connect(endpoint)
}

/// Releases the endpoint on shutdown (no-op where the OS does it for us).
pub fn cleanup(endpoint: &Path) {
    imp::cleanup(endpoint)
}

pub struct Listener(imp::Listener);

impl Listener {
    /// Blocks until a client connects.
    pub fn accept(&self) -> io::Result<Box<dyn Stream>> {
        self.0.accept()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    #[test]
    fn round_trips_a_line() {
        #[cfg(unix)]
        let endpoint =
            std::env::temp_dir().join(format!("mira-transport-{}.sock", std::process::id()));
        #[cfg(windows)]
        let endpoint =
            std::path::PathBuf::from(format!(r"\\.\pipe\mira-test-{}", std::process::id()));

        let listener = bind(&endpoint).expect("bind");
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            line
        });

        let mut client = connect(&endpoint).expect("connect");
        client.write_all(b"ping\n").unwrap();
        client.flush().unwrap();
        assert_eq!(server.join().unwrap().trim(), "ping");

        cleanup(&endpoint);
    }

    #[test]
    fn an_unserved_endpoint_is_not_live() {
        #[cfg(unix)]
        let endpoint = std::env::temp_dir().join("mira-transport-nobody-here.sock");
        #[cfg(windows)]
        let endpoint = std::path::PathBuf::from(r"\\.\pipe\mira-transport-nobody-here");
        assert!(!is_live(&endpoint));
    }
}
