//! The files a file tree pane shows: a repository's, with git's status on
//! each, or a plain directory's outside one.
//!
//! The tree is built here rather than in the component that draws it, so
//! that its order, its markers and its cap are tested.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use cmux_protocol::{FileTree, FileTreeNode};

/// Files and directories together. A repository past it is one whose tree
/// nobody browses whole; a plain directory past it is likely `~`, whose
/// walk would otherwise not end.
pub const MAX_ENTRIES: usize = 5000;

/// The directory a tree for a pane in `dir` shows: its repository's root,
/// or `dir` itself outside one.
pub fn tree_root(dir: &Path) -> PathBuf {
    crate::git::find_repo_root(&dir.to_string_lossy()).unwrap_or_else(|| dir.to_path_buf())
}

/// The tree under `root`. In a repository, what git tracks or would track
/// (never what `.gitignore` leaves out, which keeps `node_modules` and
/// `target` from burying the rest), a tracked file deleted from disk
/// included. Outside one, every entry not hidden by a leading dot.
pub fn list(root: &Path) -> Result<FileTree, String> {
    let repo = root.join(".git").exists();
    let mut tree = Builder::default();
    if repo {
        let statuses = statuses(root)?;
        let mut paths: BTreeSet<String> = git_z(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect();
        // A deletion already staged has left the index, and so ls-files.
        paths.extend(statuses.keys().cloned());
        // Shallowest first, so that a tree cut at the cap still has every
        // top-level entry, as the walk below does.
        let mut paths: Vec<String> = paths.into_iter().collect();
        paths.sort_by_key(|p| (p.trim_end_matches('/').matches('/').count(), p.clone()));
        for path in paths {
            let status = statuses.get(&path).cloned();
            // A repository inside this one, untracked, is named with a
            // trailing slash and never listed into.
            let (path, is_dir) = match path.strip_suffix('/') {
                Some(dir) => (dir, true),
                None => (path.as_str(), false),
            };
            if !tree.insert(path, is_dir, status) {
                break;
            }
        }
    } else {
        walk(root, &mut tree)?;
    }
    Ok(FileTree {
        root: root.to_string_lossy().into_owned(),
        repo,
        truncated: tree.truncated,
        nodes: tree.root.into_nodes(""),
    })
}

/// The file at `rel` under `root`, for an action on a row of the tree.
/// `rel` comes from the page, so it is checked rather than trusted: it
/// must be relative, with no `..` (nor anything else but names), and must
/// still be under `root` once links are followed — a tracked symlink to
/// `/etc` is a row, but not a way out.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    check_relative(rel)?;
    let joined = root.join(rel);
    let real = joined.canonicalize().map_err(|e| format!("{rel}: {e}"))?;
    let real_root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    if !real.starts_with(&real_root) {
        return Err(format!("{rel} leads outside the tree"));
    }
    Ok(joined)
}

/// Whether `rel` is a path down from a root and nothing else: relative,
/// made only of names — no `..`, no `.`, no drive. Enough on its own for
/// a path that only reaches git as a literal pathspec, which git never
/// follows through a link, and which may name a file deleted from disk.
pub fn check_relative(rel: &str) -> Result<(), String> {
    let normal = Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if rel.is_empty() || !normal {
        return Err(format!("not a path inside the tree: {rel}"));
    }
    Ok(())
}

/// `path` as one down from `root`, `/`-separated, for a diff of that one
/// file: an absolute path (the CLI's, resolved where it ran) must be under
/// `root`, a relative one is taken as relative to it already. The file may
/// be deleted, so an absolute path is compared as written first, then with
/// links resolved on both sides — through its directory when the file
/// itself is gone (`/var` is `/private/var` on macOS).
pub fn relative_to(root: &Path, path: &str) -> Result<String, String> {
    let given = Path::new(path);
    let under = if given.is_absolute() {
        let real = |p: &Path| {
            p.canonicalize().ok().or_else(|| {
                let dir = p.parent()?.canonicalize().ok()?;
                Some(dir.join(p.file_name()?))
            })
        };
        let under = given.strip_prefix(root).map(Path::to_path_buf).ok().or_else(|| {
            let (path, root) = (real(given)?, root.canonicalize().ok()?);
            path.strip_prefix(&root).map(Path::to_path_buf).ok()
        });
        under.ok_or_else(|| format!("{path} is not in {}", root.display()))?
    } else {
        check_relative(path)?;
        given.to_path_buf()
    };
    // `/` whichever way it came: `src\a.txt` from a Windows client too.
    let rel = under.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    check_relative(&rel)?;
    Ok(rel)
}

fn git_z(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::proc::command("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Each changed path's status. `-z` keeps paths with spaces, quotes and
/// non-ASCII unquoted; `-uall` names every untracked file rather than its
/// directory, as ls-files does.
fn statuses(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = git_z(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    Ok(parse_status(&text))
}

/// `XY path\0`, and for a rename or copy `XY new\0old\0`: the old path is
/// gone from disk, so it is skipped. The R or C is in X for a rename in
/// the index, in Y for one in the worktree (`git add -N` of a moved file).
fn parse_status(text: &str) -> BTreeMap<String, String> {
    let mut statuses = BTreeMap::new();
    let mut entries = text.split('\0');
    while let Some(entry) = entries.next() {
        let (Some(xy), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        let (x, y) = (xy.as_bytes()[0], xy.as_bytes()[1]);
        if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            entries.next();
        }
        let status = match (x, y) {
            (b'?', b'?') => "untracked",
            (b'R', _) | (_, b'R') => "renamed",
            (b'C', _) | (_, b'C') => "copied",
            (b'D', _) | (_, b'D') => "deleted",
            (b'A', _) => "added",
            _ => "modified",
        };
        statuses.insert(path.to_string(), status.to_string());
    }
    statuses
}

/// Breadth first, so that a tree cut at the cap still has every top-level
/// entry. Symlinks are listed but never followed: a link to an ancestor
/// would loop.
fn walk(root: &Path, tree: &mut Builder) -> Result<(), String> {
    let mut queue = VecDeque::from([(root.to_path_buf(), String::new())]);
    let mut first = true;
    while let Some((dir, rel)) = queue.pop_front() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if first => return Err(format!("{}: {e}", dir.display())),
            // A subdirectory we may not read is shown, empty.
            Err(_) => continue,
        };
        first = false;
        let mut entries: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| Some((e.file_name().into_string().ok()?, e)))
            .filter(|(name, _)| !name.starts_with('.'))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in entries {
            let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if !tree.insert(&path, is_dir, None) {
                return Ok(());
            }
            if is_dir {
                queue.push_back((entry.path(), path));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct Builder {
    root: Dir,
    count: usize,
    truncated: bool,
}

#[derive(Default)]
struct Dir {
    /// Its own, which only an untracked nested repository has.
    status: Option<String>,
    dirs: BTreeMap<String, Dir>,
    files: BTreeMap<String, Option<String>>,
}

impl Builder {
    /// Adds `path` and the directories above it; false, and nothing added,
    /// once that would pass the cap.
    fn insert(&mut self, path: &str, is_dir: bool, status: Option<String>) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        let (dirs, leaf) = if is_dir { (&parts[..], None) } else { (&parts[..parts.len() - 1], parts.last()) };

        let mut new = 0;
        let mut at = Some(&self.root);
        for name in dirs {
            at = at.and_then(|d| d.dirs.get(*name));
            if at.is_none() {
                new += 1;
            }
        }
        if let Some(leaf) = leaf {
            if !at.is_some_and(|d| d.files.contains_key(*leaf)) {
                new += 1;
            }
        }
        if self.count + new > MAX_ENTRIES {
            self.truncated = true;
            return false;
        }
        self.count += new;

        let mut at = &mut self.root;
        for name in dirs {
            at = at.dirs.entry(name.to_string()).or_default();
        }
        match leaf {
            Some(leaf) => {
                at.files.insert(leaf.to_string(), status);
            }
            None => at.status = status.or(at.status.take()),
        }
        true
    }
}

impl Dir {
    /// Directories first, then files, each alphabetical with case ignored
    /// (as editors list them).
    fn into_nodes(self, prefix: &str) -> Vec<FileTreeNode> {
        let path = |name: &str| if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
        let mut dirs: Vec<FileTreeNode> = self
            .dirs
            .into_iter()
            .map(|(name, mut dir)| {
                let own = dir.status.take();
                let children = dir.into_nodes(&path(&name));
                let changed = children.iter().any(|c| c.status.is_some());
                FileTreeNode {
                    path: path(&name),
                    name,
                    dir: true,
                    status: own.or_else(|| changed.then(|| "modified".to_string())),
                    children,
                }
            })
            .collect();
        let mut files: Vec<FileTreeNode> = self
            .files
            .into_iter()
            .map(|(name, status)| FileTreeNode { path: path(&name), name, dir: false, status, children: Vec::new() })
            .collect();
        let by_name = |a: &FileTreeNode, b: &FileTreeNode| {
            (a.name.to_lowercase(), &a.name).cmp(&(b.name.to_lowercase(), &b.name))
        };
        dirs.sort_by(by_name);
        files.sort_by(by_name);
        dirs.extend(files);
        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let output = crate::proc::command("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be installed to run this test");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn new_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    fn count(nodes: &[FileTreeNode]) -> usize {
        nodes.iter().map(|n| 1 + count(&n.children)).sum()
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-tree-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    fn find<'a>(nodes: &'a [FileTreeNode], path: &str) -> Option<&'a FileTreeNode> {
        nodes.iter().find_map(|n| if n.path == path { Some(n) } else { find(&n.children, path) })
    }

    fn names(nodes: &[FileTreeNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn a_repository_shows_what_git_sees_with_its_status() {
        let dir = fresh_dir("repo");
        new_repo(&dir);
        for f in ["clean.txt", "edited.txt", "gone.txt", "src/deep/lib.rs", "src/ok.rs", "docs/a.md"] {
            write(&dir, f);
        }
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "init"]);

        std::fs::write(dir.join("edited.txt"), "changed").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();
        std::fs::write(dir.join("src/deep/lib.rs"), "changed").unwrap();
        write(&dir, "new file é.txt");
        write(&dir, "fresh/inside.txt");
        write(&dir, "target/debug/out");
        // Contents of its own, or git would call it docs/a.md renamed.
        std::fs::write(dir.join("staged.txt"), "staged").unwrap();
        git(&dir, &["add", "staged.txt"]);
        git(&dir, &["rm", "-q", "docs/a.md"]);
        // A repository of its own inside this one, which git never lists into.
        write(&dir, "nested/inner.txt");
        new_repo(&dir.join("nested"));
        git(&dir.join("nested"), &["add", "."]);
        git(&dir.join("nested"), &["commit", "-qm", "inner"]);

        // Asked from a subdirectory, the tree is the repository's.
        let root = tree_root(&dir.join("src/deep"));
        assert_eq!(root, dir);
        let tree = list(&root).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(tree.repo);
        assert!(!tree.truncated);
        assert_eq!(tree.root, dir.to_string_lossy());
        // Directories first, then files; .gitignore is tracked, target/ is not.
        assert_eq!(
            names(&tree.nodes),
            ["docs", "fresh", "nested", "src", ".gitignore", "clean.txt", "edited.txt", "gone.txt", "new file é.txt", "staged.txt"]
        );
        let status = |p: &str| find(&tree.nodes, p).unwrap_or_else(|| panic!("{p} missing")).status.as_deref();
        assert_eq!(status("clean.txt"), None);
        assert_eq!(status("edited.txt"), Some("modified"));
        assert_eq!(status("gone.txt"), Some("deleted"));
        assert_eq!(status("docs/a.md"), Some("deleted"));
        assert_eq!(status("new file é.txt"), Some("untracked"));
        assert_eq!(status("fresh/inside.txt"), Some("untracked"));
        assert_eq!(status("staged.txt"), Some("added"));
        // A change deep down marks every directory above it, and only those.
        assert_eq!(status("src/deep/lib.rs"), Some("modified"));
        assert_eq!(status("src/deep"), Some("modified"));
        assert_eq!(status("src"), Some("modified"));
        assert_eq!(status("src/ok.rs"), None);
        assert_eq!(names(&find(&tree.nodes, "src").unwrap().children), ["deep", "ok.rs"]);
        assert!(find(&tree.nodes, "target").is_none());
        let nested = find(&tree.nodes, "nested").unwrap();
        assert!(nested.dir && nested.children.is_empty());
        assert_eq!(nested.status.as_deref(), Some("untracked"));
    }

    #[test]
    fn a_repository_past_the_cap_keeps_its_top_level() {
        let dir = fresh_dir("repo-cap");
        new_repo(&dir);
        // "a/…" sorts before "z.txt": cut alphabetically, z.txt would go.
        for i in 0..MAX_ENTRIES {
            write(&dir, &format!("a/{i}"));
        }
        write(&dir, "z.txt");
        let tree = list(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(tree.truncated);
        assert_eq!(count(&tree.nodes), MAX_ENTRIES);
        assert_eq!(names(&tree.nodes), ["a", "z.txt"]);
    }

    #[test]
    fn a_rename_shows_the_new_path_only() {
        let statuses = parse_status(
            "R  new name.txt\0old name.txt\0 R moved.txt\0was.txt\0 M other.txt\0?? ünï.txt\0",
        );
        assert_eq!(statuses.len(), 4);
        // In the worktree, after `mv was.txt moved.txt && git add -N moved.txt`.
        assert_eq!(statuses["moved.txt"], "renamed");
        assert_eq!(statuses["new name.txt"], "renamed");
        assert_eq!(statuses["other.txt"], "modified");
        assert_eq!(statuses["ünï.txt"], "untracked");
    }

    #[test]
    fn outside_a_repository_walks_the_directory_without_hidden_entries() {
        let dir = fresh_dir("plain");
        for f in ["b.txt", "A.txt", "Zdir/x.txt", "adir/y.txt", ".hidden", ".dotdir/z.txt"] {
            write(&dir, f);
        }
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        // temp_dir is no repository here, or this test would see one.
        assert_eq!(tree_root(&dir), dir);
        let tree = list(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(!tree.repo);
        assert_eq!(names(&tree.nodes), ["adir", "empty", "Zdir", "A.txt", "b.txt"]);
        assert_eq!(find(&tree.nodes, "Zdir/x.txt").unwrap().status, None);
        assert!(tree.nodes.iter().all(|n| n.status.is_none()));
    }

    #[test]
    fn a_tree_past_the_cap_is_cut_and_says_so() {
        let dir = fresh_dir("cap");
        write(&dir, "top.txt");
        for i in 0..MAX_ENTRIES {
            write(&dir, &format!("many/{i}"));
        }
        let tree = list(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(tree.truncated);
        assert_eq!(count(&tree.nodes), MAX_ENTRIES);
        // Breadth first: the top level is whole.
        assert_eq!(names(&tree.nodes), ["many", "top.txt"]);
    }

    #[test]
    fn a_row_resolves_under_the_root_and_nowhere_else() {
        let dir = fresh_dir("resolve");
        let root = dir.join("root");
        write(&root, "src/main rs.txt");
        write(&dir, "outside.txt");

        assert_eq!(resolve(&root, "src/main rs.txt").unwrap(), root.join("src/main rs.txt"));
        assert_eq!(resolve(&root, "src").unwrap(), root.join("src"));
        for bad in ["", "../outside.txt", "src/../../outside.txt", "./src", "src/missing.txt"] {
            assert!(resolve(&root, bad).is_err(), "{bad:?} resolved");
        }
        // A deleted file passes the check alone, for the diff of it.
        assert!(check_relative("src/missing.txt").is_ok());
        assert!(check_relative("src/../x").is_err());
        let absolute = dir.join("outside.txt");
        assert!(resolve(&root, &absolute.to_string_lossy()).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&absolute, root.join("link.txt")).unwrap();
            assert!(resolve(&root, "link.txt").unwrap_err().contains("outside"));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_path_for_a_diff_is_made_relative_to_the_root() {
        let dir = fresh_dir("relative");
        let root = dir.join("repo");
        write(&root, "src/main.rs");
        write(&dir, "outside.txt");

        let abs = |rel: &str| root.join(rel).to_string_lossy().into_owned();
        assert_eq!(relative_to(&root, &abs("src/main.rs")).unwrap(), "src/main.rs");
        // Deleted from disk, it is still a file to diff.
        assert_eq!(relative_to(&root, &abs("src/gone.rs")).unwrap(), "src/gone.rs");
        assert_eq!(relative_to(&root, "src/main.rs").unwrap(), "src/main.rs");
        // A relative path is rebuilt from its components too: stored as
        // the field says, one `/` apart, whoever wrote it.
        assert_eq!(relative_to(&root, "src//main.rs").unwrap(), "src/main.rs");
        #[cfg(windows)]
        assert_eq!(relative_to(&root, "src\\main.rs").unwrap(), "src/main.rs");
        assert!(relative_to(&root, &dir.join("outside.txt").to_string_lossy()).is_err());
        assert!(relative_to(&root, "../outside.txt").is_err());
        assert!(relative_to(&root, &abs("")).is_err(), "the root itself is no file");
        // Spelled through a link to the root, it is the same file.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root, dir.join("alias")).unwrap();
            let aliased = dir.join("alias/src/gone.rs");
            assert_eq!(relative_to(&root, &aliased.to_string_lossy()).unwrap(), "src/gone.rs");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unreadable_root_is_an_error() {
        let missing = std::env::temp_dir().join("mirador-tree-no-such-dir");
        assert!(list(&missing).is_err());
    }
}
