//! Automation socket location + discovery. The app writes a discovery file
//! so the CLI finds the live socket (and can fail fast with a clear error
//! when cmux isn't running). Unix-only for now; Windows named pipes land
//! with the M11 platform pass.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub socket: PathBuf,
    pub pid: u32,
}

pub fn discovery_path() -> PathBuf {
    crate::config::config_dir().join("socket.json")
}

#[cfg(unix)]
pub fn default_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("cmux.sock");
    }
    let uid = unsafe { libc::getuid() };
    std::env::temp_dir().join(format!("cmux-{uid}.sock"))
}

#[cfg(not(unix))]
pub fn default_socket_path() -> PathBuf {
    PathBuf::from(r"\\.\pipe\cmux")
}

pub fn write_discovery(socket: &std::path::Path) -> std::io::Result<()> {
    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir)?;
    let disc = Discovery {
        socket: socket.to_path_buf(),
        pid: std::process::id(),
    };
    std::fs::write(
        discovery_path(),
        serde_json::to_string_pretty(&disc).unwrap(),
    )
}

pub fn read_discovery() -> Option<Discovery> {
    let text = std::fs::read_to_string(discovery_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remove_discovery() {
    let _ = std::fs::remove_file(discovery_path());
}
