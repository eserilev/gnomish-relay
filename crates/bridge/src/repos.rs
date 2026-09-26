//! The git repositories inside `allowed_roots`, for the folder picker (SPEC.md 9.9).
//! A bounded walk: a root can be a whole home folder.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use protocol::action::{ToolCall, Verdict, classify};

use crate::action_input::{self, resolved_bytes};

/// Folders that hold tools, builds, or packages, never a repository of the user.
const SKIPPED: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "vendor",
    "venv",
    "__pycache__",
    "Library",
    "AppData",
];

/// The files that change with the work in a repository. The newest one dates it.
const ACTIVITY: &[&str] = &["index", "logs/HEAD", "HEAD"];

/// Without a walk into a repository, the repositories are never more than the visits.
pub struct Limits {
    /// A root has depth 0.
    pub depth: usize,
    pub visits: usize,
    pub time: Duration,
}

pub const LIMITS: Limits = Limits {
    depth: 4,
    visits: 3000,
    time: Duration::from_secs(2),
};

#[derive(Debug, PartialEq, Eq)]
pub struct Repo {
    pub path: PathBuf,
    /// The last change, in seconds since 1970.
    pub updated: u32,
}

/// Where the walk may look, and what it never shows. All paths are resolved.
#[derive(Clone)]
pub struct Walk {
    pub roots: Vec<PathBuf>,
    /// The config folder and the data folder of the bridge.
    pub deny: Vec<PathBuf>,
}

fn is_skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED.contains(&name)
}

fn is_repo(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(".git")).is_ok()
}

#[allow(clippy::cast_possible_truncation)] // u32 seconds last until 2106
fn seconds(time: SystemTime) -> u32 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

fn changed_at(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .map_or(0, seconds)
}

/// A worktree has a `.git` file, so only the file itself dates it.
fn updated(repo: &Path) -> u32 {
    let git = repo.join(".git");
    let mut newest = changed_at(&git);
    for name in ACTIVITY {
        newest = newest.max(changed_at(&git.join(name)));
    }
    newest
}

/// The classifier gives the answer of an agent that reads the folder. Only a folder
/// that it reads with no question shows: never a credential or a bridge folder.
fn is_shown(walk: &Walk, dir: &Path) -> bool {
    let policy = action_input::policy(&walk.roots, dir, &walk.deny, &[]);
    let read = ToolCall::Files {
        reads: vec![resolved_bytes(dir)],
        writes: Vec::new(),
    };
    classify(&read, &policy, &[]) == Verdict::Allow
}

/// The subfolders of `dir`. A link is never a subfolder, so the walk stays in the roots.
fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let name = entry.file_name();
        if is_dir && !is_skipped(&name.to_string_lossy()) {
            found.push(entry.path());
        }
    }
    found.sort();
    found
}

/// Shallow folders first, so a cap cuts off the deepest ones. The walk does not go
/// into a repository: its folders belong to it.
pub fn find_repos(walk: &Walk, limits: &Limits) -> Vec<Repo> {
    let deadline = Instant::now() + limits.time;
    let mut queue: VecDeque<(PathBuf, usize)> = walk.roots.iter().map(|r| (r.clone(), 0)).collect();
    let mut seen = BTreeSet::new();
    let mut repos = Vec::new();
    while let Some((dir, depth)) = queue.pop_front() {
        if seen.len() >= limits.visits || Instant::now() >= deadline {
            break;
        }
        if !seen.insert(dir.clone()) {
            continue;
        }
        if is_repo(&dir) {
            if is_shown(walk, &dir) {
                let updated = updated(&dir);
                repos.push(Repo { path: dir, updated });
            }
            continue;
        }
        if depth < limits.depth {
            queue.extend(subfolders(&dir).into_iter().map(|d| (d, depth + 1)));
        }
    }
    repos
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree {
        _tmp: tempfile::TempDir,
        home: PathBuf,
    }

    fn tree() -> Tree {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        Tree { _tmp: tmp, home }
    }

    fn repo(at: &Path) {
        fs::create_dir_all(at.join(".git")).unwrap();
        fs::write(at.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }

    fn names(repos: &[Repo], home: &Path) -> Vec<String> {
        repos
            .iter()
            .map(|r| {
                let rel = r.path.strip_prefix(home).unwrap();
                rel.to_string_lossy().replace('\\', "/")
            })
            .collect()
    }

    fn find(roots: &[PathBuf], deny: &[PathBuf], limits: &Limits) -> Vec<Repo> {
        let walk = Walk {
            roots: roots.to_vec(),
            deny: deny.to_vec(),
        };
        find_repos(&walk, limits)
    }

    fn find_home(t: &Tree, limits: &Limits) -> Vec<Repo> {
        find(std::slice::from_ref(&t.home), &[], limits)
    }

    #[test]
    fn the_walk_finds_repositories_down_to_the_depth_limit() {
        let t = tree();
        repo(&t.home.join("Code/app"));
        repo(&t.home.join("Code/a/b/c/deep"));
        repo(&t.home.join("Code/a/b/c/d/deeper"));
        let roots = [t.home.join("Code")];

        let found = find(&roots, &[], &LIMITS);

        assert_eq!(names(&found, &t.home), ["Code/app", "Code/a/b/c/deep"]);
    }

    #[test]
    fn a_root_that_is_a_repository_is_listed() {
        let t = tree();
        repo(&t.home);
        let found = find_home(&t, &LIMITS);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, t.home);
    }

    #[test]
    fn the_walk_does_not_go_into_a_repository() {
        let t = tree();
        repo(&t.home.join("app"));
        repo(&t.home.join("app/vendored"));
        let found = find_home(&t, &LIMITS);
        assert_eq!(names(&found, &t.home), ["app"]);
    }

    #[test]
    fn hidden_and_build_folders_are_skipped() {
        let t = tree();
        repo(&t.home.join(".hidden/app"));
        repo(&t.home.join("node_modules/pkg"));
        repo(&t.home.join("target/app"));
        repo(&t.home.join("real"));
        let found = find_home(&t, &LIMITS);
        assert_eq!(names(&found, &t.home), ["real"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_never_followed() {
        let t = tree();
        let root = t.home.join("Code");
        fs::create_dir_all(&root).unwrap();
        repo(&t.home.join("outside/secret"));
        std::os::unix::fs::symlink(t.home.join("outside"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(t.home.join("outside/secret"), root.join("repo")).unwrap();

        assert!(find(&[root], &[], &LIMITS).is_empty());
    }

    /// A junction is a link to a folder that needs no rights to make.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_never_followed() {
        let t = tree();
        let root = t.home.join("Code");
        fs::create_dir_all(&root).unwrap();
        repo(&t.home.join("outside/secret"));
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(t.home.join("outside"))
            .status()
            .unwrap();
        assert!(made.success());

        assert!(find(&[root], &[], &LIMITS).is_empty());
    }

    #[test]
    fn a_folder_outside_the_roots_is_never_listed() {
        let t = tree();
        repo(&t.home.join("Code/app"));
        repo(&t.home.join("Other/app"));
        let found = find(&[t.home.join("Code")], &[], &LIMITS);
        assert_eq!(names(&found, &t.home), ["Code/app"]);
    }

    #[test]
    fn a_deny_folder_and_a_credential_folder_are_never_listed() {
        let t = tree();
        repo(&t.home.join("config/gnomish-relay"));
        repo(&t.home.join("snap/firefox"));
        repo(&t.home.join("Code/app"));
        let deny = [t.home.join("config/gnomish-relay")];

        let found = find(std::slice::from_ref(&t.home), &deny, &LIMITS);

        assert_eq!(names(&found, &t.home), ["Code/app"]);
    }

    #[test]
    fn overlapping_roots_list_a_repository_once() {
        let t = tree();
        repo(&t.home.join("Code/app"));
        let roots = [t.home.clone(), t.home.join("Code")];
        assert_eq!(find(&roots, &[], &LIMITS).len(), 1);
    }

    #[test]
    fn the_walk_stops_at_the_visit_cap() {
        let t = tree();
        for name in ["a", "b", "c"] {
            repo(&t.home.join(name));
        }
        let roots = [t.home.clone()];
        let few_visits = Limits {
            visits: 3,
            ..LIMITS
        };

        assert_eq!(names(&find(&roots, &[], &few_visits), &t.home), ["a", "b"]);
    }

    #[test]
    fn the_walk_stops_at_the_time_limit() {
        let t = tree();
        repo(&t.home.join("app"));
        let no_time = Limits {
            time: Duration::ZERO,
            ..LIMITS
        };
        assert!(find_home(&t, &no_time).is_empty());
    }

    #[test]
    fn a_repository_is_dated_by_its_newest_git_file() {
        let t = tree();
        let app = t.home.join("app");
        repo(&app);
        fs::write(app.join(".git/index"), "x").unwrap();
        let newest = 4_000_000_000;
        let later = UNIX_EPOCH + Duration::from_secs(newest.into());
        let index = fs::File::options().write(true).open(app.join(".git/index"));
        index.unwrap().set_modified(later).unwrap();

        let found = find_home(&t, &LIMITS);

        assert_eq!(found[0].updated, newest);
    }

    #[test]
    fn a_worktree_with_a_git_file_is_a_repository() {
        let t = tree();
        fs::create_dir_all(t.home.join("wt")).unwrap();
        fs::write(t.home.join("wt/.git"), "gitdir: /elsewhere\n").unwrap();
        let found = find_home(&t, &LIMITS);
        assert_eq!(names(&found, &t.home), ["wt"]);
        assert!(found[0].updated > 0);
    }
}
