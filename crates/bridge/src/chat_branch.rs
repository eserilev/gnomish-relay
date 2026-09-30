//! The own branch of a chat: a linked worktree next to the repository (SPEC.md 9.10).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::folder_walk::{Walk, is_shown};
use crate::git_host::GitHost;
use crate::lane::ChatId;
use crate::new_folder::real_chat_folder;

/// The hidden folder next to a repository that holds the copies of its chats.
pub const WORKTREES: &str = ".gnomish-worktrees";
pub const BRANCH_PREFIX: &str = "gnomish/";
const MAX_SLUG: usize = 40;
/// More chats than this with one name in one repository is not normal use.
const MAX_SUFFIX: u32 = 99;
const NO_COMMITS: &str = "This repo has no commits yet, so the chat can't have its own branch. Make a first commit, or start a chat without Own branch.";

/// The worktree of one chat. `state.json` keeps it, so the next run of the chat finds it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatWorktree {
    pub chat: ChatId,
    /// The top of the repository that the chat started in.
    pub repo: String,
    pub worktree: String,
    /// The chat folder inside the worktree: its top, or the same subfolder as in the repository.
    pub folder: String,
    pub branch: String,
    /// `None` for a chat that started at a detached `HEAD`.
    pub start_branch: Option<String>,
    pub start_commit: String,
}

impl ChatWorktree {
    /// A worktree that someone removed by hand has no `.git` file.
    pub fn exists(&self) -> bool {
        Path::new(&self.worktree).join(".git").is_file()
    }
}

/// The branch of a chat folder, for the `B` block of a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchInfo {
    /// Empty for a detached `HEAD`.
    pub branch: String,
    pub own: Own,
    pub start: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Own {
    Yes,
    No,
}

/// Lower case letters and digits, with one `-` for each run of anything else.
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(MAX_SLUG).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "chat".into()
    } else {
        slug.to_owned()
    }
}

/// The top of the repository of `folder`, or `None` outside a repository.
pub fn repo_top(git: &GitHost, folder: &Path) -> Option<PathBuf> {
    let top = git.text(folder, &["rev-parse", "--show-toplevel"]).ok()?;
    PathBuf::from(top).canonicalize().ok()
}

/// The branch that `HEAD` names, or `None` for a detached `HEAD`.
pub fn current_branch(git: &GitHost, folder: &Path) -> Option<String> {
    git.text(folder, &["symbolic-ref", "-q", "--short", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty())
}

fn head_commit(git: &GitHost, folder: &Path) -> Option<String> {
    git.text(folder, &["rev-parse", "-q", "--verify", "HEAD"])
        .ok()
        .filter(|c| !c.is_empty())
}

fn branch_exists(git: &GitHost, top: &Path, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    git.yes(top, &["show-ref", "--verify", "--quiet", &reference])
        .unwrap_or(true)
}

/// `<the folder above the repository>/.gnomish-worktrees/<repository name>`, which must
/// be inside a root and never a link.
fn copies_folder(walk: &Walk, top: &Path) -> Result<PathBuf, String> {
    let outside = || {
        format!(
            "Couldn't give this chat its own branch: the folder above {} isn't in allowed_roots. Add it in config.toml, or start a chat without Own branch.",
            top.display()
        )
    };
    let (Some(above), Some(name)) = (top.parent(), top.file_name()) else {
        return Err(outside());
    };
    real_chat_folder(walk, above).map_err(|_| outside())?;
    let copies = above.join(WORKTREES).join(name);
    if !is_shown(walk, &copies) {
        return Err(outside());
    }
    fs::create_dir_all(&copies).map_err(|e| format!("Couldn't make {}: {e}", copies.display()))?;
    let real = copies.canonicalize().map_err(|e| e.to_string())?;
    if real != copies {
        return Err(format!(
            "Couldn't give this chat its own branch: {} is a link. Remove it, then send again.",
            copies.display()
        ));
    }
    Ok(copies)
}

/// A name that no branch and no folder has yet: `slug`, then `slug-2`, and so on.
fn free_name(git: &GitHost, top: &Path, copies: &Path, slug: &str) -> Option<String> {
    (1..=MAX_SUFFIX)
        .map(|n| match n {
            1 => slug.to_owned(),
            n => format!("{slug}-{n}"),
        })
        .find(|name| {
            fs::symlink_metadata(copies.join(name)).is_err()
                && !branch_exists(git, top, &format!("{BRANCH_PREFIX}{name}"))
        })
}

/// Makes the worktree of a chat at the first run. `Ok(None)` for a folder outside a
/// repository: the chat then works in its folder.
pub fn make(
    git: &GitHost,
    walk: &Walk,
    chat: &ChatId,
    name: &str,
    folder: &Path,
) -> Result<Option<ChatWorktree>, String> {
    let Some(top) = repo_top(git, folder) else {
        return Ok(None);
    };
    let start_commit = head_commit(git, &top).ok_or_else(|| NO_COMMITS.to_owned())?;
    let start_branch = current_branch(git, &top);
    let inside = folder
        .canonicalize()
        .ok()
        .and_then(|f| f.strip_prefix(&top).ok().map(Path::to_path_buf))
        .unwrap_or_default();
    let copies = copies_folder(walk, &top)?;
    let slug = slug(name);
    let free = free_name(git, &top, &copies, &slug).ok_or_else(|| {
        format!("Couldn't give this chat its own branch: too many chats named {slug}.")
    })?;
    let worktree = copies.join(&free);
    let branch = format!("{BRANCH_PREFIX}{free}");
    let args = [
        "worktree".as_ref(),
        "add".as_ref(),
        "-q".as_ref(),
        "-b".as_ref(),
        branch.as_ref(),
        worktree.as_os_str(),
        start_commit.as_ref(),
    ];
    git.bytes::<&std::ffi::OsStr>(&top, &args)
        .map_err(|e| format!("Couldn't give this chat its own branch: {e}"))?;
    let folder = real_chat_folder(walk, &worktree.join(inside))?;
    Ok(Some(ChatWorktree {
        chat: chat.clone(),
        repo: top.to_string_lossy().into_owned(),
        worktree: worktree.to_string_lossy().into_owned(),
        folder,
        branch,
        start_branch,
        start_commit,
    }))
}

/// The `B` block of a reply: the branch of the chat folder, and whether it is its own.
pub fn branch_info(git: &GitHost, folder: &Path, own: Option<&ChatWorktree>) -> Option<BranchInfo> {
    repo_top(git, folder)?;
    let branch = current_branch(git, folder).unwrap_or_default();
    Some(match own {
        Some(worktree) => BranchInfo {
            branch,
            own: Own::Yes,
            start: worktree.start_branch.clone().unwrap_or_default(),
        },
        None => BranchInfo {
            branch,
            own: Own::No,
            start: String::new(),
        },
    })
}

/// No change that is not committed, and no untracked file.
pub fn is_clean(git: &GitHost, folder: &Path) -> Result<bool, String> {
    git.bytes(
        folder,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )
    .map(|out| out.is_empty())
    .map_err(|e| e.to_string())
}

/// The last commit of a branch, for the log line that brings it back.
pub fn tip(git: &GitHost, top: &Path, branch: &str) -> Option<String> {
    let reference = format!("refs/heads/{branch}");
    git.text(top, &["rev-parse", "-q", "--verify", &reference])
        .ok()
}

/// Whether the start branch already holds every commit of the chat branch.
pub fn is_merged(git: &GitHost, worktree: &ChatWorktree) -> Result<bool, String> {
    let Some(start) = &worktree.start_branch else {
        return Ok(false);
    };
    git.yes(
        Path::new(&worktree.repo),
        &["merge-base", "--is-ancestor", &worktree.branch, start],
    )
    .map_err(|e| e.to_string())
}

fn remove_worktree(git: &GitHost, worktree: &ChatWorktree) -> Result<(), String> {
    let path = std::ffi::OsStr::new(&worktree.worktree);
    let args = [
        "worktree".as_ref(),
        "remove".as_ref(),
        "--force".as_ref(),
        path,
    ];
    git.bytes::<&std::ffi::OsStr>(Path::new(&worktree.repo), &args)
        .map(|_| ())
        .map_err(|e| e.to_string())?;
    remove_empty_parents(Path::new(&worktree.worktree));
    Ok(())
}

/// The folders of the copies go when their last copy goes.
fn remove_empty_parents(worktree: &Path) {
    for folder in worktree.ancestors().skip(1).take(2) {
        if fs::remove_dir(folder).is_err() {
            return;
        }
    }
}

fn delete_branch(git: &GitHost, worktree: &ChatWorktree) -> Result<(), String> {
    git.bytes(
        Path::new(&worktree.repo),
        &["branch", "-D", &worktree.branch],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Discard in the game: the copy and the branch go. Returns the reply.
pub fn discard(git: &GitHost, worktree: &ChatWorktree) -> Result<String, String> {
    let commit = tip(git, Path::new(&worktree.repo), &worktree.branch).unwrap_or_default();
    remove_worktree(git, worktree).map_err(|e| format!("Couldn't discard: {e}"))?;
    delete_branch(git, worktree).map_err(|e| format!("Couldn't discard: {e}"))?;
    crate::run::log(&format!(
        "discarded {} at {commit} in {}",
        worktree.branch, worktree.repo
    ));
    let short: String = commit.chars().take(7).collect();
    Ok(format!(
        "Discarded {}. To get it back, on your desktop run: git branch {} {short}",
        worktree.branch, worktree.branch
    ))
}

/// A deleted chat takes its copy and its branch along, but never work that exists
/// nowhere else. Returns one log line for each part.
pub fn remove_after_delete(git: &GitHost, worktree: &ChatWorktree) -> Vec<String> {
    let mut lines = Vec::new();
    match is_clean(git, Path::new(&worktree.worktree)) {
        Ok(true) => match remove_worktree(git, worktree) {
            Ok(()) => lines.push(format!("removed {}", worktree.worktree)),
            Err(e) => lines.push(format!("kept {}: {e}", worktree.worktree)),
        },
        Ok(false) => lines.push(format!(
            "kept {}: it has changes that aren't committed",
            worktree.worktree
        )),
        Err(e) => lines.push(format!("kept {}: {e}", worktree.worktree)),
    }
    let removed = !Path::new(&worktree.worktree).exists();
    match is_merged(git, worktree) {
        Ok(true) if removed => match delete_branch(git, worktree) {
            Ok(()) => lines.push(format!("deleted branch {}", worktree.branch)),
            Err(e) => lines.push(format!("kept branch {}: {e}", worktree.branch)),
        },
        Ok(true) => lines.push(format!("kept branch {}: its copy stays", worktree.branch)),
        Ok(false) | Err(_) => lines.push(format!(
            "kept branch {}: it has commits that the start branch doesn't",
            worktree.branch
        )),
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_host::UserConfig;

    struct Repo {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        top: PathBuf,
        git: GitHost,
    }

    fn run(git: &GitHost, dir: &Path, args: &[&str]) {
        git.bytes(dir, args).unwrap();
    }

    /// `root/app` with one commit on `main`, and `root` as the only root.
    fn repo() -> Repo {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("code");
        let top = root.join("app");
        fs::create_dir_all(top.join("src")).unwrap();
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        run(&git, &top, &["init", "-q", "-b", "main"]);
        run(&git, &top, &["config", "user.email", "t@t"]);
        run(&git, &top, &["config", "user.name", "t"]);
        fs::write(top.join("src/a.txt"), "a\n").unwrap();
        run(&git, &top, &["add", "-A"]);
        run(&git, &top, &["commit", "-q", "-m", "one"]);
        Repo {
            _tmp: tmp,
            root,
            top,
            git,
        }
    }

    fn walk(repo: &Repo) -> Walk {
        Walk {
            roots: vec![repo.root.clone()],
            deny: Vec::new(),
            home: None,
        }
    }

    fn make_for(repo: &Repo, name: &str) -> ChatWorktree {
        make(&repo.git, &walk(repo), &ChatId::new("c1"), name, &repo.top)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn a_slug_keeps_letters_and_digits_in_lower_case() {
        assert_eq!(slug("Fix the Tests 2!"), "fix-the-tests-2");
        assert_eq!(slug("  ---  "), "chat");
        assert_eq!(slug("ünï"), "n");
        assert_eq!(slug(&"x".repeat(80)).len(), MAX_SLUG);
    }

    #[test]
    fn an_own_branch_is_a_worktree_next_to_the_repository() {
        let repo = repo();

        let made = make_for(&repo, "Fix tests");

        let place = repo.root.join(".gnomish-worktrees/app/fix-tests");
        assert_eq!(made.worktree, place.to_string_lossy());
        assert_eq!(made.folder, place.to_string_lossy());
        assert_eq!(made.branch, "gnomish/fix-tests");
        assert_eq!(made.start_branch.as_deref(), Some("main"));
        assert!(place.join("src/a.txt").is_file());
        assert!(made.exists());
        assert_eq!(
            current_branch(&repo.git, &place).as_deref(),
            Some("gnomish/fix-tests")
        );
    }

    #[test]
    fn a_worktree_adds_only_its_git_folder_and_branch_to_the_repository() {
        let repo = repo();
        let before: Vec<_> = fs::read_dir(repo.top.join(".git"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();

        make_for(&repo, "one");

        let mut after: Vec<_> = fs::read_dir(repo.top.join(".git"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| !before.contains(n))
            .collect();
        after.sort();
        assert_eq!(after, ["worktrees"]);
    }

    #[test]
    fn a_second_chat_with_the_same_name_gets_another_branch() {
        let repo = repo();

        let first = make_for(&repo, "same");
        let second = make_for(&repo, "same");

        assert_eq!(first.branch, "gnomish/same");
        assert_eq!(second.branch, "gnomish/same-2");
        assert_ne!(first.worktree, second.worktree);
    }

    #[test]
    fn a_subfolder_chat_works_in_the_same_subfolder_of_its_copy() {
        let repo = repo();

        let made = make(
            &repo.git,
            &walk(&repo),
            &ChatId::new("c1"),
            "sub",
            &repo.top.join("src"),
        )
        .unwrap()
        .unwrap();

        assert!(
            made.folder.ends_with(".gnomish-worktrees/app/sub/src"),
            "{}",
            made.folder
        );
    }

    #[test]
    fn a_folder_outside_a_repository_gets_no_branch() {
        let repo = repo();
        let plain = repo.root.join("plain");
        fs::create_dir(&plain).unwrap();

        let made = make(&repo.git, &walk(&repo), &ChatId::new("c1"), "x", &plain);

        assert_eq!(made, Ok(None));
    }

    #[test]
    fn a_repository_that_is_a_root_itself_refuses_an_own_branch() {
        let repo = repo();
        let walk = Walk {
            roots: vec![repo.top.clone()],
            deny: Vec::new(),
            home: None,
        };

        let error = make(&repo.git, &walk, &ChatId::new("c1"), "x", &repo.top).unwrap_err();

        assert!(error.contains("isn't in allowed_roots"), "{error}");
        assert!(!repo.root.join(WORKTREES).exists());
    }

    #[test]
    fn a_repository_with_no_commit_refuses_an_own_branch() {
        let repo = repo();
        let empty = repo.root.join("empty");
        fs::create_dir(&empty).unwrap();
        run(&repo.git, &empty, &["init", "-q"]);

        let error = make(&repo.git, &walk(&repo), &ChatId::new("c1"), "x", &empty).unwrap_err();

        assert_eq!(error, NO_COMMITS);
    }

    #[test]
    fn discard_removes_the_copy_and_the_branch_and_names_the_commit() {
        let repo = repo();
        let made = make_for(&repo, "gone");
        let commit = tip(&repo.git, &repo.top, &made.branch).unwrap();

        let reply = discard(&repo.git, &made).unwrap();

        assert!(reply.contains(&commit[..7]), "{reply}");
        assert!(!Path::new(&made.worktree).exists());
        assert!(!branch_exists(&repo.git, &repo.top, &made.branch));
        assert!(!repo.root.join(WORKTREES).exists());
    }

    #[test]
    fn a_deleted_chat_removes_a_clean_merged_copy_and_its_branch() {
        let repo = repo();
        let made = make_for(&repo, "done");

        let lines = remove_after_delete(&repo.git, &made);

        assert!(!Path::new(&made.worktree).exists(), "{lines:?}");
        assert!(!branch_exists(&repo.git, &repo.top, &made.branch));
    }

    #[test]
    fn a_deleted_chat_keeps_a_copy_with_changes() {
        let repo = repo();
        let made = make_for(&repo, "busy");
        fs::write(Path::new(&made.worktree).join("new.txt"), "work").unwrap();

        let lines = remove_after_delete(&repo.git, &made);

        assert!(Path::new(&made.worktree).join("new.txt").is_file());
        assert!(branch_exists(&repo.git, &repo.top, &made.branch));
        assert!(lines[0].contains("aren't committed"), "{lines:?}");
    }

    #[test]
    fn a_deleted_chat_keeps_a_branch_with_commits_that_main_lacks() {
        let repo = repo();
        let made = make_for(&repo, "ahead");
        let copy = Path::new(&made.worktree);
        fs::write(copy.join("b.txt"), "b").unwrap();
        run(&repo.git, copy, &["add", "-A"]);
        run(&repo.git, copy, &["commit", "-q", "-m", "two"]);

        let lines = remove_after_delete(&repo.git, &made);

        assert!(!copy.exists());
        assert!(
            branch_exists(&repo.git, &repo.top, &made.branch),
            "{lines:?}"
        );
    }

    #[test]
    fn the_branch_info_says_whether_the_branch_is_the_chats_own() {
        let repo = repo();
        let made = make_for(&repo, "info");

        let own = branch_info(&repo.git, Path::new(&made.folder), Some(&made)).unwrap();
        let plain = branch_info(&repo.git, &repo.top, None).unwrap();

        assert_eq!(
            (own.branch.as_str(), own.own, own.start.as_str()),
            ("gnomish/info", Own::Yes, "main")
        );
        assert_eq!((plain.branch.as_str(), plain.own), ("main", Own::No));
    }
}
