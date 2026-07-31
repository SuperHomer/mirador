//! Automation endpoint location + discovery. The app writes a discovery
//! file so the CLI finds the live endpoint (and can fail fast with a clear
//! error when Mirador isn't running). The endpoint is a Unix domain socket
//! on macOS/Linux and a named pipe on Windows — see [`crate::transport`].

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
        return PathBuf::from(dir).join("mira.sock");
    }
    let uid = unsafe { libc::getuid() };
    std::env::temp_dir().join(format!("mira-{uid}.sock"))
}

/// Named pipes live in a machine-global namespace, so the pipe name carries
/// the user name — two users on one box each get their own Mirador.
#[cfg(windows)]
pub fn default_socket_path() -> PathBuf {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if user.is_empty() {
        PathBuf::from(r"\\.\pipe\mira")
    } else {
        PathBuf::from(format!(r"\\.\pipe\mira-{user}"))
    }
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
