//! The folders of the home folder around the roots, for the folder browser (SPEC.md
//! 9.9 and 9.12). A folder there under no root needs a click on the desktop first.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::folder_walk::{
    Folder, LIMITS, Limits, Snapshot, Walk, is_repo, is_shown, subfolders, walk_folders,
};
use crate::roots::Roots;

/// Fewer than for the roots: the home folder comes second, and it holds more.
pub const HOME_LIMITS: Limits = Limits {
    depth: 3,
    visits: 1000,
    time: Duration::from_secs(1),
};

/// The roots first, then the home folder. With no `home`, only the roots.
pub fn browse_folders(walk: &Walk, home: Option<&Path>) -> Snapshot {
    let roots = walk_folders(walk, &LIMITS);
    let Some(home) = home else {
        return roots;
    };
    let around = walk_home(walk, home, &HOME_LIMITS);
    join(roots, &around)
}

/// True when `dir` is above a root, so the walk goes down to the root below any depth.
fn leads_to_root(dir: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| r != dir && r.starts_with(dir))
}

/// Breadth first from `home`, with the rules of the walk of the roots. It never goes
/// into a root: the walk of the roots has it.
pub fn walk_home(walk: &Walk, home: &Path, limits: &Limits) -> Snapshot {
    let in_home = Walk {
        roots: Roots::new(vec![home.to_owned()]),
        deny: walk.deny.clone(),
        home: walk.home.clone(),
    };
    let roots = walk.roots.list();
    let deadline = Instant::now() + limits.time;
    let mut queue: VecDeque<(PathBuf, usize, Option<usize>)> =
        VecDeque::from([(home.to_owned(), 0, None)]);
    let mut visits = 0;
    let mut folders = Vec::new();
    let mut complete = true;
    while let Some((dir, depth, parent)) = queue.pop_front() {
        if visits >= limits.visits || Instant::now() >= deadline {
            complete = false;
            break;
        }
        visits += 1;
        if !is_shown(&in_home, &dir) {
            continue;
        }
        let index = folders.len();
        for below in subfolders(&dir) {
            let in_a_root = roots.iter().any(|r| below.starts_with(r));
            let wanted = depth < limits.depth || leads_to_root(&below, &roots);
            if wanted && !in_a_root {
                queue.push_back((below, depth + 1, Some(index)));
            }
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

#[derive(Clone, Copy)]
enum From {
    Home(usize),
    Roots(usize),
}

/// The order of the reply: the folders on the way down to the roots, the roots, then
/// the rest of the home folder. So a size cut takes the rest of the home folder first,
/// and each folder still comes after its parent.
fn join(roots: Snapshot, home: &Snapshot) -> Snapshot {
    let tops: Vec<PathBuf> = roots
        .folders
        .iter()
        .filter(|f| f.parent.is_none())
        .map(|f| f.path.clone())
        .collect();
    let on_the_way = |f: &Folder| leads_to_root(&f.path, &tops);
    let mut order: Vec<From> = Vec::new();
    order.extend(
        (0..home.folders.len())
            .filter(|&i| on_the_way(&home.folders[i]))
            .map(From::Home),
    );
    order.extend((0..roots.folders.len()).map(From::Roots));
    order.extend(
        (0..home.folders.len())
            .filter(|&i| !on_the_way(&home.folders[i]))
            .map(From::Home),
    );
    let mut home_at = vec![None; home.folders.len()];
    let mut roots_at = vec![None; roots.folders.len()];
    for (at, from) in order.iter().enumerate() {
        match *from {
            From::Home(i) => home_at[i] = Some(at),
            From::Roots(i) => roots_at[i] = Some(at),
        }
    }
    let parent_in_home = |path: &Path| {
        let parent = path.parent()?;
        let i = home.folders.iter().position(|f| f.path == parent)?;
        home_at[i]
    };
    let folders = order
        .iter()
        .map(|from| match *from {
            From::Home(i) => {
                let f = &home.folders[i];
                Folder {
                    path: f.path.clone(),
                    parent: f.parent.and_then(|p| home_at[p]),
                    repo: f.repo,
                }
            }
            From::Roots(i) => {
                let f = &roots.folders[i];
                let parent = match f.parent {
                    Some(p) => roots_at[p],
                    None => parent_in_home(&f.path),
                };
                Folder {
                    path: f.path.clone(),
                    parent,
                    repo: f.repo,
                }
            }
        })
        .collect();
    Snapshot {
        folders,
        complete: roots.complete && home.complete,
        home: roots.home,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Tree {
        _tmp: tempfile::TempDir,
        home: PathBuf,
    }

    fn tree() -> Tree {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        Tree { _tmp: tmp, home }
    }

    fn walk(t: &Tree, roots: &[&str], deny: &[&str]) -> Walk {
        Walk {
            roots: Roots::new(roots.iter().map(|r| t.home.join(r)).collect()),
            deny: deny.iter().map(|d| t.home.join(d)).collect(),
            home: Some(t.home.clone()),
        }
    }

    /// Each folder as its path from home, and its parent as a path too.
    fn lines(found: &Snapshot, home: &Path) -> Vec<(String, String)> {
        let rel = |p: &Path| {
            p.strip_prefix(home)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        };
        found
            .folders
            .iter()
            .map(|f| {
                let parent = f.parent.map_or("-".into(), |p| rel(&found.folders[p].path));
                (rel(&f.path), parent)
            })
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    #[test]
    fn the_roots_hang_in_the_home_tree_and_the_rest_of_home_comes_last() {
        let t = tree();
        for dir in ["Documents/Code/app", "Documents/taxes", "music"] {
            fs::create_dir_all(t.home.join(dir)).unwrap();
        }

        let found = browse_folders(&walk(&t, &["Documents/Code"], &[]), Some(&t.home));

        assert_eq!(
            lines(&found, &t.home),
            pairs(&[
                ("", "-"),
                ("Documents", ""),
                ("Documents/Code", "Documents"),
                ("Documents/Code/app", "Documents/Code"),
                ("music", ""),
                ("Documents/taxes", "Documents"),
            ])
        );
        assert!(found.complete);
    }

    #[test]
    fn each_folder_comes_after_its_parent() {
        let t = tree();
        for dir in ["a/b/c", "code/x/y", "z"] {
            fs::create_dir_all(t.home.join(dir)).unwrap();
        }

        let found = browse_folders(&walk(&t, &["code"], &[]), Some(&t.home));

        for (at, folder) in found.folders.iter().enumerate() {
            assert!(folder.parent.is_none_or(|p| p < at), "{:?}", folder.path);
        }
        assert_eq!(
            found.folders.iter().filter(|f| f.parent.is_none()).count(),
            1
        );
    }

    #[test]
    fn the_home_walk_goes_down_to_a_deep_root_but_not_to_other_deep_folders() {
        let t = tree();
        fs::create_dir_all(t.home.join("a/b/c/d/root/app")).unwrap();
        fs::create_dir_all(t.home.join("a/b/c/other/deeper")).unwrap();

        let found = browse_folders(&walk(&t, &["a/b/c/d/root"], &[]), Some(&t.home));

        let paths: Vec<String> = lines(&found, &t.home).into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"a/b/c/d/root/app".to_owned()), "{paths:?}");
        assert!(
            !paths.contains(&"a/b/c/other/deeper".to_owned()),
            "{paths:?}"
        );
    }

    #[test]
    fn the_home_walk_never_lists_hidden_or_private_folders() {
        let t = tree();
        for dir in [
            ".ssh",
            ".config/gnomish-relay",
            "snap/firefox",
            "data/gnomish-relay",
            "work",
        ] {
            fs::create_dir_all(t.home.join(dir)).unwrap();
        }

        let found = browse_folders(&walk(&t, &[], &["data/gnomish-relay"]), Some(&t.home));

        let paths: Vec<String> = lines(&found, &t.home).into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"work".to_owned()), "{paths:?}");
        for hidden in [".ssh", ".config", "snap/firefox", "data/gnomish-relay"] {
            assert!(!paths.contains(&hidden.to_owned()), "{hidden}: {paths:?}");
        }
    }

    #[test]
    fn with_no_home_folder_only_the_roots_show() {
        let t = tree();
        fs::create_dir_all(t.home.join("code/app")).unwrap();
        fs::create_dir_all(t.home.join("music")).unwrap();

        let found = browse_folders(&walk(&t, &["code"], &[]), None);

        let paths: Vec<String> = lines(&found, &t.home).into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, ["code", "code/app"]);
    }

    #[test]
    fn a_root_outside_the_home_folder_stays_a_top_of_its_own() {
        let t = tree();
        let outside = tempfile::tempdir().unwrap();
        let outside = outside.path().canonicalize().unwrap();
        fs::create_dir_all(t.home.join("work")).unwrap();
        let mut w = walk(&t, &[], &[]);
        w.roots = Roots::new(vec![outside.clone()]);

        let found = browse_folders(&w, Some(&t.home));

        let tops: Vec<&PathBuf> = found
            .folders
            .iter()
            .filter(|f| f.parent.is_none())
            .map(|f| &f.path)
            .collect();
        assert_eq!(tops, [&outside, &t.home]);
    }
}
