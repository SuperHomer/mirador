//! A file pasted or dropped into a pane becomes a path typed there. In a
//! local pane that is the file's own path, or for a pasted image, the file
//! it was saved as. An SSH pane's program cannot read this computer's
//! files, so there the file goes to the remote first and the path typed is
//! the remote's. Which one a pane gets is decided here, from the pane,
//! never by the page.

use std::time::SystemTime;

use cmux_core::paste;

use crate::AppState;

/// The path to type into `pane` for a pasted image.
pub fn image(state: &AppState, pane: &str, bytes: &[u8]) -> Result<String, String> {
    match remote_host(state, pane)? {
        None => {
            let path = paste::save(&paste::local_dir(), bytes, SystemTime::now())?;
            Ok(path.to_string_lossy().into_owned())
        }
        Some(host) => {
            let name = paste::image_name(paste::image_ext(bytes)?, SystemTime::now());
            upload(&host, vec![(name, bytes.to_vec())]).map(|mut paths| paths.remove(0))
        }
    }
}

/// The paths to type into `pane` for `files`, in their order. A local
/// pane gets them as they are; an SSH pane, the copies made on its remote,
/// all of them checked before any is sent.
pub fn files(state: &AppState, pane: &str, files: Vec<String>) -> Result<Vec<String>, String> {
    let Some(host) = remote_host(state, pane)? else {
        return Ok(files);
    };
    let mut uploads = Vec::new();
    for file in &files {
        let meta = std::fs::metadata(file).map_err(|e| format!("{file}: {e}"))?;
        if meta.is_dir() {
            return Err(format!("{file} is a directory: only files can be sent to an SSH pane"));
        }
        paste::check_size(file, meta.len())?;
        let original = std::path::Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or("");
        uploads.push((paste::sent_file_name(original, SystemTime::now()), std::fs::read(file).map_err(|e| format!("{file}: {e}"))?));
    }
    upload(&host, uploads)
}

/// The remote `pane` is on, if it is an SSH pane; refused when the app has
/// no such pane.
fn remote_host(state: &AppState, pane: &str) -> Result<Option<String>, String> {
    if state.workspace.lock().unwrap().tab_of_pane(pane).is_none() {
        return Err(format!("no pane {pane}"));
    }
    Ok(state.meta.lock().unwrap().get(pane).and_then(|m| m.remote_host.clone()))
}

/// Sends each `(name, bytes)` to `host`'s paste directory and answers
/// their paths there. Then, in the background, the remote's pastes a day
/// old go: a failure there costs nothing but a file left a little longer.
#[cfg(not(windows))]
fn upload(host: &str, files: Vec<(String, Vec<u8>)>) -> Result<Vec<String>, String> {
    let home = cmux_core::ssh::remote_home(host)?;
    let mut paths = Vec::new();
    for (name, bytes) in files {
        // Absolute, not relative to wherever the remote's shell starts:
        // that is its home only as long as nothing says otherwise.
        let path = format!("{home}/{}", paste::remote_rel_path(&name));
        cmux_core::ssh::put_file(host, &path, &bytes)?;
        paths.push(path);
    }
    let host = host.to_string();
    std::thread::spawn(move || {
        let _ = cmux_core::ssh::prune_pastes(&host, &home);
    });
    Ok(paths)
}

#[cfg(windows)]
fn upload(_host: &str, _files: Vec<(String, Vec<u8>)>) -> Result<Vec<String>, String> {
    Err("sending files to an SSH pane is not supported on Windows yet".into())
}
