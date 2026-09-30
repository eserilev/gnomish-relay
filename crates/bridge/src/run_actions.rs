//! Commit and Revert of the change summary of one run (SPEC.md 9.11). They act only on
//! the record of the bridge, never on text from the game or the agent.

use std::fs;
use std::path::Path;

use crate::git_host::{GitError, GitHost, nul_parts};
use crate::run::log;
use crate::run_changes::{ChangeKind, Outcome, RunChanges, snapshot};

const NO_MESSAGE: &str = "Commit needs a message.";
const ODD_NAMES: &str =
    "A file name in this summary isn't UTF-8, so the game can't name it. Use git on your desktop.";

fn files(count: usize) -> String {
    match count {
        1 => "1 file".into(),
        n => format!("{n} files"),
    }
}

/// A second action on one summary changes nothing.
fn check_open(run: &RunChanges) -> Result<(), String> {
    match run.outcome {
        Outcome::Open if run.odd_names => Err(ODD_NAMES.into()),
        Outcome::Open => Ok(()),
        Outcome::Committed => Err("This change summary is already committed.".into()),
        Outcome::Reverted => Err("This change summary is already reverted.".into()),
    }
}

fn head(git: &GitHost, top: &Path) -> Option<String> {
    git.text(top, &["rev-parse", "-q", "--verify", "HEAD"])
        .ok()
        .filter(|h| !h.is_empty())
}

/// After `--`, git takes a path that starts with `-` as a path.
fn with_paths<'a>(args: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
    let mut all = args.to_vec();
    all.push("--");
    all.extend_from_slice(paths);
    all
}

/// Commits exactly the files of the summary, with their content now. A merge that waits
/// in the chat copy takes every change, which ends the merge.
pub fn commit(git: &GitHost, run: &RunChanges, message: &str) -> Result<String, String> {
    check_open(run)?;
    let message = message.trim();
    if message.is_empty() {
        return Err(NO_MESSAGE.into());
    }
    let top = Path::new(&run.top);
    let merging = git
        .yes(top, &["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .unwrap_or(false);
    let paths = run.paths();
    let result = if merging {
        git.bytes(top, &["add", "-A"])
            .and_then(|_| git.bytes(top, &["commit", "-q", "--no-verify", "-m", message]))
    } else {
        git.bytes(top, &with_paths(&["add", "-A"], &paths))
            .and_then(|_| {
                git.bytes(
                    top,
                    &with_paths(
                        &["commit", "-q", "--no-verify", "-m", message, "--only"],
                        &paths,
                    ),
                )
            })
    };
    if let Err(e) = result {
        return Err(commit_error(&e));
    }
    let short = git
        .text(top, &["rev-parse", "--short", "HEAD"])
        .unwrap_or_default();
    let branch =
        crate::chat_branch::current_branch(git, top).unwrap_or_else(|| "a detached HEAD".into());
    Ok(format!(
        "Committed {} as {short} on {branch}.",
        files(paths.len())
    ))
}

/// git says "nothing to commit" on its output, after a line about the branch.
fn commit_error(error: &GitError) -> String {
    if error.all.contains("nothing to commit") || error.all.contains("no changes added") {
        return "Nothing to commit: these changes are already committed.".into();
    }
    format!("Couldn't commit: {}", error.line)
}

/// The files of the run that changed after it, between the end tree and now.
fn changed_since(git: &GitHost, run: &RunChanges, now: &str) -> Result<Vec<String>, String> {
    let args = with_paths(
        &[
            "diff-tree",
            "-r",
            "-z",
            "--name-only",
            "--no-renames",
            &run.end.tree,
            now,
        ],
        &run.paths(),
    );
    let out = git.bytes(Path::new(&run.top), &args).map_err(|e| e.line)?;
    Ok(nul_parts(&out)
        .into_iter()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect())
}

fn later_changes(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
    format!(
        "Revert would also undo later changes to {}. Nothing changed.",
        shown.join(", ")
    )
}

/// Only the changes of this run go, and only while the files hold what the run left.
fn check_revert(git: &GitHost, run: &RunChanges) -> Result<(), String> {
    check_open(run)?;
    if run.start.head != run.end.head {
        return Err("The agent made a commit in this run, so Revert can't undo it.".into());
    }
    let top = Path::new(&run.top);
    if head(git, top) != run.end.head {
        return Err("These changes are committed now, so Revert can't undo them.".into());
    }
    let now = snapshot(git, top).map_err(|e| format!("Couldn't revert: {e}"))?;
    let later = changed_since(git, run, &now.tree)?;
    if !later.is_empty() {
        return Err(later_changes(&later));
    }
    Ok(())
}

/// A new file of the run goes, never through a link, and so does each folder above it
/// that is empty then, up to the top.
fn remove_new_file(top: &Path, path: &str) -> Result<(), String> {
    let file = top.join(path);
    let parent = file.parent().unwrap_or(top);
    let real_parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if !real_parent.starts_with(top) {
        return Err(format!("{path} is behind a link"));
    }
    if fs::symlink_metadata(&file).is_ok() {
        fs::remove_file(&file).map_err(|e| format!("{path}: {e}"))?;
    }
    for folder in parent.ancestors().take_while(|f| *f != top) {
        if fs::remove_dir(folder).is_err() {
            break;
        }
    }
    Ok(())
}

pub fn revert(git: &GitHost, run: &RunChanges) -> Result<String, String> {
    check_revert(git, run)?;
    let top = Path::new(&run.top);
    let (new, old): (Vec<_>, Vec<_>) = run.files.iter().partition(|f| f.kind == ChangeKind::Added);
    if !old.is_empty() {
        let source = format!("--source={}", run.start.tree);
        let paths: Vec<&str> = old.iter().map(|f| f.path.as_str()).collect();
        let args = with_paths(&["restore", &source, "--worktree"], &paths);
        git.bytes::<&str>(top, &args)
            .map_err(|e| format!("Couldn't revert: {e}"))?;
    }
    for file in &new {
        remove_new_file(top, &file.path).map_err(|e| format!("Couldn't revert: {e}"))?;
    }
    log(&format!(
        "reverted {} files in {}: git restore --source={} brings them back",
        run.files.len(),
        run.top,
        run.end.tree
    ));
    Ok(format!("Reverted {}.", files(run.files.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_host::UserConfig;
    use crate::lane::{ChatId, MessageId};
    use crate::run_changes::changes;
    use std::path::PathBuf;

    struct Repo {
        _tmp: tempfile::TempDir,
        top: PathBuf,
        git: GitHost,
    }

    impl Repo {
        fn run(&self, args: &[&str]) -> String {
            String::from_utf8(self.git.bytes(&self.top, args).unwrap()).unwrap()
        }

        fn write(&self, path: &str, text: &str) {
            let file = self.top.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }

        fn read(&self, path: &str) -> Option<String> {
            fs::read_to_string(self.top.join(path)).ok()
        }

        /// Records a run: `change` is what the agent does between the two snapshots.
        fn run_that(&self, change: impl FnOnce(&Repo)) -> RunChanges {
            let start = snapshot(&self.git, &self.top).unwrap();
            change(self);
            let end = snapshot(&self.git, &self.top).unwrap();
            let (files, odd_names) = changes(&self.git, &self.top, &start.tree, &end.tree).unwrap();
            RunChanges {
                chat: ChatId::new("c"),
                id: MessageId(7),
                top: self.top.to_string_lossy().into_owned(),
                start,
                end,
                files,
                odd_names,
                outcome: Outcome::Open,
            }
        }
    }

    fn repo() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo {
            top: tmp.path().canonicalize().unwrap(),
            _tmp: tmp,
            git: GitHost::with_config(UserConfig::Skip).unwrap(),
        };
        repo.run(&["init", "-q", "-b", "main"]);
        repo.run(&["config", "user.email", "t@t"]);
        repo.run(&["config", "user.name", "t"]);
        repo.write("a.txt", "a\n");
        repo.write("b.txt", "b\n");
        repo.run(&["add", "-A"]);
        repo.run(&["commit", "-q", "-m", "one"]);
        repo
    }

    #[test]
    fn commit_takes_only_the_files_of_the_summary() {
        let repo = repo();
        repo.write("mine.txt", "the user's work\n");
        let run = repo.run_that(|r| {
            r.write("a.txt", "agent\n");
            r.write("new/n.txt", "new\n");
        });

        let reply = commit(&repo.git, &run, "fix the test").unwrap();

        assert!(reply.starts_with("Committed 2 files as "), "{reply}");
        assert!(reply.ends_with(" on main."), "{reply}");
        let shown = repo.run(&["show", "--name-only", "--format=%s", "HEAD"]);
        assert_eq!(shown.trim(), "fix the test\n\na.txt\nnew/n.txt");
        assert_eq!(repo.run(&["status", "--porcelain"]), "?? mine.txt\n");
    }

    #[test]
    fn commit_keeps_what_the_user_staged_before() {
        let repo = repo();
        repo.write("b.txt", "staged by the user\n");
        repo.run(&["add", "b.txt"]);
        let run = repo.run_that(|r| r.write("a.txt", "agent\n"));

        commit(&repo.git, &run, "agent work").unwrap();

        assert_eq!(repo.run(&["status", "--porcelain"]), "M  b.txt\n");
    }

    #[test]
    fn commit_needs_a_message() {
        let repo = repo();
        let run = repo.run_that(|r| r.write("a.txt", "x\n"));

        assert_eq!(commit(&repo.git, &run, "  "), Err(NO_MESSAGE.into()));
    }

    #[test]
    fn a_committed_summary_takes_no_second_action() {
        let repo = repo();
        let mut run = repo.run_that(|r| r.write("a.txt", "x\n"));
        run.outcome = Outcome::Committed;

        assert!(
            commit(&repo.git, &run, "again")
                .unwrap_err()
                .contains("already committed")
        );
        assert!(
            revert(&repo.git, &run)
                .unwrap_err()
                .contains("already committed")
        );
    }

    #[test]
    fn commit_of_changes_that_are_committed_already_says_so() {
        let repo = repo();
        let run = repo.run_that(|r| r.write("a.txt", "x\n"));
        repo.run(&["commit", "-qam", "by hand"]);

        let error = commit(&repo.git, &run, "again").unwrap_err();

        assert_eq!(
            error,
            "Nothing to commit: these changes are already committed."
        );
    }

    #[test]
    fn commit_during_a_merge_takes_every_change_and_ends_the_merge() {
        let repo = repo();
        repo.run(&["checkout", "-q", "-b", "side"]);
        repo.write("a.txt", "side\n");
        repo.run(&["commit", "-qam", "side"]);
        repo.run(&["checkout", "-q", "main"]);
        repo.write("a.txt", "main\n");
        repo.run(&["commit", "-qam", "main"]);
        let merge = repo
            .git
            .output(&repo.top, &["merge", "--no-edit", "side"])
            .unwrap();
        assert!(!merge.status.success());
        let run = repo.run_that(|r| r.write("a.txt", "both\n"));

        commit(&repo.git, &run, "merge side").unwrap();

        let parents = repo.run(&["rev-list", "--parents", "-n", "1", "HEAD"]);
        assert_eq!(parents.split_whitespace().count(), 3);
    }

    #[test]
    fn revert_puts_back_only_the_changes_of_the_run() {
        let repo = repo();
        repo.write("b.txt", "the user's earlier work\n");
        repo.write("mine.txt", "mine\n");
        let run = repo.run_that(|r| {
            r.write("a.txt", "agent\n");
            r.write("b.txt", "agent on top\n");
            r.write("deep/new.txt", "new\n");
            fs::remove_file(r.top.join("mine.txt")).unwrap();
        });

        let reply = revert(&repo.git, &run).unwrap();

        assert_eq!(reply, "Reverted 4 files.");
        assert_eq!(repo.read("a.txt").as_deref(), Some("a\n"));
        assert_eq!(
            repo.read("b.txt").as_deref(),
            Some("the user's earlier work\n")
        );
        assert_eq!(repo.read("mine.txt").as_deref(), Some("mine\n"));
        assert!(!repo.top.join("deep").exists());
    }

    #[test]
    fn revert_leaves_the_index_alone() {
        let repo = repo();
        repo.write("b.txt", "staged\n");
        repo.run(&["add", "b.txt"]);
        let run = repo.run_that(|r| r.write("a.txt", "agent\n"));

        revert(&repo.git, &run).unwrap();

        assert_eq!(repo.run(&["status", "--porcelain"]), "M  b.txt\n");
    }

    #[test]
    fn revert_refuses_after_a_later_change_and_changes_nothing() {
        let repo = repo();
        let run = repo.run_that(|r| r.write("a.txt", "agent\n"));
        repo.write("a.txt", "the user after the run\n");

        let error = revert(&repo.git, &run).unwrap_err();

        assert_eq!(
            error,
            "Revert would also undo later changes to a.txt. Nothing changed."
        );
        assert_eq!(
            repo.read("a.txt").as_deref(),
            Some("the user after the run\n")
        );
    }

    #[test]
    fn revert_refuses_a_run_that_made_a_commit() {
        let repo = repo();
        let run = repo.run_that(|r| {
            r.write("a.txt", "agent\n");
            r.run(&["commit", "-qam", "agent"]);
        });

        let error = revert(&repo.git, &run).unwrap_err();

        assert!(error.contains("made a commit"), "{error}");
    }

    #[test]
    fn revert_refuses_changes_that_are_committed_now() {
        let repo = repo();
        let run = repo.run_that(|r| r.write("a.txt", "agent\n"));
        repo.run(&["commit", "-qam", "later"]);

        let error = revert(&repo.git, &run).unwrap_err();

        assert!(error.contains("committed now"), "{error}");
        assert_eq!(repo.read("a.txt").as_deref(), Some("agent\n"));
    }

    #[cfg(unix)]
    #[test]
    fn revert_never_removes_through_a_link() {
        let repo = repo();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep.txt"), "keep").unwrap();
        let run = repo.run_that(|r| r.write("sub/keep.txt", "new\n"));
        fs::remove_dir_all(repo.top.join("sub")).unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.top.join("sub")).unwrap();

        let result = remove_new_file(&repo.top, "sub/keep.txt");

        assert!(result.is_err());
        assert!(outside.path().join("keep.txt").exists());
        drop(run);
    }
}
