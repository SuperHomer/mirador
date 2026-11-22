//! An image pasted into a pane becomes a file whose path is typed, as a
//! dropped file's is, so an agent like Claude Code attaches it. This is
//! where those files go, what they are called, and when they go away.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::image::sniff_image;

/// The largest image a paste takes. A paste into an SSH pane sends the
/// file to the remote as base64 through a shell, and a screenshot is a few
/// MB at most; anything past this is not one.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// Where pastes go on a remote, relative to its home: out of the way, and
/// a cache, which is what they are.
pub const REMOTE_DIR: &str = ".cache/mirador/paste";

/// Where pastes go on this computer.
pub fn local_dir() -> PathBuf {
    crate::session::data_dir().join("paste")
}

/// Where a paste called `file_name` goes on a remote, relative to its home.
pub fn remote_rel_path(file_name: &str) -> String {
    format!("{REMOTE_DIR}/{file_name}")
}

/// Writes a pasted image into `dir` and answers its path. Refuses bytes
/// that are not a raster image, and an image over [`MAX_BYTES`]. The file
/// is the owner's alone: a screenshot can show anything.
pub fn save(dir: &Path, bytes: &[u8], now: SystemTime) -> Result<PathBuf, String> {
    let ext = sniff_image(bytes).ok_or("only a PNG, JPEG, GIF, WebP or AVIF image can be pasted")?;
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "the image is {} MB: an image pasted into a pane can be up to {} MB",
            bytes.len().div_ceil(1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    create_private_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stamp = stamp(now);
    // Two pastes in the same second get different names, and `create_new`
    // makes sure no paste ever overwrites another, however unlikely.
    loop {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = dir.join(format!("paste-{stamp}-{}.{ext}", &id[..8]));
        match create_private_file(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes) {
                    // A cut-off image is not one to type a path to, nor
                    // to leave lying there for a day.
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("{}: {e}", path.display()));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
}

/// Deletes the pastes in `dir` older than `max_age`, and answers how many
/// went. Anything else there is left alone, and no `dir` is no pastes.
pub fn prune(dir: &Path, max_age: Duration, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let is_paste = entry.file_name().to_str().is_some_and(|n| n.starts_with("paste-"));
        let Ok(meta) = entry.metadata() else { continue };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if is_paste && meta.is_file() && old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// `now` as `20261122-221530`, in UTC: the names sort by when they were
/// pasted, and need no time zone to be unique.
fn stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest of it";

    /// A fresh directory under the temp dir, not yet created.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-paste-{name}-{}", uuid::Uuid::new_v4().simple()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 2026-11-22 22:15:30 UTC.
    fn evening() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_795_385_730)
    }

    fn file_name(path: &Path) -> String {
        path.file_name().unwrap().to_str().unwrap().to_string()
    }

    #[test]
    fn a_paste_is_saved_under_its_time_and_its_kind() {
        let dir = scratch("save");
        let path = save(&dir, PNG, evening()).unwrap();
        assert_eq!(path.parent(), Some(dir.as_path()), "the directory is made");
        let name = file_name(&path);
        assert!(name.starts_with("paste-20261122-221530-"), "{name}");
        assert!(name.ends_with(".png"), "{name}");
        assert_eq!(std::fs::read(&path).unwrap(), PNG);

        let jpeg = save(&dir, &[0xff, 0xd8, 0xff, 0xe0, 0x00], evening()).unwrap();
        assert!(file_name(&jpeg).ends_with(".jpeg"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn two_pastes_in_the_same_second_keep_both() {
        let dir = scratch("same");
        let a = save(&dir, PNG, evening()).unwrap();
        let b = save(&dir, b"GIF89a.....", evening()).unwrap();
        let c = save(&dir, PNG, evening()).unwrap();
        assert!(a != b && b != c && a != c);
        assert_eq!(std::fs::read(&a).unwrap(), PNG);
        assert_eq!(std::fs::read(&b).unwrap(), b"GIF89a.....");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_paste_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let path = save(&dir, PNG, evening()).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir), 0o700);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_raster_image_is_saved() {
        let dir = scratch("refuse");
        for bytes in [&b"<svg xmlns=\"http://www.w3.org/2000/svg\">"[..], b"just text", b""] {
            let err = save(&dir, bytes, evening()).unwrap_err();
            assert!(err.contains("PNG, JPEG"), "{err}");
        }
        assert!(!dir.exists(), "nothing was written, not even the directory");
    }

    #[test]
    fn an_image_over_the_cap_is_refused() {
        let dir = scratch("cap");
        let mut big = PNG.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        let err = save(&dir, &big, evening()).unwrap_err();
        assert!(err.contains("21 MB") && err.contains("up to 20 MB"), "{err}");
        assert!(!dir.exists());

        big.truncate(MAX_BYTES);
        assert!(save(&dir, &big, evening()).is_ok(), "the cap itself is allowed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pruning_removes_old_pastes_only() {
        let dir = scratch("prune");
        let now = SystemTime::now();
        let old = save(&dir, PNG, now).unwrap();
        let fresh = save(&dir, PNG, now).unwrap();
        let other = dir.join("notes.txt");
        std::fs::write(&other, "mine").unwrap();
        let two_days = Duration::from_secs(2 * 86_400);
        for path in [&old, &other] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(now - two_days)
                .unwrap();
        }

        assert_eq!(prune(&dir, Duration::from_secs(86_400), now), 1);
        assert!(!old.exists());
        assert!(fresh.exists(), "a paste from today stays");
        assert!(other.exists(), "a file that is not a paste stays, however old");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pruning_a_missing_directory_is_nothing_to_do() {
        assert_eq!(prune(&scratch("missing"), Duration::from_secs(86_400), SystemTime::now()), 0);
    }

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(UNIX_EPOCH), "19700101-000000");
        assert_eq!(stamp(evening()), "20261122-221530");
        // A leap day, and the last second of a year.
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_709_164_800)), "20240229-000000");
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_767_225_599)), "20251231-235959");
    }

    #[test]
    fn remote_pastes_go_under_the_cache() {
        assert_eq!(remote_rel_path("paste-x.png"), ".cache/mirador/paste/paste-x.png");
        assert!(local_dir().ends_with("paste"));
    }
}
