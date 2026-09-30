//! What a run changed in a repository: a git tree of the chat folder at the start and
//! at the end of the run (SPEC.md 9.10). The index, the branches, and the stash of the
//! user never change.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git_host::{GitError, GitHost, nul_parts};
use crate::lane::{ChatId, MessageId};

/// The state of a work tree as git sees it: tracked and untracked files, not ignored ones.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub tree: String,
    /// `None` in a repository with no commit yet.
    pub head: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// Lines added and lines removed, or `None` for a binary file.
pub type LineCounts = Option<(u32, u32)>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    /// Relative to the top of the repository.
    pub path: String,
    /// `None` for a binary file.
    pub lines: LineCounts,
    pub kind: ChangeKind,
}

/// What became of the changes of a run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    #[default]
    Open,
    Committed,
    Reverted,
}

/// A run with a change summary. Commit and Revert act only on this record.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RunChanges {
    pub chat: ChatId,
    pub id: MessageId,
    /// The top of the repository, where every path of `files` starts.
    pub top: String,
    pub start: Snapshot,
    pub end: Snapshot,
    pub files: Vec<FileChange>,
    /// A file name that is not UTF-8 shows with `?`, and no action takes such a run.
    #[serde(default)]
    pub odd_names: bool,
    #[serde(default)]
    pub outcome: Outcome,
}

impl RunChanges {
    /// Lines added and lines removed, over the files with counts.
    pub fn totals(&self) -> (u32, u32) {
        self.files
            .iter()
            .filter_map(|f| f.lines)
            .fold((0, 0), |(a, r), (add, rem)| {
                (a.saturating_add(add), r.saturating_add(rem))
            })
    }

    pub fn paths(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }
}

fn head(git: &GitHost, top: &Path) -> Option<String> {
    git.text(top, &["rev-parse", "-q", "--verify", "HEAD"])
        .ok()
        .filter(|h| !h.is_empty())
}

/// A copy of the index keeps the file times, so `git add` reads only the changed files.
pub fn snapshot(git: &GitHost, top: &Path) -> Result<Snapshot, GitError> {
    let private = tempfile::tempdir_in(git.scratch())
        .map_err(|e| GitError::other(format!("no private folder for the snapshot: {e}")))?;
    let copy = private.path().join("index");
    let index = git.text(
        top,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?;
    if Path::new(&index).is_file() {
        std::fs::copy(&index, &copy)
            .map_err(|e| GitError::other(format!("cannot copy the index: {e}")))?;
    }
    git.with_index(top, &copy, &["add", "-A"])?;
    let tree = git.with_index(top, &copy, &["write-tree"])?;
    Ok(Snapshot {
        tree: String::from_utf8_lossy(&tree).trim().to_owned(),
        head: head(git, top),
    })
}

fn kind_of(status: &[u8]) -> ChangeKind {
    match status.first() {
        Some(b'A') => ChangeKind::Added,
        Some(b'D') => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    }
}

fn count(field: &[u8]) -> Option<u32> {
    std::str::from_utf8(field).ok()?.parse().ok()
}

/// One `added TAB removed TAB path` part of `--numstat -z`.
fn numstat_line(part: &[u8]) -> Option<(LineCounts, &[u8])> {
    let mut fields = part.splitn(3, |b| *b == b'\t');
    let (added, removed, path) = (fields.next()?, fields.next()?, fields.next()?);
    Some((count(added).zip(count(removed)), path))
}

/// The files that changed between two trees, and whether any name is not UTF-8.
pub fn changes(
    git: &GitHost,
    top: &Path,
    start: &str,
    end: &str,
) -> Result<(Vec<FileChange>, bool), GitError> {
    let base = ["diff-tree", "-r", "-z", "--no-renames"];
    let numstat = git.bytes(top, &[&base[..], &["--numstat", start, end]].concat())?;
    let statuses = git.bytes(top, &[&base[..], &["--name-status", start, end]].concat())?;
    let status_parts = nul_parts(&statuses);
    let mut files = Vec::new();
    let mut odd = false;
    for (i, part) in nul_parts(&numstat).into_iter().enumerate() {
        let Some((lines, path)) = numstat_line(part) else {
            continue;
        };
        let status = status_parts.get(2 * i).copied().unwrap_or(b"M");
        odd |= std::str::from_utf8(path).is_err();
        files.push(FileChange {
            path: String::from_utf8_lossy(path).into_owned(),
            lines,
            kind: kind_of(status),
        });
    }
    Ok((files, odd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_host::UserConfig;
    use std::fs;

    struct Repo {
        _tmp: tempfile::TempDir,
        top: std::path::PathBuf,
        git: GitHost,
    }

    fn run(repo: &Repo, args: &[&str]) -> Vec<u8> {
        repo.git.bytes(&repo.top, args).unwrap()
    }

    fn repo() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let repo = Repo {
            _tmp: tmp,
            top,
            git: GitHost::with_config(UserConfig::Skip).unwrap(),
        };
        run(&repo, &["init", "-q", "-b", "main"]);
        run(&repo, &["config", "user.email", "t@t"]);
        run(&repo, &["config", "user.name", "t"]);
        fs::write(repo.top.join("kept.txt"), "one\ntwo\n").unwrap();
        fs::write(repo.top.join("gone.txt"), "bye\n").unwrap();
        fs::write(repo.top.join(".gitignore"), "target/\n").unwrap();
        run(&repo, &["add", "-A"]);
        run(&repo, &["commit", "-q", "-m", "one"]);
        repo
    }

    fn diff(repo: &Repo, start: &Snapshot, end: &Snapshot) -> Vec<FileChange> {
        changes(&repo.git, &repo.top, &start.tree, &end.tree)
            .unwrap()
            .0
    }

    #[test]
    fn a_snapshot_changes_nothing_that_the_user_sees() {
        let repo = repo();
        fs::write(repo.top.join("kept.txt"), "one\nTWO\n").unwrap();
        fs::write(repo.top.join("new.txt"), "new\n").unwrap();
        let status = run(&repo, &["status", "--porcelain"]);
        let index = fs::read(repo.top.join(".git/index")).unwrap();

        snapshot(&repo.git, &repo.top).unwrap();

        assert_eq!(run(&repo, &["status", "--porcelain"]), status);
        assert_eq!(fs::read(repo.top.join(".git/index")).unwrap(), index);
        assert!(run(&repo, &["stash", "list"]).is_empty());
    }

    #[test]
    fn the_changes_hold_new_changed_and_removed_files_but_not_ignored_ones() {
        let repo = repo();
        let start = snapshot(&repo.git, &repo.top).unwrap();
        fs::write(repo.top.join("kept.txt"), "one\nTWO\nthree\n").unwrap();
        fs::remove_file(repo.top.join("gone.txt")).unwrap();
        fs::write(repo.top.join("new.txt"), "n\n").unwrap();
        fs::create_dir(repo.top.join("target")).unwrap();
        fs::write(repo.top.join("target/build.o"), "x").unwrap();

        let end = snapshot(&repo.git, &repo.top).unwrap();

        let files = diff(&repo, &start, &end);
        assert_eq!(
            files,
            [
                FileChange {
                    path: "gone.txt".into(),
                    lines: Some((0, 1)),
                    kind: ChangeKind::Removed
                },
                FileChange {
                    path: "kept.txt".into(),
                    lines: Some((2, 1)),
                    kind: ChangeKind::Modified
                },
                FileChange {
                    path: "new.txt".into(),
                    lines: Some((1, 0)),
                    kind: ChangeKind::Added
                },
            ]
        );
    }

    #[test]
    fn earlier_work_of_the_user_is_not_a_change_of_the_run() {
        let repo = repo();
        fs::write(repo.top.join("mine.txt"), "mine\n").unwrap();
        let start = snapshot(&repo.git, &repo.top).unwrap();
        fs::write(repo.top.join("agent.txt"), "agent\n").unwrap();

        let end = snapshot(&repo.git, &repo.top).unwrap();

        let paths: Vec<_> = diff(&repo, &start, &end)
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(paths, ["agent.txt"]);
    }

    #[test]
    fn a_binary_file_has_no_line_counts() {
        let repo = repo();
        let start = snapshot(&repo.git, &repo.top).unwrap();
        fs::write(repo.top.join("image.bin"), [0u8, 1, 2, 0, 255]).unwrap();

        let end = snapshot(&repo.git, &repo.top).unwrap();

        assert_eq!(diff(&repo, &start, &end)[0].lines, None);
    }

    #[test]
    fn a_run_with_no_change_has_the_same_tree() {
        let repo = repo();

        let start = snapshot(&repo.git, &repo.top).unwrap();
        let end = snapshot(&repo.git, &repo.top).unwrap();

        assert_eq!(start, end);
        assert!(diff(&repo, &start, &end).is_empty());
    }

    #[test]
    fn a_snapshot_records_the_commit_of_head() {
        let repo = repo();
        let head = String::from_utf8(run(&repo, &["rev-parse", "HEAD"])).unwrap();

        let snap = snapshot(&repo.git, &repo.top).unwrap();

        assert_eq!(snap.head.as_deref(), Some(head.trim()));
    }

    #[test]
    fn the_totals_add_up_the_counted_files() {
        let changes = RunChanges {
            chat: ChatId::new("c"),
            id: MessageId(1),
            top: "/r".into(),
            start: Snapshot {
                tree: "a".into(),
                head: None,
            },
            end: Snapshot {
                tree: "b".into(),
                head: None,
            },
            files: vec![
                FileChange {
                    path: "a".into(),
                    lines: Some((3, 1)),
                    kind: ChangeKind::Modified,
                },
                FileChange {
                    path: "b".into(),
                    lines: None,
                    kind: ChangeKind::Added,
                },
                FileChange {
                    path: "c".into(),
                    lines: Some((2, 0)),
                    kind: ChangeKind::Added,
                },
            ],
            odd_names: false,
            outcome: Outcome::Open,
        };

        assert_eq!(changes.totals(), (5, 1));
    }
}
