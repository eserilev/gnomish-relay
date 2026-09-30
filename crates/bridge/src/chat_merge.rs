//! Merge of a chat branch into the branch that the chat started from (SPEC.md 9.10). The
//! test merge changes no folder, so a conflict never reaches the start branch.

use std::path::{Path, PathBuf};

use crate::chat_branch::{ChatWorktree, is_clean, tip};
use crate::git_host::{GitHost, nul_parts};

const NO_START: &str =
    "This chat started from a detached HEAD, so there's no branch to merge into.";
const NOT_CLEAN: &str = "Commit or revert this chat's changes first, then press Merge.";

/// A merge that the test found clean, with what the desktop dialog names.
#[derive(Debug, PartialEq, Eq)]
pub struct Ready {
    pub start: String,
    pub start_tip: String,
    pub branch_tip: String,
    /// The tree of the merge result.
    pub tree: String,
}

/// The test of `git merge-tree`: a clean tree, or the files with a conflict.
enum Test {
    Clean(String),
    Conflict(Vec<String>),
}

fn merge_test(git: &GitHost, top: &Path, start: &str, branch: &str) -> Result<Test, String> {
    let args = [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "-z",
        "--no-messages",
        start,
        branch,
    ];
    let output = git.output(top, &args).map_err(|e| e.line)?;
    let parts = nul_parts(&output.stdout);
    let tree = parts
        .first()
        .map(|t| String::from_utf8_lossy(t).trim().to_owned())
        .unwrap_or_default();
    match output.status.code() {
        Some(0) => Ok(Test::Clean(tree)),
        Some(1) => Ok(Test::Conflict(
            parts[1..]
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect(),
        )),
        _ => Err(crate::git_host::error_of(&output).line),
    }
}

fn names(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
    match shown.as_slice() {
        [] => "the same files".into(),
        [one] => (*one).to_owned(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}

/// The start branch goes into the chat copy, so the agent can fix the conflicts there.
fn start_merge_in_copy(
    git: &GitHost,
    worktree: &ChatWorktree,
    start: &str,
    files: &[String],
) -> String {
    let copy = Path::new(&worktree.worktree);
    let _ = git.output(copy, &["merge", "--no-edit", "--no-verify", start]);
    format!(
        "Can't merge yet: {start} also changed {}. I started the merge in this chat's copy. Ask the agent to fix the conflicts, then press Commit and Merge again.",
        names(files)
    )
}

/// Steps 1 to 3 of Merge: nothing outside the chat copy changes here.
pub fn prepare(git: &GitHost, worktree: &ChatWorktree) -> Result<Ready, String> {
    let start = worktree
        .start_branch
        .clone()
        .ok_or_else(|| NO_START.to_owned())?;
    if !is_clean(git, Path::new(&worktree.worktree))? {
        return Err(NOT_CLEAN.into());
    }
    let top = Path::new(&worktree.repo);
    let merged = git
        .yes(
            top,
            &["merge-base", "--is-ancestor", &worktree.branch, &start],
        )
        .map_err(|e| e.line)?;
    if merged {
        return Err(format!(
            "Nothing to merge: {start} already has this chat's work."
        ));
    }
    let tree = match merge_test(git, top, &start, &worktree.branch)? {
        Test::Clean(tree) => tree,
        Test::Conflict(files) => return Err(start_merge_in_copy(git, worktree, &start, &files)),
    };
    let start_tip = tip(git, top, &start).ok_or_else(|| format!("{start} is gone."))?;
    let branch_tip =
        tip(git, top, &worktree.branch).ok_or_else(|| format!("{} is gone.", worktree.branch))?;
    Ok(Ready {
        start,
        start_tip,
        branch_tip,
        tree,
    })
}

/// Fixed text and names from git, never text from the game (SPEC.md 6.6.3).
pub fn approval_text(worktree: &ChatWorktree, ready: &Ready, repo: &str) -> String {
    format!(
        "A chat from WoW asks to merge {} into {} in {repo}. Approve only if you just clicked Merge in WoW.",
        worktree.branch, ready.start
    )
}

/// The folder that has `branch` checked out, from `git worktree list`.
fn checked_out_in(git: &GitHost, top: &Path, branch: &str) -> Option<PathBuf> {
    let list = git
        .bytes(top, &["worktree", "list", "--porcelain", "-z"])
        .ok()?;
    let wanted = format!("branch refs/heads/{branch}");
    let mut folder = None;
    for part in nul_parts(&list) {
        let line = String::from_utf8_lossy(part);
        if let Some(path) = line.strip_prefix("worktree ") {
            folder = Some(PathBuf::from(path));
        } else if line == wanted {
            return folder;
        }
    }
    None
}

/// `git merge` in the folder that has the start branch, with an abort when git stops
/// in the middle.
fn merge_in(git: &GitHost, folder: &Path, worktree: &ChatWorktree) -> Result<(), String> {
    let merged = git.bytes(
        folder,
        &["merge", "--no-edit", "--no-verify", &worktree.branch],
    );
    let Err(e) = merged else {
        return Ok(());
    };
    if git.yes(folder, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]) == Ok(true) {
        let _ = git.output(folder, &["merge", "--abort"]);
    }
    Err(format!(
        "Couldn't merge in {}: {}",
        folder.display(),
        e.line
    ))
}

/// No folder has the start branch: the branch moves, and only from the commit that
/// the test saw.
fn move_branch(
    git: &GitHost,
    top: &Path,
    worktree: &ChatWorktree,
    ready: &Ready,
) -> Result<(), String> {
    let fast_forward = git
        .yes(
            top,
            &[
                "merge-base",
                "--is-ancestor",
                &ready.start_tip,
                &ready.branch_tip,
            ],
        )
        .map_err(|e| e.line)?;
    let target = if fast_forward {
        ready.branch_tip.clone()
    } else {
        let message = format!("Merge branch '{}'", worktree.branch);
        let args = [
            "commit-tree",
            &ready.tree,
            "-p",
            &ready.start_tip,
            "-p",
            &ready.branch_tip,
            "-m",
            &message,
        ];
        git.text(top, &args).map_err(|e| e.line)?
    };
    let reference = format!("refs/heads/{}", ready.start);
    git.bytes(top, &["update-ref", &reference, &target, &ready.start_tip])
        .map(|_| ())
        .map_err(|e| format!("Couldn't merge: {e}"))
}

/// Step 5, after Approve on the desktop.
pub fn apply(git: &GitHost, worktree: &ChatWorktree, ready: &Ready) -> Result<String, String> {
    let top = Path::new(&worktree.repo);
    match checked_out_in(git, top, &ready.start) {
        Some(folder) => merge_in(git, &folder, worktree)?,
        None => move_branch(git, top, worktree, ready)?,
    }
    crate::run::log(&format!(
        "merged {} into {} in {}",
        worktree.branch, ready.start, worktree.repo
    ));
    Ok(format!("Merged {} into {}.", worktree.branch, ready.start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_branch::make;
    use crate::folder_walk::Walk;
    use crate::git_host::UserConfig;
    use crate::lane::ChatId;
    use std::fs;

    struct Repo {
        _tmp: tempfile::TempDir,
        top: PathBuf,
        git: GitHost,
        chat: ChatWorktree,
    }

    impl Repo {
        fn run(&self, dir: &Path, args: &[&str]) -> String {
            String::from_utf8(self.git.bytes(dir, args).unwrap()).unwrap()
        }

        fn copy(&self) -> &Path {
            Path::new(&self.chat.worktree)
        }

        fn commit_in(&self, dir: &Path, file: &str, text: &str) {
            fs::write(dir.join(file), text).unwrap();
            self.run(dir, &["add", "-A"]);
            self.run(dir, &["commit", "-q", "-m", file]);
        }
    }

    fn repo() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let top = root.join("app");
        fs::create_dir_all(&top).unwrap();
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
        ] {
            git.bytes(&top, args).unwrap();
        }
        fs::write(top.join("a.txt"), "a\n").unwrap();
        git.bytes(&top, &["add", "-A"]).unwrap();
        git.bytes(&top, &["commit", "-q", "-m", "one"]).unwrap();
        let walk = Walk {
            roots: vec![root],
            deny: Vec::new(),
            home: None,
        };
        let chat = make(&git, &walk, &ChatId::new("c"), "work", &top)
            .unwrap()
            .unwrap();
        Repo {
            _tmp: tmp,
            top,
            git,
            chat,
        }
    }

    #[test]
    fn a_merge_of_new_work_fast_forwards_the_checked_out_start_branch() {
        let repo = repo();
        repo.commit_in(repo.copy(), "b.txt", "b\n");

        let ready = prepare(&repo.git, &repo.chat).unwrap();
        let reply = apply(&repo.git, &repo.chat, &ready).unwrap();

        assert_eq!(reply, "Merged gnomish/work into main.");
        assert!(repo.top.join("b.txt").is_file());
        assert_eq!(
            repo.run(&repo.top, &["rev-parse", "main"]),
            repo.run(&repo.top, &["rev-parse", "gnomish/work"])
        );
    }

    #[test]
    fn a_merge_after_work_on_main_makes_a_merge_commit() {
        let repo = repo();
        repo.commit_in(repo.copy(), "b.txt", "b\n");
        repo.commit_in(&repo.top, "c.txt", "c\n");

        let ready = prepare(&repo.git, &repo.chat).unwrap();
        apply(&repo.git, &repo.chat, &ready).unwrap();

        let parents = repo.run(&repo.top, &["rev-list", "--parents", "-n", "1", "main"]);
        assert_eq!(parents.split_whitespace().count(), 3);
        assert!(repo.top.join("b.txt").is_file());
    }

    #[test]
    fn a_start_branch_that_no_folder_has_moves_by_itself() {
        let repo = repo();
        repo.run(&repo.top, &["checkout", "-q", "-b", "other"]);
        repo.commit_in(repo.copy(), "b.txt", "b\n");
        repo.commit_in(&repo.top, "o.txt", "o\n");

        let ready = prepare(&repo.git, &repo.chat).unwrap();
        apply(&repo.git, &repo.chat, &ready).unwrap();

        let files = repo.run(&repo.top, &["ls-tree", "--name-only", "main"]);
        assert_eq!(files, "a.txt\nb.txt\n");
        assert!(!repo.top.join("b.txt").exists());
    }

    #[test]
    fn a_conflict_changes_nothing_outside_the_chat_copy_and_starts_the_merge_in_it() {
        let repo = repo();
        repo.commit_in(repo.copy(), "a.txt", "chat\n");
        repo.commit_in(&repo.top, "a.txt", "main\n");
        let main = repo.run(&repo.top, &["rev-parse", "main"]);

        let error = prepare(&repo.git, &repo.chat).unwrap_err();

        assert_eq!(
            error,
            "Can't merge yet: main also changed a.txt. I started the merge in this chat's copy. Ask the agent to fix the conflicts, then press Commit and Merge again."
        );
        assert_eq!(repo.run(&repo.top, &["rev-parse", "main"]), main);
        assert_eq!(
            fs::read_to_string(repo.top.join("a.txt")).unwrap(),
            "main\n"
        );
        assert!(
            fs::read_to_string(repo.copy().join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );
    }

    #[test]
    fn merge_refuses_a_chat_copy_with_changes() {
        let repo = repo();
        fs::write(repo.copy().join("wip.txt"), "wip").unwrap();

        assert_eq!(prepare(&repo.git, &repo.chat), Err(NOT_CLEAN.into()));
    }

    #[test]
    fn merge_of_a_branch_that_main_holds_says_there_is_nothing() {
        let repo = repo();

        let error = prepare(&repo.git, &repo.chat).unwrap_err();

        assert_eq!(
            error,
            "Nothing to merge: main already has this chat's work."
        );
    }

    #[test]
    fn merge_with_changes_in_the_start_folder_aborts_and_says_so() {
        let repo = repo();
        repo.commit_in(repo.copy(), "a.txt", "chat\n");
        let ready = prepare(&repo.git, &repo.chat).unwrap();
        fs::write(repo.top.join("a.txt"), "the user's edit\n").unwrap();

        let error = apply(&repo.git, &repo.chat, &ready).unwrap_err();

        assert!(error.starts_with("Couldn't merge in "), "{error}");
        assert_eq!(
            fs::read_to_string(repo.top.join("a.txt")).unwrap(),
            "the user's edit\n"
        );
    }

    #[test]
    fn the_desktop_text_names_both_branches_and_the_folder() {
        let repo = repo();
        repo.commit_in(repo.copy(), "b.txt", "b\n");
        let ready = prepare(&repo.git, &repo.chat).unwrap();

        let text = approval_text(&repo.chat, &ready, "~/Code/app");

        assert_eq!(
            text,
            "A chat from WoW asks to merge gnomish/work into main in ~/Code/app. Approve only if you just clicked Merge in WoW."
        );
    }

    #[test]
    fn the_names_of_a_conflict_join_with_and() {
        let files = [
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ];

        assert_eq!(names(&files[..1]), "a");
        assert_eq!(names(&files), "a, b and c");
    }
}
