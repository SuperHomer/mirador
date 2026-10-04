//! The commit graph: `git log` turned into rows placed in lanes, so a view
//! can draw the branch lines without knowing anything about git.
//!
//! Lane assignment is the whole job. Each lane is "waiting for" a commit
//! sha; a row claims the lane waiting for it (or takes a free one), then
//! hands that lane to its first parent and puts any further parents into
//! lanes of their own. Lines between rows come out of that as links, which
//! the view draws as straight or diagonal segments and never has to reason
//! about.
//!
//! Lanes are never compacted mid-history. Renumbering them would move a
//! branch sideways halfway down the graph, which reads as two branches
//! rather than one; freed lanes are reused from the left instead, so the
//! graph stays narrow without any line changing identity.

use std::path::Path;

use cmux_protocol::{GraphLink, GraphRef, GraphResult, GraphRow};

/// Field separator inside a log record: a byte no commit message contains.
const SEP: char = '\u{1f}';

/// Rows fetched by default. A graph is read, not scrolled forever, and
/// every row is a DOM node — a 50k-commit repository must not try to draw
/// itself.
pub const DEFAULT_LIMIT: usize = 400;

/// The graph of `repo`, newest first, across all local and remote refs.
pub fn load(repo: &Path, limit: usize) -> Result<GraphResult, String> {
    // One extra row answers "is there more?" without a second call.
    let probe = limit.saturating_add(1);
    let format = format!("--format=%H{SEP}%h{SEP}%P{SEP}%D{SEP}%an{SEP}%at{SEP}%s");
    let text = run_git(
        repo,
        &[
            "log",
            "--all",
            // Topological order with dates respected: parents never appear
            // above their children, and concurrent branches stay in the
            // order they were committed.
            "--date-order",
            "--no-color",
            &format!("--max-count={probe}"),
            &format,
        ],
    )?;

    let remotes = remotes(repo);
    let mut commits: Vec<Parsed> =
        text.lines().filter_map(|l| parse_record(l, &remotes)).collect();
    let truncated = commits.len() > limit;
    commits.truncate(limit);

    let (rows, lanes) = place(&commits);
    Ok(GraphResult {
        repo: repo.to_string_lossy().to_string(),
        rows,
        lanes,
        truncated,
    })
}

struct Parsed {
    sha: String,
    short: String,
    parents: Vec<String>,
    refs: Vec<GraphRef>,
    author: String,
    timestamp: i64,
    subject: String,
}

fn parse_record(line: &str, remotes: &[String]) -> Option<Parsed> {
    let mut f = line.split(SEP);
    let sha = f.next()?.trim().to_string();
    if sha.is_empty() {
        return None;
    }
    let short = f.next()?.to_string();
    let parents = f
        .next()?
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let refs = parse_refs(f.next()?, remotes);
    let author = f.next()?.to_string();
    let timestamp = f.next()?.trim().parse().unwrap_or(0);
    // The subject may itself contain the separator only if a commit message
    // does, which `%s` cannot produce — but joining the remainder is free
    // insurance against ever being wrong about that.
    let subject = f.collect::<Vec<_>>().join(&SEP.to_string());
    Some(Parsed {
        sha,
        short,
        parents,
        refs,
        author,
        timestamp,
        subject,
    })
}

/// `%D` → refs. "HEAD -> main, origin/main, tag: v0.1.15" becomes three,
/// each knowing what it is so the view can colour them apart.
fn parse_refs(decoration: &str, remotes: &[String]) -> Vec<GraphRef> {
    decoration
        .split(',')
        .filter_map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            // "HEAD -> main" is the checked-out branch; bare "HEAD" is a
            // detached one, which is still worth showing.
            if let Some(branch) = raw.strip_prefix("HEAD -> ") {
                return Some(GraphRef {
                    name: branch.to_string(),
                    kind: "head".into(),
                });
            }
            if raw == "HEAD" {
                return Some(GraphRef {
                    name: "HEAD".into(),
                    kind: "head".into(),
                });
            }
            if let Some(tag) = raw.strip_prefix("tag: ") {
                return Some(GraphRef {
                    name: tag.to_string(),
                    kind: "tag".into(),
                });
            }
            // `feature/login` is a local branch and `origin/main` is not,
            // and only the repository's remote list tells them apart.
            let kind = if remotes
                .iter()
                .any(|r| raw.strip_prefix(r.as_str()).is_some_and(|rest| rest.starts_with('/')))
            {
                "remote"
            } else {
                "branch"
            };
            Some(GraphRef {
                name: raw.to_string(),
                kind: kind.into(),
            })
        })
        .collect()
}

/// Assigns every commit a lane and the links leaving its row. Returns the
/// rows and how many lanes were ever occupied.
fn place(commits: &[Parsed]) -> (Vec<GraphRow>, u32) {
    // `lanes[i]` is the sha lane `i` is waiting to draw, if any.
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());
    let mut widest = 0usize;

    for commit in commits {
        // The lane already reserved by a child, else the leftmost free one.
        // A commit with no child in view (a branch tip) starts a new lane.
        let lane = match lanes.iter().position(|l| l.as_deref() == Some(commit.sha.as_str())) {
            Some(i) => i,
            None => free_lane(&mut lanes),
        };
        // Clear it first: the first parent usually takes this same lane,
        // and a parent must never match the slot we are about to fill.
        lanes[lane] = None;

        let mut links: Vec<GraphLink> = Vec::new();
        let mut assigned: Vec<usize> = Vec::new();
        for (i, parent) in commit.parents.iter().enumerate() {
            match lanes.iter().position(|l| l.as_deref() == Some(parent.as_str())) {
                // Another lane is already waiting for this parent: the two
                // histories rejoin, so the line crosses to that lane.
                Some(existing) => links.push(GraphLink {
                    from: lane as u32,
                    to: existing as u32,
                }),
                None => {
                    // The first parent continues this commit's own lane, so
                    // a branch keeps one column for its whole length.
                    let target = if i == 0 { lane } else { free_lane(&mut lanes) };
                    lanes[target] = Some(parent.clone());
                    assigned.push(target);
                    links.push(GraphLink {
                        from: lane as u32,
                        to: target as u32,
                    });
                }
            }
        }

        // Every *other* occupied lane passes straight through this row.
        // Lanes this commit just handed to a parent already have their link
        // and must not get a second, vertical one through the same gap.
        for (i, slot) in lanes.iter().enumerate() {
            if i != lane && slot.is_some() && !assigned.contains(&i) {
                links.push(GraphLink {
                    from: i as u32,
                    to: i as u32,
                });
            }
        }

        widest = widest.max(lanes.iter().rposition(|l| l.is_some()).map_or(0, |i| i + 1));
        widest = widest.max(lane + 1);

        rows.push(GraphRow {
            sha: commit.sha.clone(),
            short: commit.short.clone(),
            subject: commit.subject.clone(),
            author: commit.author.clone(),
            timestamp: commit.timestamp,
            refs: commit.refs.clone(),
            lane: lane as u32,
            links,
            merge: commit.parents.len() > 1,
        });
    }

    (rows, widest as u32)
}

/// The leftmost unused lane, extending the set only when all are busy.
fn free_lane(lanes: &mut Vec<Option<String>>) -> usize {
    match lanes.iter().position(|l| l.is_none()) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// The repository's remote names. Failure is an empty list: a ref is then
/// labelled a branch, which is what a repository with no remotes has.
fn remotes(repo: &Path) -> Vec<String> {
    run_git(repo, &["remote"])
        .map(|text| text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
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
            "git log failed".into()
        } else {
            err
        });
    }
    // A commit message need not be UTF-8; lossy keeps the graph readable
    // rather than failing the whole pane over one author's encoding.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the parsed form directly: these tests are about lane
    /// placement, not about `git log`'s output format.
    fn c(sha: &str, parents: &[&str]) -> Parsed {
        Parsed {
            sha: sha.into(),
            short: sha.chars().take(7).collect(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            refs: Vec::new(),
            author: "A".into(),
            timestamp: 0,
            subject: format!("commit {sha}"),
        }
    }

    fn links_of(row: &GraphRow) -> Vec<(u32, u32)> {
        let mut l: Vec<(u32, u32)> = row.links.iter().map(|l| (l.from, l.to)).collect();
        l.sort_unstable();
        l
    }

    #[test]
    fn a_linear_history_is_one_lane() {
        let (rows, lanes) = place(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(lanes, 1);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(links_of(&rows[0]), vec![(0, 0)]);
        assert_eq!(links_of(&rows[1]), vec![(0, 0)]);
        // A root commit has no parent, so nothing leaves its row.
        assert_eq!(links_of(&rows[2]), vec![]);
        assert!(rows.iter().all(|r| !r.merge));
    }

    #[test]
    fn a_merge_sends_its_second_parent_to_a_new_lane() {
        //   m  (merge of f into a)
        //   |\
        //   a f
        //   |/
        //   base
        let (rows, lanes) = place(&[
            c("m", &["a", "f"]),
            c("a", &["base"]),
            c("f", &["base"]),
            c("base", &[]),
        ]);
        assert_eq!(lanes, 2, "the side branch needs a column of its own");

        // The merge sits in lane 0 and opens lane 1 for its second parent.
        assert_eq!(rows[0].lane, 0);
        assert!(rows[0].merge);
        assert_eq!(links_of(&rows[0]), vec![(0, 0), (0, 1)]);

        // `a` keeps lane 0; lane 1 carries `f` past it.
        assert_eq!(rows[1].lane, 0);
        assert_eq!(links_of(&rows[1]), vec![(0, 0), (1, 1)]);

        // `f` is in lane 1 and rejoins `base`, already awaited in lane 0 —
        // and lane 0 also carries `a`'s line down to that same `base`, two
        // rows below, so this gap holds both segments.
        assert_eq!(rows[2].lane, 1);
        assert_eq!(links_of(&rows[2]), vec![(0, 0), (1, 0)]);

        assert_eq!(rows[3].lane, 0);
        assert!(!rows[3].merge);
    }

    #[test]
    fn two_tips_each_start_their_own_lane() {
        // Two unrelated heads, as `--all` produces. Both have history, so
        // both lanes stay occupied and the second tip cannot reuse the first.
        let (rows, lanes) = place(&[c("x", &["x0"]), c("y", &["y0"])]);
        assert_eq!(lanes, 2);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1, "a tip with no child cannot reuse a busy lane");
    }

    #[test]
    fn disconnected_roots_share_a_lane() {
        // The other half of that: a root frees its lane at once, so an
        // unrelated commit below it reuses the column. Nothing links them.
        let (rows, lanes) = place(&[c("x", &[]), c("y", &[])]);
        assert_eq!(lanes, 1, "an empty column should not be left standing");
        assert_eq!(rows[1].lane, 0);
        assert!(rows[0].links.is_empty() && rows[1].links.is_empty());
    }

    #[test]
    fn a_freed_lane_is_reused_from_the_left() {
        // `side` rejoins at `b`, freeing lane 1 while lane 0 is still
        // carrying `b` — so the next tip reuses lane 1 and the graph does
        // not grow a column for every branch that ever existed.
        let (rows, lanes) = place(&[
            c("m", &["a", "side"]),
            c("a", &["b"]),
            c("side", &["b"]),
            c("tip2", &["other"]),
            c("b", &["root"]),
        ]);
        assert_eq!(lanes, 2, "lane 1 is free once `side` merges back");
        assert_eq!(rows[3].lane, 1, "the new tip takes the freed lane");
        assert_eq!(rows[4].lane, 0, "`b` still holds the lane it was promised");
    }

    #[test]
    fn an_octopus_merge_opens_a_lane_per_extra_parent() {
        let (rows, lanes) = place(&[c("o", &["p1", "p2", "p3"]), c("p1", &[])]);
        assert!(rows[0].merge);
        assert_eq!(lanes, 3);
        assert_eq!(links_of(&rows[0]), vec![(0, 0), (0, 1), (0, 2)]);
    }

    #[test]
    fn parents_outside_the_window_simply_end() {
        // The row limit cuts history: `a`'s parent is never seen, and the
        // line it reserved just stops rather than breaking placement.
        let (rows, lanes) = place(&[c("a", &["cut-off"])]);
        assert_eq!(lanes, 1);
        assert_eq!(links_of(&rows[0]), vec![(0, 0)]);
    }

    #[test]
    fn refs_are_split_and_classified() {
        let remotes = vec!["origin".to_string()];
        let refs = parse_refs("HEAD -> main, origin/main, tag: v0.1.15, feature/login", &remotes);
        assert_eq!(refs.len(), 4);
        assert_eq!((refs[0].name.as_str(), refs[0].kind.as_str()), ("main", "head"));
        assert_eq!((refs[1].name.as_str(), refs[1].kind.as_str()), ("origin/main", "remote"));
        assert_eq!((refs[2].name.as_str(), refs[2].kind.as_str()), ("v0.1.15", "tag"));
        assert_eq!((refs[3].name.as_str(), refs[3].kind.as_str()), ("feature/login", "branch"));
        // No decoration at all is no refs, not one empty one.
        assert!(parse_refs("", &remotes).is_empty());
        // A detached HEAD still labels its commit.
        assert_eq!(parse_refs("HEAD", &remotes)[0].kind, "head");
        // With no remote configured, a slashed name is just a branch.
        assert_eq!(parse_refs("origin/main", &[])[0].kind, "branch");
    }

    #[test]
    fn a_record_parses_into_its_fields() {
        let line = format!(
            "abc123def{SEP}abc123d{SEP}p1 p2{SEP}HEAD -> main{SEP}Yoan{SEP}1730000000{SEP}Fix the thing"
        );
        let p = parse_record(&line, &["origin".to_string()]).expect("a well-formed record");
        assert_eq!(p.sha, "abc123def");
        assert_eq!(p.short, "abc123d");
        assert_eq!(p.parents, vec!["p1", "p2"]);
        assert_eq!(p.author, "Yoan");
        assert_eq!(p.timestamp, 1730000000);
        assert_eq!(p.subject, "Fix the thing");
        assert_eq!(p.refs[0].name, "main");
        // A root commit's empty parent field is no parents.
        let line = format!("sha{SEP}sha{SEP}{SEP}{SEP}A{SEP}0{SEP}subject");
        assert!(parse_record(&line, &[]).unwrap().parents.is_empty());
        // Junk is skipped rather than panicking.
        assert!(parse_record("", &[]).is_none());
        assert!(parse_record("no-separators-here", &[]).is_none());
    }
}
