//! Which files the markdown panes follow, and which directories that means
//! watching. The app's watcher does the watching; this is the bookkeeping,
//! kept here so it can be tested.
//!
//! A directory is watched rather than the file: an editor's atomic save
//! renames a new file over the old one, and a watch on the file itself is
//! lost with the first rename. Watching the directory also sees a file
//! deleted and written again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Follows {
    /// Each following pane: the path it shows, as it spells it, and the
    /// resolved one events are matched against.
    panes: HashMap<String, (String, PathBuf)>,
    /// How many panes follow a file in each (resolved) directory.
    dirs: HashMap<PathBuf, usize>,
}

impl Follows {
    /// `pane` follows `path`, and no longer what it followed before.
    /// Returns the directory to start watching, when no pane watched it,
    /// and the one to stop watching, when its last pane moved away.
    /// Following again what it already follows changes nothing, so a
    /// component mounted twice holds one watch.
    pub fn follow(&mut self, pane: &str, path: &str) -> (Option<PathBuf>, Option<PathBuf>) {
        if self.panes.get(pane).is_some_and(|(p, _)| p == path) {
            return (None, None);
        }
        let stop = self.unfollow(pane);
        let resolved = resolve(Path::new(path));
        let dir = resolved.parent().unwrap_or(Path::new("")).to_path_buf();
        self.panes.insert(pane.to_string(), (path.to_string(), resolved));
        let count = self.dirs.entry(dir.clone()).or_insert(0);
        *count += 1;
        // Moved to another file in the same directory: it is watched still.
        if stop.as_ref() == Some(&dir) {
            return (None, None);
        }
        ((*count == 1).then_some(dir), stop)
    }

    /// `pane` follows nothing. Returns the directory to stop watching, when
    /// it was the last pane following a file there.
    pub fn unfollow(&mut self, pane: &str) -> Option<PathBuf> {
        let (_, resolved) = self.panes.remove(pane)?;
        let dir = resolved.parent().unwrap_or(Path::new("")).to_path_buf();
        let count = self.dirs.get_mut(&dir)?;
        *count -= 1;
        if *count == 0 {
            self.dirs.remove(&dir);
            Some(dir)
        } else {
            None
        }
    }

    /// The paths, as the panes spell them, that a change to `changed` is a
    /// change to — one per spelling, however many panes show it.
    pub fn changed(&self, changed: &Path) -> Vec<String> {
        let changed = resolve(changed);
        let mut paths: Vec<String> = self
            .panes
            .values()
            .filter(|(_, resolved)| *resolved == changed)
            .map(|(path, _)| path.clone())
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }
}

/// The path with its directory resolved, so that `/tmp/a.md` and what a
/// watcher reports for it (`/private/tmp/a.md` on macOS) agree. The file
/// itself is not resolved: it may be gone, between an editor's delete and
/// its write.
fn resolve(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()).join(name),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-follow-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn a_directory_is_watched_while_any_pane_follows_a_file_there() {
        let d = dir("count");
        let mut f = Follows::default();
        let watched = f.follow("a", &file(&d, "README.md")).0.expect("the first follow watches");
        assert_eq!(f.follow("b", &file(&d, "PLAN.md")), (None, None));
        assert_eq!(f.unfollow("a"), None);
        assert_eq!(f.unfollow("b"), Some(watched));
        // A pane that follows nothing unfollows nothing.
        assert_eq!(f.unfollow("b"), None);
    }

    #[test]
    fn mounted_unmounted_and_mounted_again_holds_one_watch() {
        // React's StrictMode in development, and a pane moved on the board.
        let d = dir("strict");
        let path = file(&d, "README.md");
        let mut f = Follows::default();
        let watched = f.follow("a", &path).0.unwrap();
        assert_eq!(f.unfollow("a"), Some(watched.clone()));
        assert_eq!(f.follow("a", &path), (Some(watched.clone()), None));
        // A second mount with no unmount between holds the same one.
        assert_eq!(f.follow("a", &path), (None, None));
        assert_eq!(f.unfollow("a"), Some(watched));
    }

    #[test]
    fn a_pane_moved_to_another_file_moves_its_watch() {
        let (one, two) = (dir("one"), dir("two"));
        let mut f = Follows::default();
        let first = f.follow("a", &file(&one, "a.md")).0.unwrap();
        // Same directory: nothing to start, nothing to stop.
        assert_eq!(f.follow("a", &file(&one, "b.md")), (None, None));
        assert_eq!(f.changed(&one.join("a.md")), Vec::<String>::new());
        // Another directory: that one starts, the first one stops.
        let (start, stop) = f.follow("a", &file(&two, "c.md"));
        assert_eq!(stop, Some(first));
        assert_eq!(f.unfollow("a"), start);
    }

    #[test]
    fn a_change_names_the_path_as_each_pane_spells_it() {
        let d = dir("changed");
        let path = file(&d, "README.md");
        let mut f = Follows::default();
        f.follow("a", &path);
        f.follow("b", &path);
        // Reported with its directory resolved, as a watcher reports it.
        let reported = std::fs::canonicalize(&d).unwrap().join("README.md");
        assert_eq!(f.changed(&reported), vec![path.clone()]);
        assert!(f.changed(&d.join("OTHER.md")).is_empty());
        // Deleted, and so not resolvable itself: still the same file.
        assert_eq!(f.changed(&d.join("README.md")), vec![path]);
    }
}
