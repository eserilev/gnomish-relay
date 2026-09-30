//! The folders inside `allowed_roots`, for the folder browser (SPEC.md 9.9).
//! A bounded walk: a root can be a whole home folder.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use protocol::action::{ToolCall, Verdict, classify};

use crate::action_input::{self, resolved_bytes};
use crate::roots::Roots;

/// Folders that hold tools, builds, or packages, never a project of the user.
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

/// One folder of the walk, in breadth-first order.
#[derive(Debug, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    /// The index of the parent folder in the walk. A root has none.
    pub parent: Option<usize>,
    pub repo: bool,
}

/// What the walk found. `complete` is false when a limit of visits or time stopped it.
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub folders: Vec<Folder>,
    pub complete: bool,
    pub home: Option<PathBuf>,
}

/// Where the walk may look, and what it never shows. All paths are resolved.
#[derive(Clone)]
pub struct Walk {
    pub roots: Roots,
    /// The config folder and the data folder of the bridge.
    pub deny: Vec<PathBuf>,
    /// The browser shows a path in the home folder as `~/...`.
    pub home: Option<PathBuf>,
}

pub fn is_skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED.contains(&name)
}

/// A worktree has a `.git` file. The walk never reads it: its line can lead out of the roots.
pub fn is_repo(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// The classifier gives the answer of an agent that reads the folder. Only a folder
/// that it reads with no question shows: never a credential or a bridge folder.
pub fn is_shown(walk: &Walk, dir: &Path) -> bool {
    let policy = action_input::policy(&walk.roots.list(), dir, &walk.deny, &[]);
    let read = ToolCall::Files {
        reads: vec![resolved_bytes(dir)],
        writes: Vec::new(),
    };
    classify(&read, &policy, &[]) == Verdict::Allow
}

/// The subfolders of `dir`. A link is never a subfolder, so the walk stays in the roots.
pub fn subfolders(dir: &Path) -> Vec<PathBuf> {
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

/// Shallow folders first, so a cap cuts off the deepest ones. A folder that does not
/// show hides its subfolders too.
pub fn walk_folders(walk: &Walk, limits: &Limits) -> Snapshot {
    let deadline = Instant::now() + limits.time;
    let mut queue: VecDeque<(PathBuf, usize, Option<usize>)> = walk
        .roots
        .list()
        .into_iter()
        .map(|r| (r, 0, None))
        .collect();
    let mut seen = BTreeSet::new();
    let mut folders = Vec::new();
    let mut complete = true;
    while let Some((dir, depth, parent)) = queue.pop_front() {
        if seen.len() >= limits.visits || Instant::now() >= deadline {
            complete = false;
            break;
        }
        if !seen.insert(dir.clone()) || !is_shown(walk, &dir) {
            continue;
        }
        let index = folders.len();
        if depth < limits.depth {
            let below = subfolders(&dir).into_iter();
            queue.extend(below.map(|d| (d, depth + 1, Some(index))));
        }
        let repo = is_repo(&dir);
        folders.push(Folder {
            path: dir,
            parent,
            repo,
        });
    }
    Snapshot {
        folders,
        complete,
        home: walk.home.clone(),
    }
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

    fn folder(at: &Path) {
        fs::create_dir_all(at).unwrap();
    }

    fn names(found: &Snapshot, home: &Path) -> Vec<String> {
        found
            .folders
            .iter()
            .map(|f| {
                let rel = f.path.strip_prefix(home).unwrap();
                rel.to_string_lossy().replace('\\', "/")
            })
            .collect()
    }

    fn find(roots: &[PathBuf], deny: &[PathBuf], limits: &Limits) -> Snapshot {
        let walk = Walk {
            roots: Roots::new(roots.to_vec()),
            deny: deny.to_vec(),
            home: None,
        };
        walk_folders(&walk, limits)
    }

    fn find_in(root: &Path, limits: &Limits) -> Snapshot {
        find(&[root.to_path_buf()], &[], limits)
    }

    #[test]
    fn the_walk_lists_every_folder_breadth_first_down_to_the_depth_limit() {
        let t = tree();
        folder(&t.home.join("Code/b/c/d/e/deeper"));
        folder(&t.home.join("Code/a"));

        let found = find_in(&t.home.join("Code"), &LIMITS);

        assert_eq!(
            names(&found, &t.home),
            [
                "Code",
                "Code/a",
                "Code/b",
                "Code/b/c",
                "Code/b/c/d",
                "Code/b/c/d/e"
            ]
        );
        assert!(found.complete, "the depth limit is not a cut");
    }

    #[test]
    fn each_folder_names_its_parent_and_a_root_has_none() {
        let t = tree();
        folder(&t.home.join("a/b"));

        let found = find_in(&t.home, &LIMITS);

        let parents: Vec<Option<usize>> = found.folders.iter().map(|f| f.parent).collect();
        assert_eq!(parents, [None, Some(0), Some(1)]);
    }

    #[test]
    fn a_repository_is_marked_and_the_walk_goes_into_it() {
        let t = tree();
        repo(&t.home.join("app"));
        folder(&t.home.join("app/src"));

        let found = find_in(&t.home, &LIMITS);

        assert_eq!(names(&found, &t.home), ["", "app", "app/src"]);
        let repos: Vec<bool> = found.folders.iter().map(|f| f.repo).collect();
        assert_eq!(repos, [false, true, false]);
    }

    #[test]
    fn a_worktree_with_a_git_file_is_a_repository() {
        let t = tree();
        folder(&t.home.join("wt"));
        fs::write(t.home.join("wt/.git"), "gitdir: /elsewhere\n").unwrap();
        let found = find_in(&t.home, &LIMITS);
        assert!(found.folders[1].repo);
    }

    #[test]
    fn hidden_and_build_folders_are_skipped() {
        let t = tree();
        for name in [".hidden/app", "node_modules/pkg", "target/app", "real"] {
            folder(&t.home.join(name));
        }
        let found = find_in(&t.home, &LIMITS);
        assert_eq!(names(&found, &t.home), ["", "real"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_never_followed() {
        let t = tree();
        let root = t.home.join("Code");
        folder(&root);
        folder(&t.home.join("outside/secret"));
        std::os::unix::fs::symlink(t.home.join("outside"), root.join("link")).unwrap();

        assert_eq!(names(&find_in(&root, &LIMITS), &t.home), ["Code"]);
    }

    /// A junction is a link to a folder that needs no rights to make.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_never_followed() {
        let t = tree();
        let root = t.home.join("Code");
        folder(&root);
        folder(&t.home.join("outside/secret"));
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(t.home.join("outside"))
            .status()
            .unwrap();
        assert!(made.success());

        assert_eq!(names(&find_in(&root, &LIMITS), &t.home), ["Code"]);
    }

    #[test]
    fn a_folder_outside_the_roots_is_never_listed() {
        let t = tree();
        folder(&t.home.join("Code/app"));
        folder(&t.home.join("Other/app"));
        let found = find_in(&t.home.join("Code"), &LIMITS);
        assert_eq!(names(&found, &t.home), ["Code", "Code/app"]);
    }

    #[test]
    fn a_deny_folder_and_a_credential_folder_and_their_subfolders_are_never_listed() {
        let t = tree();
        folder(&t.home.join("config/gnomish-relay/inner"));
        folder(&t.home.join("snap/firefox/profile"));
        let deny = [t.home.join("config/gnomish-relay")];

        let found = find(std::slice::from_ref(&t.home), &deny, &LIMITS);

        assert_eq!(names(&found, &t.home), ["", "config", "snap"]);
    }

    #[test]
    fn overlapping_roots_list_a_folder_once() {
        let t = tree();
        folder(&t.home.join("Code/app"));
        let roots = [t.home.clone(), t.home.join("Code")];
        let found = find(&roots, &[], &LIMITS);
        assert_eq!(names(&found, &t.home), ["", "Code", "Code/app"]);
        assert_eq!(found.folders[1].parent, None, "a root stays a root");
    }

    #[test]
    fn the_walk_stops_at_the_visit_cap_and_says_so() {
        let t = tree();
        for name in ["a", "b", "c"] {
            folder(&t.home.join(name));
        }
        let few_visits = Limits {
            visits: 3,
            ..LIMITS
        };

        let found = find_in(&t.home, &few_visits);

        assert_eq!(names(&found, &t.home), ["", "a", "b"]);
        assert!(!found.complete);
    }

    #[test]
    fn the_walk_stops_at_the_time_limit_and_says_so() {
        let t = tree();
        let no_time = Limits {
            time: Duration::ZERO,
            ..LIMITS
        };
        let found = find_in(&t.home, &no_time);
        assert!(found.folders.is_empty());
        assert!(!found.complete);
    }
}
