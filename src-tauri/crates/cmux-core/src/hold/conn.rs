//! A holder connection on either platform: a Unix domain socket, or a
//! Windows named pipe opened for overlapped I/O.
//!
//! Overlapped, because a holder connection is full duplex — one thread
//! blocked reading the client's input while another writes output — and
//! Windows serializes every I/O on a *synchronous* handle: the write would
//! wait behind the read forever. (The automation pipe in `transport.rs` is
//! request/response and gets away with synchronous handles; this cannot.)
//! Each read and write here waits on its own event, a read timeout cancels
//! the pending read, and `shutdown` cancels everything pending on the
//! handle from any thread.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use imp::{bind, connect, Conn, Listener};

/// A pane's endpoint within a holder location (see `hold::location`).
pub fn endpoint(location: &Path, pane: &str) -> PathBuf {
    imp::endpoint(location, pane)
}

/// Panes with a live holder in `location`.
pub fn live_panes(location: &Path) -> Vec<String> {
    imp::live_panes(location)
}

pub fn is_alive(endpoint: &Path) -> bool {
    connect(endpoint).is_ok()
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};

    pub struct Conn(UnixStream);
    pub struct Listener(UnixListener);

    impl Conn {
        pub fn try_clone(&self) -> io::Result<Conn> {
            self.0.try_clone().map(Conn)
        }
        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            self.0.set_read_timeout(timeout)
        }
        /// Ends the connection for both sides, waking any blocked reader.
        pub fn shutdown(&self) {
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        }
    }

    impl Read for Conn {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for Conn {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Conn> {
            self.0.accept().map(|(s, _)| Conn(s))
        }
    }

    /// Binds in a directory only this user can enter — the directory, not
    /// the socket, is what keeps other users out, so one that exists
    /// already must be ours and private. A socket file left by a holder
    /// that died is replaced; one with a live holder behind it is an error,
    /// never a takeover.
    pub fn bind(path: &Path) -> io::Result<Listener> {
        // sockaddr_un's path is 104 bytes on macOS, 108 on Linux.
        if path.as_os_str().len() >= 104 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("socket path too long for a Unix socket: {}", path.display()),
            ));
        }
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            let meta = std::fs::symlink_metadata(dir)?;
            let uid = unsafe { libc::geteuid() };
            if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} must be a directory owned by you with mode 0700", dir.display()),
                ));
            }
        }
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("a holder is already serving {}", path.display()),
                ));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener(listener))
    }

    pub fn connect(path: &Path) -> io::Result<Conn> {
        UnixStream::connect(path).map(Conn)
    }

    pub fn endpoint(location: &Path, pane: &str) -> PathBuf {
        location.join(format!("{pane}.sock"))
    }

    /// Sweeps socket files whose holder is gone on the way, so the
    /// directory doesn't accumulate them.
    pub fn live_panes(location: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(location) else {
            return Vec::new();
        };
        let mut panes = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(pane) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".sock"))
            else {
                continue;
            };
            if UnixStream::connect(&path).is_ok() {
                panes.push(pane.to_string());
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        panes.sort();
        panes
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_IO_PENDING,
        ERROR_NO_DATA, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FindClose, FindFirstFileW, FindNextFileW, ReadFile, WriteFile,
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
        WIN32_FIND_DATAW,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    const BUFFER: u32 = 64 * 1024;

    fn wide(path: &Path) -> Vec<u16> {
        OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Closes the handle when the last `Conn` sharing it goes.
    struct Owned(HANDLE);
    // A pipe handle is usable from any thread; overlapped I/O is what makes
    // concurrent use from two of them work.
    unsafe impl Send for Owned {}
    unsafe impl Sync for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub struct Conn {
        handle: Arc<Owned>,
        /// The server end of a pipe: `shutdown` also disconnects it, which
        /// is what the client sees as the end of the stream.
        server: bool,
        /// Set by `shutdown`, shared by every clone.
        closed: Arc<AtomicBool>,
        /// Per clone, like a socket's: only the reader sets one.
        timeout: Mutex<Option<Duration>>,
    }

    impl Conn {
        fn new(handle: HANDLE, server: bool) -> Conn {
            Conn {
                handle: Arc::new(Owned(handle)),
                server,
                closed: Arc::new(AtomicBool::new(false)),
                timeout: Mutex::new(None),
            }
        }

        pub fn try_clone(&self) -> io::Result<Conn> {
            Ok(Conn {
                handle: Arc::clone(&self.handle),
                server: self.server,
                closed: Arc::clone(&self.closed),
                timeout: Mutex::new(None),
            })
        }

        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            *self.timeout.lock().unwrap() = timeout;
            Ok(())
        }

        /// Ends the connection: pending reads and writes on every clone,
        /// in any thread, return at once, and the peer sees the end.
        pub fn shutdown(&self) {
            self.closed.store(true, Ordering::SeqCst);
            unsafe {
                CancelIoEx(self.handle.0, std::ptr::null());
                if self.server {
                    DisconnectNamedPipe(self.handle.0);
                }
            }
        }
    }

    /// The manual-reset event an operation waits on, closed with it.
    struct Event(HANDLE);
    impl Drop for Event {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Runs one overlapped operation to completion (or `timeout`).
    /// `start` issues it against the OVERLAPPED it is given, which lives on
    /// this stack frame until the operation is over — every path out of
    /// here waits for that, including a cancelled one.
    fn overlapped(
        handle: HANDLE,
        timeout: Option<Duration>,
        start: impl FnOnce(*mut OVERLAPPED) -> i32,
    ) -> io::Result<u32> {
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = Event(event);
        let mut ov = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        if start(&mut ov) == 0 {
            let err = unsafe { GetLastError() };
            if err != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            if let Some(timeout) = timeout {
                let ms = timeout.as_millis().min(u32::MAX as u128 - 1) as u32;
                if unsafe { WaitForSingleObject(event.0, ms) } == WAIT_TIMEOUT {
                    unsafe { CancelIoEx(handle, &ov) };
                    // Wait for the cancellation; if the operation finished
                    // in the meantime, its result stands.
                    let mut n = 0u32;
                    if unsafe { GetOverlappedResult(handle, &ov, &mut n, 1) } != 0 {
                        return Ok(n);
                    }
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "read timed out"));
                }
            }
        }
        let mut n = 0u32;
        if unsafe { GetOverlappedResult(handle, &ov, &mut n, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n)
    }

    /// The ways a pipe says its other end is gone, or this one was shut.
    fn is_end(e: &io::Error) -> bool {
        matches!(
            e.raw_os_error().map(|c| c as u32),
            Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_OPERATION_ABORTED | ERROR_NO_DATA)
        )
    }

    impl Read for Conn {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.closed.load(Ordering::SeqCst) || buf.is_empty() {
                return Ok(0);
            }
            let h = self.handle.0;
            let len = buf.len().min(u32::MAX as usize) as u32;
            let timeout = *self.timeout.lock().unwrap();
            match overlapped(h, timeout, |ov| unsafe {
                ReadFile(h, buf.as_mut_ptr(), len, std::ptr::null_mut(), ov)
            }) {
                Ok(n) => Ok(n as usize),
                Err(e) if is_end(&e) => Ok(0),
                Err(e) => Err(e),
            }
        }
    }

    impl Write for Conn {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.closed.load(Ordering::SeqCst) {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let h = self.handle.0;
            let len = buf.len().min(u32::MAX as usize) as u32;
            match overlapped(h, None, |ov| unsafe {
                WriteFile(h, buf.as_ptr(), len, std::ptr::null_mut(), ov)
            }) {
                Ok(n) => Ok(n as usize),
                Err(e) if is_end(&e) => Err(io::ErrorKind::BrokenPipe.into()),
                Err(e) => Err(e),
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn create_instance(name: &[u16], first: bool) -> io::Result<HANDLE> {
        // The default security descriptor gives the creating user and
        // SYSTEM full access; remote clients are refused outright.
        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
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
        /// One idle instance always waiting. Without it, a client arriving
        /// between two accepts finds no pipe at all — "not found", which
        /// reads as "this holder is dead" — instead of a busy one.
        pending: Mutex<Option<Owned>>,
    }

    /// `FILE_FLAG_FIRST_PIPE_INSTANCE` makes a second holder for the same
    /// pane fail here: the race-free form of unix's "is it still live?".
    pub fn bind(endpoint: &Path) -> io::Result<Listener> {
        let name = wide(endpoint);
        let first = create_instance(&name, true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("a holder is already serving {}", endpoint.display()),
                )
            } else {
                e
            }
        })?;
        Ok(Listener {
            name,
            pending: Mutex::new(Some(Owned(first))),
        })
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Conn> {
            let instance = match self.pending.lock().unwrap().take() {
                Some(owned) => owned,
                None => Owned(create_instance(&self.name, false)?),
            };
            let h = instance.0;
            let result = overlapped(h, None, |ov| unsafe { ConnectNamedPipe(h, ov) });
            match result {
                Ok(_) => {}
                // A client got in between creating the instance and
                // waiting on it: already connected, which is a success.
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) => {}
                Err(e) => return Err(e),
            }
            if let Ok(next) = create_instance(&self.name, false) {
                *self.pending.lock().unwrap() = Some(Owned(next));
            }
            // Ownership moves to the Conn; don't let `instance` close it.
            let h = instance.0;
            std::mem::forget(instance);
            Ok(Conn::new(h, true))
        }
    }

    pub fn connect(endpoint: &Path) -> io::Result<Conn> {
        let name = wide(endpoint);
        let mut last = None;
        for _ in 0..5 {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Conn::new(handle, false));
            }
            let err = io::Error::last_os_error();
            // No pipe by that name at all: nothing is serving it.
            if err.kind() == io::ErrorKind::NotFound {
                return Err(err);
            }
            // Every instance busy is transient: the holder is between
            // accepts. Wait for one to free up.
            if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) {
                unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
            }
            last = Some(err);
        }
        Err(last.unwrap_or_else(|| io::ErrorKind::NotFound.into()))
    }

    /// `location` is a pipe-name prefix (`\\.\pipe\mirador-hold-<user>`);
    /// a pane's pipe is that, a dash, and the pane id.
    pub fn endpoint(location: &Path, pane: &str) -> PathBuf {
        PathBuf::from(format!("{}-{pane}", location.display()))
    }

    /// Pipes vanish with their holder, so listing the pipe namespace is
    /// already the list of live holders — no files to sweep.
    pub fn live_panes(location: &Path) -> Vec<String> {
        let location = location.display().to_string();
        let Some(prefix) = location.strip_prefix(r"\\.\pipe\") else {
            return Vec::new();
        };
        let prefix = format!("{prefix}-");
        let pattern = wide(Path::new(r"\\.\pipe\*"));
        let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        let find = unsafe { FindFirstFileW(pattern.as_ptr(), &mut data) };
        if find == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut panes = Vec::new();
        loop {
            let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(260);
            let name = String::from_utf16_lossy(&data.cFileName[..len]);
            if let Some(pane) = name.strip_prefix(&prefix) {
                panes.push(pane.to_string());
            }
            if unsafe { FindNextFileW(find, &mut data) } == 0 {
                break;
            }
        }
        unsafe { FindClose(find) };
        panes.sort();
        panes.dedup();
        panes
    }
}
