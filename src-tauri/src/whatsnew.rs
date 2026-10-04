//! The "What's New" pane: the release notes for the running version, shown
//! once after an update.
//!
//! Notes are the GitHub release body for this build's tag, which is where
//! they are already written — so nothing in the repository has to be kept
//! in sync with them, and a note can still be corrected after a release.
//! Fetched once per version and cached, so reopening the pane later works
//! on a plane.
//!
//! Showing it is deliberately quiet: a fresh install gets nothing (there is
//! no "new" yet), a failed fetch gets nothing, and the pane never steals
//! focus from the tab you were working in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppState;

/// Where the notes come from. The repository is pinned rather than derived
/// from the updater endpoint: this only ever fetches public release notes
/// for this product, and a config-driven URL would be a way to point the
/// pane at someone else's text.
const REPO: &str = "SuperHomer/mirador";

/// GitHub rejects API requests without one.
const USER_AGENT: &str = "mirador-whats-new";

/// Fetching is best-effort and must never hold up a launch.
const FETCH_TIMEOUT_SECS: u64 = 10;

/// One release's notes, as the pane renders them.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    /// The version these notes describe, e.g. "0.1.16".
    pub version: String,
    /// Release title ("v0.1.15 — no more black stripe under the terminal").
    pub title: String,
    /// The body, parsed into blocks. Structured rather than raw markdown so
    /// the frontend renders elements instead of interpreting remote text.
    pub blocks: Vec<cmux_protocol::NoteBlock>,
    /// The release page, for the pane's "open on GitHub" link.
    pub url: String,
}

/// The version last launched, remembered so an update can be noticed.
/// Separate from session.json: a corrupt or deleted session must not make
/// the app re-announce a version the user has already seen, and this file
/// stays meaningful when the session is reset.
fn seen_path() -> PathBuf {
    cmux_core::session::data_dir().join("last-version.txt")
}

fn cache_path(version: &str) -> PathBuf {
    // One file per version: the previous version's notes stay readable, and
    // a version string cannot escape the directory because it comes from
    // the binary's own package info, not from input.
    cmux_core::session::data_dir().join(format!("release-notes-{version}.json"))
}

fn read_seen() -> Option<String> {
    let text = std::fs::read_to_string(seen_path()).ok()?;
    let seen = text.trim().to_string();
    (!seen.is_empty()).then_some(seen)
}

fn write_seen(version: &str) {
    let path = seen_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, version) {
        eprintln!("mirador: could not record the running version: {e}");
    }
}

/// Decides whether this launch follows an update, and records the version
/// either way so the question is only ever asked once per upgrade.
///
/// A first install has nothing to compare against and shows nothing: the
/// notes for a version you just chose to download are not news. A
/// *downgrade* counts as a change too — whatever the user is now running
/// is what they may want to read about.
fn updated_version(current: &str) -> Option<String> {
    let seen = read_seen();
    write_seen(current);
    match seen {
        Some(seen) if seen != current => Some(current.to_string()),
        _ => None,
    }
}

/// Cached notes, else GitHub. Caching the fetch means the pane can be
/// reopened from the palette offline, which is where it is read a second
/// time.
pub fn notes_for(version: &str) -> Result<ReleaseNotes, String> {
    if let Some(cached) = std::fs::read_to_string(cache_path(version))
        .ok()
        .and_then(|t| serde_json::from_str::<ReleaseNotes>(&t).ok())
    {
        return Ok(cached);
    }
    let notes = fetch(version)?;
    if let Some(parent) = cache_path(version).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&notes) {
        let _ = std::fs::write(cache_path(version), json);
    }
    Ok(notes)
}

/// Installs a rustls crypto provider if the process has none.
///
/// `tauri-plugin-updater` selects reqwest's `rustls-no-provider`, which
/// leaves the choice to the binary; building a client without one *panics*.
/// The updater installs it when its own first check runs, ten seconds after
/// launch, which is after this module wants to fetch — so whoever arrives
/// first installs it. `install_default` errs when that has already
/// happened, and the error is the answer, not a problem.
fn ensure_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// The release body for `v<version>`. Blocking: every caller is already on
/// a worker thread or an async command, and a blocking client keeps this
/// module free of a runtime handle.
fn fetch(version: &str) -> Result<ReleaseNotes, String> {
    ensure_crypto_provider();
    let url = format!("https://api.github.com/repos/{REPO}/releases/tags/v{version}");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|e| format!("could not reach GitHub: {e}"))?;
    if !response.status().is_success() {
        // A release that exists but has no notes, an unpublished tag, or a
        // rate limit all land here, and all mean the same thing to the
        // caller: there is nothing to show.
        return Err(format!("no release notes for v{version} ({})", response.status()));
    }
    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let body = json["body"].as_str().unwrap_or("").trim().to_string();
    if body.is_empty() {
        return Err(format!("release v{version} has no notes"));
    }
    Ok(ReleaseNotes {
        version: version.to_string(),
        title: json["name"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&format!("v{version}"))
            .to_string(),
        blocks: cmux_core::notes::parse(&body),
        url: json["html_url"]
            .as_str()
            .unwrap_or(&format!("https://github.com/{REPO}/releases/tag/v{version}"))
            .to_string(),
    })
}

/// Hosts a release-note image may be fetched from.
///
/// A release body is authored by whoever can publish a release, and these
/// are where GitHub actually stores note images — an upload lands under
/// `github.com/user-attachments`. Narrowing to them means a release body
/// cannot make the app fetch from an arbitrary host, and anything else
/// degrades to a link the user can choose to open.
const IMAGE_HOSTS: &[&str] = &["github.com", "githubusercontent.com"];

/// Per image, and across one release. A note is a page to read, not a
/// payload: a demo GIF is worth a few megabytes, a disk image is not.
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// What a stored image may be. Checked against the bytes themselves rather
/// than a header or a file extension, so nothing but a raster image is
/// ever written to disk or handed to the webview — an SVG, which can carry
/// script, does not qualify.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("png");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpeg");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.len() > 12 && &bytes[4..12] == b"ftypavif" {
        return Some("avif");
    }
    None
}

/// True when `url` is an https URL on a host we will fetch an image from.
fn allowed_image_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    IMAGE_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// A stable, filesystem-safe name for a url. FNV-1a: this is a cache key,
/// not a signature — it only has to be the same next launch.
fn image_id(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn image_path(url: &str) -> PathBuf {
    cmux_core::session::data_dir()
        .join("release-images")
        .join(image_id(url))
}

/// The bytes of a release-note image, from the cache or from GitHub.
///
/// Fetched here rather than by the pane: the notes view runs in the webview
/// that holds the IPC bridge, and it should not be making requests to
/// anywhere. Bytes come back raw over IPC and the pane turns them into a
/// blob, so nothing in the page ever holds a remote URL.
#[tauri::command]
pub async fn release_image(url: String) -> Result<tauri::ipc::Response, String> {
    if !allowed_image_url(&url) {
        return Err(format!("not a fetchable image url: {url}"));
    }
    let bytes = tauri::async_runtime::spawn_blocking(move || image_bytes(&url))
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

fn image_bytes(url: &str) -> Result<Vec<u8>, String> {
    let path = image_path(url);
    if let Ok(bytes) = std::fs::read(&path) {
        // Re-checked on the way out as well as in: a cache directory is an
        // ordinary directory, and this is the last point before the bytes
        // reach the webview.
        if sniff_image(&bytes).is_some() {
            return Ok(bytes);
        }
        let _ = std::fs::remove_file(&path);
    }

    ensure_crypto_provider();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS * 3))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .map_err(|e| format!("could not fetch the image: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("image fetch failed ({})", response.status()));
    }
    // Checked before reading the body as well as after: a server that
    // declares a huge image should not get to stream it first.
    if let Some(len) = response.content_length() {
        if len > MAX_IMAGE_BYTES as u64 {
            return Err(format!("image is larger than {MAX_IMAGE_BYTES} bytes"));
        }
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?.to_vec();
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!("image is larger than {MAX_IMAGE_BYTES} bytes"));
    }
    if sniff_image(&bytes).is_none() {
        return Err("not an image the pane will display".into());
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes)
}

/// After an update, opens the pane in a tab of its own — once, in the
/// background, and only if there are notes to read.
///
/// Runs off the main thread because it makes a network request, and waits
/// for the window to exist: a tab added during `setup()` is added to a
/// workspace the frontend has not mounted yet.
pub fn announce_on_launch(app: AppHandle) {
    let current = app.package_info().version.to_string();
    let Some(version) = updated_version(&current) else {
        return;
    };
    std::thread::spawn(move || {
        let notes = match notes_for(&version) {
            Ok(notes) => notes,
            // Offline, rate-limited, or a release without notes. The user
            // asked for a terminal, not an error about release notes.
            Err(e) => {
                eprintln!("mirador: no release notes for v{version}: {e}");
                return;
            }
        };
        open_pane(&app, &notes.version, false);
    });
}

/// Adds a What's New pane in its own tab. `focus` is false for the launch
/// announcement — the tab appears, lit, but the pane you were typing in
/// keeps the cursor.
fn open_pane(app: &AppHandle, version: &str, focus: bool) -> String {
    let state = app.state::<AppState>();
    let previous = state.workspace.lock().unwrap().active_tab().id.clone();
    let (tab_id, pane_id) = state.workspace.lock().unwrap().new_tab();
    {
        let mut meta = state.meta.lock().unwrap();
        meta.entry(pane_id.clone()).or_default().whats_new = Some(version.to_string());
    }
    // Titles otherwise come from a pane's cwd or its shell's OSC, and this
    // pane has neither — the tab would read "shell".
    state
        .workspace
        .lock()
        .unwrap()
        .rename_tab(&tab_id, &format!("What's New in v{version}"));
    if !focus {
        state.workspace.lock().unwrap().set_active_tab(&previous);
    }
    crate::commands::emit_workspace(app);
    pane_id
}

/// Notes carried by the update manifest for a version still on offer.
///
/// An update's own manifest already holds its release body, so the notes for
/// a version you have not installed need no request at all — which matters
/// because the API that would serve them rate-limits by IP, and a shared
/// office address can be out of requests through no fault of yours.
fn offered_notes(app: &AppHandle, version: &str) -> Option<ReleaseNotes> {
    let state = app.state::<AppState>();
    let offer = state.update.available.lock().unwrap().clone()?;
    if offer.version != version {
        return None;
    }
    notes_from_manifest(version, offer.notes.as_deref()?)
}

/// Turns a manifest's `notes` into renderable notes, or `None` when it has
/// nothing worth rendering — in which case the caller falls through to the
/// API and gets the real thing.
fn notes_from_manifest(version: &str, body: &str) -> Option<ReleaseNotes> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    // Manifests published before the notes were real carried a bare link in
    // their place; a pane containing one sentence pointing elsewhere is
    // worse than the fetch this skips.
    if !body.contains('\n') && body.starts_with("See https://") {
        return None;
    }
    Some(ReleaseNotes {
        version: version.to_string(),
        // A manifest has no release title, and "v0.1.16" is the honest
        // heading for something not yet installed anyway.
        title: format!("v{version}"),
        blocks: cmux_core::notes::parse(body),
        url: format!("https://github.com/{REPO}/releases/tag/v{version}"),
    })
}

/// The pane's own content request: the manifest's notes when this version is
/// the one on offer, else the cache, else GitHub.
#[tauri::command]
pub async fn whats_new(app: AppHandle, version: String) -> Result<ReleaseNotes, String> {
    if let Some(notes) = offered_notes(&app, &version) {
        return Ok(notes);
    }
    tauri::async_runtime::spawn_blocking(move || notes_for(&version))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens the notes pane in the foreground, because reaching this means the
/// user asked: the palette's "What's New" (the running version) or the
/// update banner's (the version on offer).
#[tauri::command]
pub fn open_whats_new(app: AppHandle, version: Option<String>) -> String {
    let version = version.unwrap_or_else(|| app.package_info().version.to_string());
    open_pane(&app, &version, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `updated_version` writes to the real data directory, so the tests
    /// drive the comparison it performs rather than the file it touches.
    #[test]
    fn a_changed_version_is_news_and_a_first_install_is_not() {
        // Equivalent to `updated_version`'s decision table.
        let decide = |seen: Option<&str>, current: &str| -> Option<String> {
            match seen {
                Some(seen) if seen != current => Some(current.to_string()),
                _ => None,
            }
        };
        assert_eq!(decide(None, "0.1.16"), None, "a first install is not news");
        assert_eq!(decide(Some("0.1.16"), "0.1.16"), None, "a relaunch is not news");
        assert_eq!(decide(Some("0.1.15"), "0.1.16"), Some("0.1.16".into()));
        // A downgrade is still a change of what you are running.
        assert_eq!(decide(Some("0.1.16"), "0.1.15"), Some("0.1.15".into()));
    }

    #[test]
    fn manifest_notes_are_used_only_when_they_are_notes() {
        // The shape every release from now on carries.
        let notes = notes_from_manifest("0.1.16", "## Fixed\n\nA thing.\n")
            .expect("a real body is renderable");
        assert_eq!(notes.title, "v0.1.16");
        assert_eq!(notes.url, "https://github.com/SuperHomer/mirador/releases/tag/v0.1.16");
        assert_eq!(notes.blocks.len(), 2);

        // The shape older manifests carry: fall through, fetch the real one.
        assert!(notes_from_manifest(
            "0.1.15",
            "See https://github.com/SuperHomer/mirador/releases/tag/v0.1.15"
        )
        .is_none());
        assert!(notes_from_manifest("0.1.15", "").is_none());
        assert!(notes_from_manifest("0.1.15", "   \n  ").is_none());

        // A real body that merely opens with that sentence is still a body:
        // the placeholder is one line and nothing else.
        assert!(notes_from_manifest(
            "0.1.16",
            "See https://github.com/SuperHomer/mirador/releases/tag/v0.1.16\n\n## Also fixed\n\nMore."
        )
        .is_some());
    }

    #[test]
    fn only_github_https_urls_are_fetchable() {
        assert!(allowed_image_url(
            "https://github.com/user-attachments/assets/8f3c1d2e-aaaa"
        ));
        assert!(allowed_image_url("https://raw.githubusercontent.com/o/r/main/a.png"));
        assert!(allowed_image_url("https://USER-IMAGES.GithubUserContent.com/x.gif"));

        // Plain http is not fetched even from an allowed host.
        assert!(!allowed_image_url("http://github.com/a.png"));
        // Nor any other host, however it is dressed up.
        assert!(!allowed_image_url("https://example.com/a.png"));
        // A suffix match must not accept a lookalike domain.
        assert!(!allowed_image_url("https://evil-github.com/a.png"));
        assert!(!allowed_image_url("https://github.com.evil.test/a.png"));
        // Nor userinfo smuggling the real host past a naive prefix check.
        assert!(!allowed_image_url("https://github.com@evil.test/a.png"));
        assert!(!allowed_image_url(""));
        assert!(!allowed_image_url("javascript:alert(1)"));
    }

    #[test]
    fn only_raster_image_bytes_are_accepted() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff_image(b"GIF89a....."), Some("gif"));
        assert_eq!(sniff_image(b"GIF87a....."), Some("gif"));
        assert_eq!(sniff_image(&[0xff, 0xd8, 0xff, 0xe0, 0x00]), Some("jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_image(b"\0\0\0\x20ftypavif\0\0"), Some("avif"));

        // An SVG can carry script, so it is not something this will store
        // or hand to the webview, whatever the server called it.
        assert_eq!(sniff_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"), None);
        assert_eq!(sniff_image(b"<!doctype html><script>"), None);
        assert_eq!(sniff_image(b""), None);
        assert_eq!(sniff_image(b"RIFF"), None, "a truncated header is not a webp");
    }

    #[test]
    fn an_image_id_is_stable_and_per_url() {
        let a = image_id("https://github.com/user-attachments/assets/one");
        assert_eq!(a, image_id("https://github.com/user-attachments/assets/one"));
        assert_ne!(a, image_id("https://github.com/user-attachments/assets/two"));
        // It names a file, so it must not carry anything path-like.
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn cache_paths_are_per_version() {
        assert_ne!(cache_path("0.1.15"), cache_path("0.1.16"));
        assert!(cache_path("0.1.16")
            .to_string_lossy()
            .ends_with("release-notes-0.1.16.json"));
    }
}
