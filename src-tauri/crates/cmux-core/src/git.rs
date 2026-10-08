//! Git branch detection for the sidebar. No libgit2 — reading HEAD is a
//! one-line file read, and that's all the sidebar needs.

use std::path::{Path, PathBuf};

/// Walks up from `dir` to the repository root (the directory containing
/// `.git`). Handles worktrees/submodules where `.git` is a file.
pub fn find_repo_root(dir: &str) -> Option<PathBuf> {
    let mut current = Path::new(dir);
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Current branch name, or a short sha when detached.
pub fn read_branch(repo_root: &Path) -> Option<String> {
    let git_path = repo_root.join(".git");
    // Worktree/submodule: `.git` is a file with `gitdir: <path>`.
    let git_dir = if git_path.is_file() {
        let text = std::fs::read_to_string(&git_path).ok()?;
        let target = text.strip_prefix("gitdir:")?.trim();
        if Path::new(target).is_absolute() {
            PathBuf::from(target)
        } else {
            repo_root.join(target)
        }
    } else {
        git_path
    };

    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        return reference
            .strip_prefix("refs/heads/")
            .map(str::to_string)
            .or_else(|| Some(reference.to_string()));
    }
    // Detached HEAD: short sha.
    Some(head.chars().take(8).collect())
}

/// The repository a checkout belongs to: the main checkout's root for a
/// linked worktree, and `repo_root` itself otherwise. Two file reads, no
/// `git`: a worktree's `.git` file names its git dir
/// (`<main>/.git/worktrees/<name>`), whose `commondir` file points at the
/// shared `<main>/.git`. A submodule's git dir has no `commondir`, so a
/// submodule stays a repository of its own.
pub fn main_checkout(repo_root: &Path) -> PathBuf {
    let linked = || -> Option<PathBuf> {
        let git_path = repo_root.join(".git");
        if !git_path.is_file() {
            return None;
        }
        let text = std::fs::read_to_string(&git_path).ok()?;
        let target = text.strip_prefix("gitdir:")?.trim();
        let git_dir = repo_root.join(target);
        let common = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
        let common = git_dir.join(common.trim());
        // `<main>/.git` → `<main>`; a bare repository's common dir is the
        // repository itself, and names no checkout.
        let common = std::fs::canonicalize(&common).unwrap_or(common);
        (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf))?
    };
    // Canonical either way, so the same repository reached through a
    // symlink (`/tmp` and `/private/tmp`) is one project, not two.
    linked().unwrap_or_else(|| std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf()))
}

/// Branch of the repository containing `cwd`, with its root.
pub fn branch_for_cwd(cwd: &str) -> Option<(PathBuf, String)> {
    let root = find_repo_root(cwd)?;
    let branch = read_branch(&root)?;
    Some((root, branch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(head: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmux-git-{}-{}",
            std::process::id(),
            head.len()
        ));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src/nested")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), head).unwrap();
        dir
    }

    #[test]
    fn branch_from_nested_dir() {
        let dir = fixture("ref: refs/heads/feature/login\n");
        let (root, branch) =
            branch_for_cwd(dir.join("src/nested").to_str().unwrap()).unwrap();
        assert_eq!(root, dir);
        assert_eq!(branch, "feature/login");
    }

    #[test]
    fn detached_head_short_sha() {
        let dir = fixture("0123456789abcdef0123456789abcdef01234567\n");
        let (_, branch) = branch_for_cwd(dir.to_str().unwrap()).unwrap();
        assert_eq!(branch, "01234567");
    }

    #[test]
    fn a_linked_worktree_belongs_to_its_main_checkout() {
        let base = std::env::temp_dir().join(format!("cmux-git-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let main = base.join("repo");
        let wt_git = main.join(".git/worktrees/feature");
        std::fs::create_dir_all(&wt_git).unwrap();
        std::fs::write(wt_git.join("commondir"), "../..\n").unwrap();
        let wt = base.join("repo-feature");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", wt_git.display())).unwrap();

        let main = std::fs::canonicalize(&main).unwrap();
        assert_eq!(main_checkout(&wt), main);
        // The main checkout, and a repository with no worktrees, are their
        // own — reached through a symlink or not.
        assert_eq!(main_checkout(&main), main);
        #[cfg(unix)]
        {
            let link = base.join("link");
            std::os::unix::fs::symlink(&main, &link).unwrap();
            assert_eq!(main_checkout(&link), main);
        }

        // A submodule's git dir has no commondir: it is a repository of its own.
        let sub = base.join("sub");
        let sub_git = main.join(".git/modules/sub");
        std::fs::create_dir_all(&sub_git).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), format!("gitdir: {}\n", sub_git.display())).unwrap();
        assert_eq!(main_checkout(&sub), std::fs::canonicalize(&sub).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn non_repo_is_none() {
        assert!(branch_for_cwd("/tmp").is_none() || find_repo_root("/tmp").is_some());
    }
}
