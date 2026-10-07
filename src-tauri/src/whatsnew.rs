//! The "What's New" pane: the release notes for the running version, shown
//! once after an update — or, when the update skipped releases, the notes
//! for every one of them, so a user who ignored five updates reads about
//! all five rather than only the last.
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
/// either way so the question is only ever asked once per upgrade. Returns
/// the version launched before this one: the start of what is news.
///
/// A first install has nothing to compare against and shows nothing: the
/// notes for a version you just chose to download are not news. A
/// *downgrade* counts as a change too — whatever the user is now running
/// is what they may want to read about.
fn updated_from(current: &str) -> Option<String> {
    let seen = read_seen();
    write_seen(current);
    seen.filter(|seen| seen != current)
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
    let response = api_get(&format!(
        "https://api.github.com/repos/{REPO}/releases/tags/v{version}"
    ))?;
    if !response.status().is_success() {
        // A release that exists but has no notes, an unpublished tag, or a
        // rate limit all land here, and all mean the same thing to the
        // caller: there is nothing to show.
        return Err(format!("no release notes for v{version} ({})", response.status()));
    }
    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    release_from_json(&json, version).ok_or_else(|| format!("release v{version} has no notes"))
}

fn api_get(url: &str) -> Result<reqwest::blocking::Response, String> {
    ensure_crypto_provider();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|e| format!("could not reach GitHub: {e}"))
}

/// One release from the GitHub API's JSON, or `None` when it has no notes.
fn release_from_json(json: &serde_json::Value, version: &str) -> Option<ReleaseNotes> {
    let body = json["body"].as_str().unwrap_or("").trim().to_string();
    if body.is_empty() {
        return None;
    }
    Some(ReleaseNotes {
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

/// A plain `X.Y.Z`, as an orderable key. Anything else — a pre-release
/// suffix, a stray tag — is `None` and never part of a range: what a range
/// answers is "which releases did this update bring", and Mirador's
/// releases are all plain versions.
fn version_key(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let key = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(key)
}

/// The releases an update from `since` to `version` brought, newest first:
/// everything after `since`, up to and including `version`. Empty for a
/// downgrade or a relaunch, which bring nothing new.
fn releases_between(all: &[ReleaseNotes], since: &str, version: &str) -> Vec<ReleaseNotes> {
    let (Some(low), Some(high)) = (version_key(since), version_key(version)) else {
        return Vec::new();
    };
    let mut range: Vec<ReleaseNotes> = all
        .iter()
        .filter(|n| version_key(&n.version).is_some_and(|k| k > low && k <= high))
        .cloned()
        .collect();
    range.sort_by_key(|n| std::cmp::Reverse(version_key(&n.version)));
    range
}

/// Every published release, newest first, in one file. Separate from the
/// per-version cache: a range is answered from one list request rather
/// than one request per release, which a rate limit of 60 an hour per IP
/// would not survive for a user five releases behind.
fn release_list_path() -> PathBuf {
    cmux_core::session::data_dir().join("release-notes-all.json")
}

/// Every release with notes, from one API request. Drafts and pre-releases
/// are not something an update installs, so they are not news either. One
/// page of a hundred covers every release Mirador has made several times
/// over; a jump across more than that shows the newest hundred.
fn fetch_releases() -> Result<Vec<ReleaseNotes>, String> {
    let response = api_get(&format!(
        "https://api.github.com/repos/{REPO}/releases?per_page=100"
    ))?;
    if !response.status().is_success() {
        return Err(format!("could not list releases ({})", response.status()));
    }
    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let releases = json.as_array().ok_or("unexpected release list")?;
    Ok(releases
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            // Re-rendered from the parsed key, so the version — which names
            // nothing on disk here, but does in the per-version cache — is
            // digits and dots whatever the tag said.
            let (major, minor, patch) = version_key(tag)?;
            release_from_json(r, &format!("{major}.{minor}.{patch}"))
        })
        .collect())
}

/// Notes for every release from `since` (exclusive) to `version`, newest
/// first.
///
/// The cached list answers when it already holds `version`: releases older
/// than one already listed do not appear later, so it is complete for any
/// range ending there. Otherwise one request refreshes it. A range that
/// does not reach `version` is an error, not a partial answer — the caller
/// falls back to that one release, which is the part that must not be lost.
pub fn notes_since(since: &str, version: &str) -> Result<Vec<ReleaseNotes>, String> {
    let cached = std::fs::read_to_string(release_list_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<ReleaseNotes>>(&t).ok());
    let all = match cached {
        Some(all) if all.iter().any(|n| n.version == version) => all,
        _ => {
            let all = fetch_releases()?;
            if let Some(parent) = release_list_path().parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&all) {
                let _ = std::fs::write(release_list_path(), json);
            }
            all
        }
    };
    let range = releases_between(&all, since, version);
    match range.first() {
        Some(newest) if newest.version == version => Ok(range),
        _ => Err(format!("no releases listed between v{since} and v{version}")),
    }
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
    let version = app.package_info().version.to_string();
    let Some(previous) = updated_from(&version) else {
        return;
    };
    std::thread::spawn(move || {
        // Every release since the one last launched, when the update
        // skipped some. One release in range is the ordinary case and gets
        // the ordinary pane, titled for its version.
        if let Ok(range) = notes_since(&previous, &version) {
            if range.len() > 1 {
                open_pane(&app, &version, Some(&previous), false);
                return;
            }
        }
        if let Err(e) = notes_for(&version) {
            // Offline, rate-limited, or a release without notes. The user
            // asked for a terminal, not an error about release notes.
            eprintln!("mirador: no release notes for v{version}: {e}");
            return;
        }
        open_pane(&app, &version, None, false);
    });
}

/// Adds a What's New pane in its own tab. `focus` is false for the launch
/// announcement — the tab appears, lit, but the pane you were typing in
/// keeps the cursor.
fn open_pane(app: &AppHandle, version: &str, since: Option<&str>, focus: bool) -> String {
    let state = app.state::<AppState>();
    let previous = state.workspace.lock().unwrap().active_tab().id.clone();
    let (tab_id, pane_id) = state.workspace.lock().unwrap().new_tab();
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(pane_id.clone()).or_default();
        entry.whats_new = Some(version.to_string());
        entry.whats_new_since = since.map(str::to_string);
    }
    // Titles otherwise come from a pane's cwd or its shell's OSC, and this
    // pane has neither — the tab would read "shell".
    let title = match since {
        Some(since) => format!("What's New since v{since}"),
        None => format!("What's New in v{version}"),
    };
    state.workspace.lock().unwrap().rename_tab(&tab_id, &title);
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

/// The pane's content request, newest release first.
///
/// With `since`, every release after it up to `version`. Without it — or
/// when that range cannot be had, offline or rate-limited — the one
/// release: the manifest's notes when this version is the one on offer,
/// else the cache, else GitHub. Falling back rather than failing keeps the
/// newest notes on screen, which is all the pane showed before ranges.
#[tauri::command]
pub async fn whats_new(
    app: AppHandle,
    version: String,
    since: Option<String>,
) -> Result<Vec<ReleaseNotes>, String> {
    let offered = offered_notes(&app, &version);
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(range) = since.and_then(|since| notes_since(&since, &version).ok()) {
            return Ok(range);
        }
        match offered {
            Some(notes) => Ok(vec![notes]),
            None => notes_for(&version).map(|notes| vec![notes]),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens the notes pane in the foreground, because reaching this means the
/// user asked: the palette's "What's New" (the running version) or the
/// update banner's (everything the update on offer would bring, `since` the
/// running version).
#[tauri::command]
pub fn open_whats_new(app: AppHandle, version: Option<String>, since: Option<String>) -> String {
    let version = version.unwrap_or_else(|| app.package_info().version.to_string());
    open_pane(&app, &version, since.as_deref(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `updated_from` writes to the real data directory, so the tests
    /// drive the comparison it performs rather than the file it touches.
    #[test]
    fn a_changed_version_is_news_and_a_first_install_is_not() {
        // Equivalent to `updated_from`'s decision table.
        let decide = |seen: Option<&str>, current: &str| -> Option<String> {
            seen.map(str::to_string).filter(|seen| seen != current)
        };
        assert_eq!(decide(None, "0.1.16"), None, "a first install is not news");
        assert_eq!(decide(Some("0.1.16"), "0.1.16"), None, "a relaunch is not news");
        assert_eq!(decide(Some("0.1.15"), "0.1.16"), Some("0.1.15".into()));
        // A downgrade is still a change of what you are running.
        assert_eq!(decide(Some("0.1.16"), "0.1.15"), Some("0.1.16".into()));
    }

    fn release(version: &str) -> ReleaseNotes {
        ReleaseNotes {
            version: version.into(),
            title: format!("v{version}"),
            blocks: Vec::new(),
            url: String::new(),
        }
    }

    fn versions(notes: &[ReleaseNotes]) -> Vec<&str> {
        notes.iter().map(|n| n.version.as_str()).collect()
    }

    #[test]
    fn versions_order_numerically_and_only_plain_ones_parse() {
        assert!(version_key("0.1.10") > version_key("0.1.9"), "not by string");
        assert!(version_key("1.0.0") > version_key("0.99.99"));
        assert_eq!(version_key("v0.1.21"), Some((0, 1, 21)));
        assert_eq!(version_key("0.1.21"), version_key("v0.1.21"));

        assert_eq!(version_key("0.2.0-beta.1"), None);
        assert_eq!(version_key("0.1"), None);
        assert_eq!(version_key("0.1.2.3"), None);
        assert_eq!(version_key("nightly"), None);
        assert_eq!(version_key(""), None);
    }

    #[test]
    fn a_skipped_range_is_every_release_after_since_up_to_version() {
        // Out of order on purpose: the API's order is not what this trusts.
        let all: Vec<_> = ["0.1.9", "0.1.12", "0.1.10", "0.1.11", "0.1.13", "0.1.8"]
            .into_iter()
            .map(release)
            .collect();

        assert_eq!(
            versions(&releases_between(&all, "0.1.9", "0.1.12")),
            ["0.1.12", "0.1.11", "0.1.10"],
            "newest first; since excluded, version included"
        );
        // The ordinary update is a range of one.
        assert_eq!(versions(&releases_between(&all, "0.1.12", "0.1.13")), ["0.1.13"]);
        // A since that is not a plain version gives nothing rather than
        // everything: the caller falls back to the one release.
        assert!(releases_between(&all, "0.1.11-dev", "0.1.13").is_empty());
        // A relaunch and a downgrade bring nothing new.
        assert!(releases_between(&all, "0.1.12", "0.1.12").is_empty());
        assert!(releases_between(&all, "0.1.13", "0.1.9").is_empty());
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
