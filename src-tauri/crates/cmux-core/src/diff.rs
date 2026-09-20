//! Git diffs for the diff pane: run `git`, parse the unified output into
//! structured hunks the frontend can render.
//!
//! No libgit2 — `git` itself is the one implementation that always agrees
//! with what the user sees on the command line, including their
//! `.gitattributes` and rename detection. The invocation is pinned
//! (`--no-color`, `--no-ext-diff`, explicit prefixes) so a user's `delta`
//! pager or `diff.noprefix` cannot reshape output we then parse.

use std::path::Path;

use cmux_protocol::{DiffFile, DiffHunk, DiffLine, DiffResult};

/// Lines kept per file. Beyond this the body is dropped (counts stay
/// exact) — a 60k-line vendored lockfile must not become 60k DOM rows.
const MAX_LINES_PER_FILE: usize = 4000;
/// Untracked files listed for the worktree diff.
const MAX_UNTRACKED: usize = 200;
/// Untracked files larger than this are shown as a stat line only.
const MAX_UNTRACKED_BYTES: u64 = 512 * 1024;

/// The working tree against HEAD — what `mira diff` shows by default.
pub const WORKTREE: &str = "worktree";
/// The index against HEAD.
pub const STAGED: &str = "staged";

/// Diff of `spec` in `repo`.
pub fn load(repo: &Path, spec: &str) -> Result<DiffResult, String> {
    let text = run_git(repo, &git_args(spec))?;
    let mut files = parse(&text);
    // `git diff HEAD` cannot see a file git has never been told about, and
    // a new file is most of what an agent's turn produces — so the
    // worktree view synthesizes their diffs rather than hiding the work.
    if spec == WORKTREE {
        files.extend(untracked(repo));
    }
    Ok(DiffResult {
        repo: repo.to_string_lossy().to_string(),
        spec: spec.to_string(),
        label: label(repo, spec),
        files,
    })
}

/// Rejects a revspec git cannot resolve, so `mira diff typo` fails in the
/// terminal the agent is watching instead of opening a pane whose only
/// content is an error. `rev-parse` without `--verify` accepts ranges
/// ("main...HEAD") as well as single revisions.
pub fn verify_spec(repo: &Path, spec: &str) -> Result<(), String> {
    if spec == WORKTREE || spec == STAGED {
        return Ok(());
    }
    run_git(repo, &["rev-parse", "--quiet", spec].map(String::from))
        .map(|_| ())
        .map_err(|_| format!("unknown revision `{spec}`"))
}

/// Git arguments for a spec. Anything that isn't one of the two working
/// states is a revspec: a range diffs its endpoints, a single commit shows
/// itself (`show` handles root commits, which `<sha>^..<sha>` does not).
fn git_args(spec: &str) -> Vec<String> {
    let common = [
        "--no-color",
        "--no-ext-diff",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "-M",
        "-U3",
    ]
    .map(String::from);
    let mut args: Vec<String> = match spec {
        WORKTREE => vec!["diff".into(), "HEAD".into()],
        STAGED => vec!["diff".into(), "--cached".into()],
        rev if rev.contains("..") => vec!["diff".into(), rev.into()],
        rev => vec!["show".into(), "--format=".into(), rev.into()],
    };
    // Flags go after the subcommand; the revspec must stay last.
    let tail = args.split_off(1);
    args.extend(common);
    args.extend(tail);
    args
}

fn label(repo: &Path, spec: &str) -> String {
    match spec {
        WORKTREE => "uncommitted changes".into(),
        STAGED => "staged changes".into(),
        rev if rev.contains("..") => rev.into(),
        rev => run_git(repo, &["show", "-s", "--format=%h %s", rev].map(String::from))
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| rev.to_string()),
    }
}

fn run_git(repo: &Path, args: &[String]) -> Result<String, String> {
    let output = crate::proc::command("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "git failed".into()
        } else {
            err
        });
    }
    // Diffs are bytes, not necessarily UTF-8 (a latin-1 source file, a
    // half-binary blob). Lossy keeps those files reviewable instead of
    // failing the whole diff.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parses `git diff` output into one entry per file.
pub fn parse(text: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current: Option<DiffFile> = None;
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut in_hunk = false;

    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }
            current = Some(new_file(rest));
            in_hunk = false;
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue; // preamble (a `git show` header, say)
        };

        if let Some((old_start, new_start, header)) = hunk_header(line) {
            old_no = old_start;
            new_no = new_start;
            in_hunk = true;
            file.hunks.push(DiffHunk {
                old_start,
                new_start,
                header,
                lines: Vec::new(),
            });
            continue;
        }

        if !in_hunk {
            match line {
                l if l.starts_with("new file mode") => file.status = "added".into(),
                l if l.starts_with("deleted file mode") => file.status = "deleted".into(),
                l if l.starts_with("Binary files") || l.starts_with("GIT binary patch") => {
                    file.binary = true;
                }
                l if l.starts_with("rename from ") => {
                    file.status = "renamed".into();
                    file.old_path = unprefix(&l["rename from ".len()..]);
                }
                l if l.starts_with("rename to ") => {
                    if let Some(p) = unprefix(&l["rename to ".len()..]) {
                        file.path = p;
                    }
                }
                l if l.starts_with("copy from ") => {
                    file.status = "copied".into();
                    file.old_path = unprefix(&l["copy from ".len()..]);
                }
                l if l.starts_with("copy to ") => {
                    if let Some(p) = unprefix(&l["copy to ".len()..]) {
                        file.path = p;
                    }
                }
                // The --- / +++ pair is the only unambiguous source of the
                // paths: everything after the marker is one path, so names
                // containing " b/" survive it. /dev/null means the file is
                // absent on that side.
                l if l.starts_with("--- ") => {
                    if let Some(p) = strip_side(&l[4..]) {
                        if file.old_path.is_none() {
                            file.old_path = Some(p);
                        }
                    }
                }
                l if l.starts_with("+++ ") => {
                    if let Some(p) = strip_side(&l[4..]) {
                        file.path = p;
                    }
                }
                _ => {}
            }
            continue;
        }

        // Inside a hunk.
        let (kind, content) = match line.chars().next() {
            Some('+') => ("add", &line[1..]),
            Some('-') => ("del", &line[1..]),
            Some(' ') => ("context", &line[1..]),
            Some('\\') => ("meta", line),
            // A bare empty line is an empty context line from a producer
            // that trimmed the marker; anything else ends the hunk body.
            None => ("context", line),
            _ => {
                in_hunk = false;
                continue;
            }
        };
        let (old_line, new_line) = match kind {
            "add" => {
                file.additions += 1;
                let at = new_no;
                new_no += 1;
                (None, Some(at))
            }
            "del" => {
                file.deletions += 1;
                let at = old_no;
                old_no += 1;
                (Some(at), None)
            }
            "meta" => (None, None),
            _ => {
                let (o, n) = (old_no, new_no);
                old_no += 1;
                new_no += 1;
                (Some(o), Some(n))
            }
        };
        if file.truncated {
            continue; // counts stay exact; the body stops growing
        }
        if let Some(hunk) = file.hunks.last_mut() {
            hunk.lines.push(DiffLine {
                kind: kind.into(),
                old_line,
                new_line,
                content: content.to_string(),
            });
        }
        if file.hunks.iter().map(|h| h.lines.len()).sum::<usize>() >= MAX_LINES_PER_FILE {
            file.truncated = true;
        }
    }
    if let Some(file) = current.take() {
        files.push(file);
    }
    files
}

fn new_file(header: &str) -> DiffFile {
    DiffFile {
        path: guess_path(header),
        old_path: None,
        status: "modified".into(),
        additions: 0,
        deletions: 0,
        binary: false,
        truncated: false,
        hunks: Vec::new(),
    }
}

/// Path from a `diff --git a/x b/x` header. Only a fallback — binary files
/// carry no --- / +++ pair, and those paths are all we get. Prefers the
/// split that makes both halves name the same file, which is every case
/// but a rename (where `rename to` supplies the real path anyway).
fn guess_path(header: &str) -> String {
    let candidates: Vec<usize> = header
        .match_indices(" b/")
        .map(|(i, _)| i)
        .chain(header.match_indices(" \"b/").map(|(i, _)| i))
        .collect();
    for i in &candidates {
        let (left, right) = (&header[..*i], &header[i + 1..]);
        if let (Some(a), Some(b)) = (unprefix(left), unprefix(right)) {
            if a == b {
                return b;
            }
        }
    }
    candidates
        .first()
        .and_then(|i| unprefix(&header[i + 1..]))
        .unwrap_or_else(|| header.to_string())
}

/// `a/src/foo.rs` → `src/foo.rs`, unquoting git's C-style escaping first.
fn unprefix(raw: &str) -> Option<String> {
    let path = unquote(raw.trim());
    let mut chars = path.char_indices();
    let (_, first) = chars.next()?;
    let (second_at, second) = chars.next()?;
    if (first == 'a' || first == 'b' || first == 'i' || first == 'w' || first == 'c')
        && second == '/'
    {
        return Some(path[second_at + 1..].to_string());
    }
    Some(path)
}

/// The same, but `/dev/null` (the absent side of an add or delete) is None.
fn strip_side(raw: &str) -> Option<String> {
    if unquote(raw.trim()) == "/dev/null" {
        return None;
    }
    unprefix(raw)
}

/// Git quotes paths with unusual bytes: `"a/caf\303\251.txt"`. Undo that,
/// octal escapes included, so the tree shows the real name.
fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        match next {
            b'n' => (out.push(b'\n'), i += 2).1,
            b't' => (out.push(b'\t'), i += 2).1,
            b'r' => (out.push(b'\r'), i += 2).1,
            b'"' | b'\\' => (out.push(next), i += 2).1,
            b'0'..=b'7' => {
                let end = (i + 4).min(bytes.len());
                let octal = &inner[i + 1..end];
                match u8::from_str_radix(octal, 8) {
                    Ok(byte) => (out.push(byte), i = end).1,
                    Err(_) => (out.push(bytes[i]), i += 1).1,
                }
            }
            _ => (out.push(next), i += 2).1,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `@@ -12,7 +14,9 @@ fn thing()` → (12, 14, "fn thing()").
fn hunk_header(line: &str) -> Option<(u32, u32, String)> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, tail) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let start = |r: &str, sign: char| -> Option<u32> {
        r.strip_prefix(sign)?.split(',').next()?.parse().ok()
    };
    Some((
        start(old, '-')?,
        start(new, '+')?,
        tail.trim().to_string(),
    ))
}

/// Files git has never been told about, as synthetic all-additions diffs.
fn untracked(repo: &Path) -> Vec<DiffFile> {
    let Ok(listing) = run_git(
        repo,
        &["ls-files", "--others", "--exclude-standard", "-z"].map(String::from),
    ) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for rel in listing.split('\0').filter(|p| !p.is_empty()) {
        if files.len() >= MAX_UNTRACKED {
            break;
        }
        files.push(untracked_file(repo, rel));
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn untracked_file(repo: &Path, rel: &str) -> DiffFile {
    let mut file = DiffFile {
        path: rel.to_string(),
        old_path: None,
        status: "untracked".into(),
        additions: 0,
        deletions: 0,
        binary: false,
        truncated: false,
        hunks: Vec::new(),
    };
    let full = repo.join(rel);
    let size = std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0);
    if size > MAX_UNTRACKED_BYTES {
        file.truncated = true;
        return file;
    }
    let Ok(bytes) = std::fs::read(&full) else {
        return file;
    };
    if bytes.iter().take(8000).any(|b| *b == 0) {
        file.binary = true;
        return file;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut content: Vec<&str> = text.split('\n').collect();
    // A trailing newline ends the last line; it doesn't start a new one.
    if content.last() == Some(&"") {
        content.pop();
    }
    file.additions = content.len() as u32;
    let lines: Vec<DiffLine> = content
        .iter()
        .take(MAX_LINES_PER_FILE)
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: "add".into(),
            old_line: None,
            new_line: Some(i as u32 + 1),
            content: l.trim_end_matches('\r').to_string(),
        })
        .collect();
    file.truncated = content.len() > lines.len();
    if !lines.is_empty() {
        file.hunks.push(DiffHunk {
            old_start: 0,
            new_start: 1,
            header: String::new(),
            lines,
        });
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/src/foo.rs b/src/foo.rs
index 1111111..2222222 100644
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -10,6 +10,7 @@ fn thing() {
 one
-two
+TWO
+two and a half
 three
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 4444444..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/img.png b/img.png
index 5555555..6666666 100644
Binary files a/img.png and b/img.png differ
";

    #[test]
    fn parses_files_statuses_and_counts() {
        let files = parse(SAMPLE);
        assert_eq!(files.len(), 4);

        assert_eq!(files[0].path, "src/foo.rs");
        assert_eq!(files[0].status, "modified");
        assert_eq!((files[0].additions, files[0].deletions), (2, 1));
        assert_eq!(files[0].hunks[0].header, "fn thing() {");

        assert_eq!(files[1].status, "added");
        assert_eq!(files[1].additions, 2);
        assert_eq!(files[2].status, "deleted");
        assert_eq!(files[2].deletions, 1);

        // Binary files carry no ---/+++ pair: the path comes from the
        // `diff --git` header alone.
        assert_eq!(files[3].path, "img.png");
        assert!(files[3].binary);
    }

    /// Line numbers are what makes a diff navigable; an off-by-one here
    /// mislabels every row below a hunk.
    #[test]
    fn line_numbers_track_both_sides() {
        let files = parse(SAMPLE);
        let lines = &files[0].hunks[0].lines;
        let at = |i: usize| (lines[i].kind.as_str(), lines[i].old_line, lines[i].new_line);
        assert_eq!(at(0), ("context", Some(10), Some(10)));
        assert_eq!(at(1), ("del", Some(11), None));
        assert_eq!(at(2), ("add", None, Some(11)));
        assert_eq!(at(3), ("add", None, Some(12)));
        assert_eq!(at(4), ("context", Some(12), Some(13)));
    }

    #[test]
    fn rename_keeps_both_paths() {
        let files = parse("\
diff --git a/old/name.rs b/new/name.rs
similarity index 92%
rename from old/name.rs
rename to new/name.rs
--- a/old/name.rs
+++ b/new/name.rs
@@ -1 +1 @@
-a
+b
");
        assert_eq!(files[0].status, "renamed");
        assert_eq!(files[0].old_path.as_deref(), Some("old/name.rs"));
        assert_eq!(files[0].path, "new/name.rs");
    }

    /// A path containing " b/" breaks any parser that splits the
    /// `diff --git` header on it; the +++ line is what saves us.
    #[test]
    fn paths_with_spaces_and_quotes() {
        let files = parse("\
diff --git a/my b/dir/file.txt b/my b/dir/file.txt
--- a/my b/dir/file.txt
+++ b/my b/dir/file.txt
@@ -1 +1 @@
-x
+y
diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"
--- \"a/caf\\303\\251.txt\"
+++ \"b/caf\\303\\251.txt\"
@@ -1 +1 @@
-x
+y
");
        assert_eq!(files[0].path, "my b/dir/file.txt");
        assert_eq!(files[1].path, "café.txt");
    }

    #[test]
    fn no_newline_marker_is_not_a_change() {
        let files = parse("\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
");
        assert_eq!((files[0].additions, files[0].deletions), (1, 1));
        assert!(files[0].hunks[0].lines.iter().any(|l| l.kind == "meta"));
    }

    /// The whole point of the cap: a huge file must not ship a huge body,
    /// but its +/- counts must still be right.
    #[test]
    fn huge_file_is_truncated_but_counted() {
        let mut text = String::from(
            "diff --git a/big.txt b/big.txt\n--- a/big.txt\n+++ b/big.txt\n@@ -1 +1,9000 @@\n",
        );
        for i in 0..9000 {
            text.push_str(&format!("+line {i}\n"));
        }
        let files = parse(&text);
        assert!(files[0].truncated);
        assert_eq!(files[0].additions, 9000);
        let shipped: usize = files[0].hunks.iter().map(|h| h.lines.len()).sum();
        assert!(shipped <= MAX_LINES_PER_FILE, "shipped {shipped} lines");
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = crate::proc::command("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be installed to run this test");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A throwaway repository. Tests that need history build their own:
    /// CI checks this project out shallow, so the ambient repo has no
    /// HEAD~1 and cannot stand in for one.
    fn new_repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cmux-diff-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        // A developer with global commit signing must not fail this test.
        git(&dir, &["config", "commit.gpgsign", "false"]);
        dir
    }

    /// End to end against a real repository: the invocation, the
    /// untracked synthesis and the labels are all things only `git`
    /// itself can confirm.
    #[test]
    fn load_reads_a_real_repository() {
        let dir = new_repo("load");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/keep.txt"), "one\ntwo\nthree\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", "first commit"]);

        std::fs::write(dir.join("src/keep.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(dir.join("src/fresh.txt"), "brand new\n").unwrap();

        let result = load(&dir, WORKTREE).unwrap();
        assert_eq!(result.label, "uncommitted changes");
        let by_path = |p: &str| {
            result
                .files
                .iter()
                .find(|f| f.path == p)
                .unwrap_or_else(|| panic!("{p} missing from {:?}", result.files))
        };
        assert_eq!(by_path("src/keep.txt").status, "modified");
        // The file git has never seen is the one an agent just wrote.
        let fresh = by_path("src/fresh.txt");
        assert_eq!(fresh.status, "untracked");
        assert_eq!(fresh.additions, 1);
        assert_eq!(fresh.hunks[0].lines[0].content, "brand new");

        // Staged sees nothing until something is added.
        assert!(load(&dir, STAGED).unwrap().files.is_empty());

        // A commit spec labels itself with its subject.
        let head = load(&dir, "HEAD").unwrap();
        assert!(head.label.ends_with("first commit"), "label: {}", head.label);
        assert_eq!(head.files.len(), 1);
        assert_eq!(head.files[0].status, "added");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_revisions_are_rejected_before_a_pane_opens() {
        let repo = new_repo("verify");
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "first"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        git(&repo, &["commit", "-qam", "second"]);

        assert!(verify_spec(&repo, WORKTREE).is_ok());
        assert!(verify_spec(&repo, STAGED).is_ok());
        assert!(verify_spec(&repo, "HEAD").is_ok());
        // Ranges reach git as-is, so they have to survive verification.
        assert!(verify_spec(&repo, "HEAD~1...HEAD").is_ok());
        assert_eq!(
            verify_spec(&repo, "no-such-ref-ever").unwrap_err(),
            "unknown revision `no-such-ref-ever`"
        );

        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn specs_map_to_git_arguments() {
        let args = |spec: &str| git_args(spec).join(" ");
        assert!(args(WORKTREE).starts_with("diff --no-color"));
        assert!(args(WORKTREE).ends_with(" HEAD"));
        assert!(args(STAGED).ends_with(" --cached"));
        assert!(args("main...HEAD").ends_with(" main...HEAD"));
        // A lone commit goes through `show`, which handles root commits.
        assert!(args("abc123").starts_with("show "));
        assert!(args("abc123").ends_with(" abc123"));
    }
}
