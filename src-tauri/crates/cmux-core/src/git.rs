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
    fn non_repo_is_none() {
        assert!(branch_for_cwd("/tmp").is_none() || find_repo_root("/tmp").is_some());
    }
}
