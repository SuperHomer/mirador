//! Git worktrees: the checkouts a diff pane can be pointed at.
//!
//! All worktrees of a repository share one object store and one ref store,
//! so `git worktree list` gives the same answer from any of them — the diff
//! pane only has to know which checkout it is currently reading, and the
//! user can move it to another.
//!
//! Parsed from `--porcelain` rather than the human listing, whose columns
//! are ambiguous once a path contains spaces.

use std::path::Path;

use cmux_protocol::Worktree;

/// Every worktree of the repository containing `repo`, main one first (the
/// order `git` itself reports). `current` is set on the entry whose path is
/// `repo`, so the caller can show which checkout a pane is reading.
pub fn list(repo: &Path) -> Result<Vec<Worktree>, String> {
    let text = run_git(repo, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = parse(&text);
    // The path git reports is canonical; the pane's may not be (a symlinked
    // /tmp on macOS is the usual culprit), so compare both resolved.
    let current = std::fs::canonicalize(repo).ok();
    for wt in &mut worktrees {
        wt.current = std::fs::canonicalize(&wt.path).ok() == current;
    }
    Ok(worktrees)
}

/// Resolves what a user or an agent typed — a path, or a branch name — to
/// one of the repository's worktrees. Branch names are what a human
/// remembers; paths are what the UI sends back.
pub fn resolve<'a>(worktrees: &'a [Worktree], wanted: &str) -> Option<&'a Worktree> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return None;
    }
    worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(wanted))
        .or_else(|| worktrees.iter().find(|w| w.path == wanted))
        // A path the user typed and the one git reports can differ by a
        // symlink or a trailing slash without naming different checkouts.
        .or_else(|| {
            let wanted = std::fs::canonicalize(wanted).ok()?;
            worktrees
                .iter()
                .find(|w| std::fs::canonicalize(&w.path).ok().as_ref() == Some(&wanted))
        })
}

/// Parses `git worktree list --porcelain`: stanzas of `key value` lines
/// separated by blank lines, each opening with `worktree <path>`.
///
/// Plain porcelain, not `-z`: a path containing a newline would split a
/// stanza, but such an entry is dropped rather than corrupting its
/// neighbours, and git's own `-z` exists precisely because that case is
/// exotic.
fn parse(text: &str) -> Vec<Worktree> {
    let mut out: Vec<Worktree> = Vec::new();
    for stanza in text.split("\n\n") {
        let mut wt: Option<Worktree> = None;
        for line in stanza.lines() {
            let (key, value) = match line.split_once(' ') {
                Some((k, v)) => (k, v.trim()),
                None => (line.trim(), ""),
            };
            match key {
                "worktree" if !value.is_empty() => {
                    wt = Some(Worktree {
                        path: value.to_string(),
                        // The first stanza is always the main worktree.
                        main: out.is_empty(),
                        ..Default::default()
                    });
                }
                "HEAD" => {
                    if let Some(w) = &mut wt {
                        w.head = Some(value.chars().take(8).collect());
                    }
                }
                "branch" => {
                    if let Some(w) = &mut wt {
                        // refs/heads/feature/login → feature/login
                        w.branch =
                            Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string());
                    }
                }
                // Both carry an optional reason we don't surface; their
                // presence is the fact that matters.
                "bare" => {
                    if let Some(w) = &mut wt {
                        w.bare = true;
                    }
                }
                "locked" => {
                    if let Some(w) = &mut wt {
                        w.locked = true;
                    }
                }
                "prunable" => {
                    if let Some(w) = &mut wt {
                        w.prunable = true;
                    }
                }
                _ => {}
            }
        }
        if let Some(w) = wt {
            out.push(w);
        }
    }
    out
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::proc::command("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "git worktree list failed".into()
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = "\
worktree /repo
HEAD 8f3c1d2e4b5a6c7d8e9f0a1b2c3d4e5f60718293
branch refs/heads/main

worktree /repo/../wt-login
HEAD 1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d
branch refs/heads/feature/login

worktree /wt-detached
HEAD 9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f30291827
detached

worktree /wt-stale
HEAD 5555555555555555555555555555555555555555
branch refs/heads/old
locked under review
prunable gitdir file points to non-existent location
";

    #[test]
    fn parses_every_stanza_shape() {
        let wts = parse(PORCELAIN);
        assert_eq!(wts.len(), 4);

        assert_eq!(wts[0].path, "/repo");
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[0].head.as_deref(), Some("8f3c1d2e"));
        assert!(wts[0].main, "the first stanza is the main worktree");

        // A slash in a branch name survives stripping the ref prefix.
        assert_eq!(wts[1].branch.as_deref(), Some("feature/login"));
        assert!(!wts[1].main);

        // Detached: a head to show, no branch to name it by.
        assert_eq!(wts[2].branch, None);
        assert_eq!(wts[2].head.as_deref(), Some("9c8d7e6f"));

        // `locked`/`prunable` carry reasons we ignore, but must register.
        assert!(wts[3].locked);
        assert!(wts[3].prunable);
    }

    #[test]
    fn resolves_by_branch_then_path() {
        let wts = parse(PORCELAIN);
        assert_eq!(resolve(&wts, "feature/login").unwrap().path, "/repo/../wt-login");
        assert_eq!(resolve(&wts, "/wt-detached").unwrap().head.as_deref(), Some("9c8d7e6f"));
        // Whitespace is what a shell leaves behind, not a different name.
        assert_eq!(resolve(&wts, "  main  ").unwrap().path, "/repo");
        assert!(resolve(&wts, "nope").is_none());
        assert!(resolve(&wts, "").is_none());
    }

    #[test]
    fn a_bare_main_repository_is_flagged() {
        let wts = parse("worktree /srv/repo.git\nbare\n");
        assert_eq!(wts.len(), 1);
        assert!(wts[0].bare);
        // Nothing is checked out, so there is no head or branch to show.
        assert_eq!(wts[0].head, None);
        assert_eq!(wts[0].branch, None);
    }

    #[test]
    fn garbage_yields_no_worktrees() {
        assert!(parse("").is_empty());
        assert!(parse("HEAD abc\nbranch refs/heads/x\n").is_empty());
    }
}
