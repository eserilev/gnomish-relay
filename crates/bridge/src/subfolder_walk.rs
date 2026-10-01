//! The subfolders of one folder that the tree did not walk, for the folder browser
//! (SPEC.md 9.9, "One folder").

use std::path::Path;

use crate::folder_path::{path_bytes, real_path};
use crate::folder_trust::{Untrusted, check_text};
use crate::folder_walk::{LIMITS, Snapshot, Walk, is_skipped, walk_from};
use crate::roots::Roots;

/// The walk of the roots, from `folder`. A folder that fails a check gives no folders.
/// `home` is resolved.
pub fn list_below(walk: &Walk, home: Option<&Path>, folder: &Path) -> Snapshot {
    let found = real_path(folder)
        .ok()
        .and_then(|real| scope(walk, home, &real).map(|scope| (scope, real)));
    let Some((scope, real)) = found else {
        return Snapshot {
            folders: Vec::new(),
            complete: true,
            home: walk.home.clone(),
        };
    };
    walk_from(&scope, vec![real], &LIMITS)
}

/// The walk whose classifier rules `real`: the roots, or the home folder as the only
/// root, as in the home walk. `None` for a folder that the browser never shows.
fn scope(walk: &Walk, home: Option<&Path>, real: &Path) -> Option<Walk> {
    if let Some(root) = walk.roots.list().into_iter().find(|r| real.starts_with(r)) {
        return (!has_skipped_part(&root, real)).then(|| walk.clone());
    }
    let home = home?;
    match check_text(&path_bytes(home), &path_bytes(real)) {
        Ok(()) | Err(Untrusted::Home) => Some(Walk {
            roots: Roots::new(vec![home.to_owned()]),
            deny: walk.deny.clone(),
            home: walk.home.clone(),
        }),
        Err(_) => None,
    }
}

/// The walk never goes into a hidden or a build folder, so a listing never starts there.
fn has_skipped_part(root: &Path, real: &Path) -> bool {
    let Ok(below) = real.strip_prefix(root) else {
        return true;
    };
    below
        .components()
        .any(|part| is_skipped(&part.as_os_str().to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct Tree {
        _tmp: tempfile::TempDir,
        home: PathBuf,
    }

    fn tree() -> Tree {
        let tmp = tempfile::tempdir().unwrap();
        let home = real_path(tmp.path()).unwrap();
        Tree { _tmp: tmp, home }
    }

    fn walk(t: &Tree, roots: &[&str], deny: &[&str]) -> Walk {
        Walk {
            roots: Roots::new(roots.iter().map(|r| t.home.join(r)).collect()),
            deny: deny.iter().map(|d| t.home.join(d)).collect(),
            home: Some(t.home.clone()),
        }
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

    #[test]
    fn a_deep_folder_in_a_root_lists_its_real_subfolders() {
        let t = tree();
        fs::create_dir_all(t.home.join("Code/a/b/c/d/e/f")).unwrap();
        fs::create_dir_all(t.home.join("Code/a/b/c/d/e/node_modules")).unwrap();

        let found = list_below(
            &walk(&t, &["Code"], &[]),
            Some(&t.home),
            &t.home.join("Code/a/b/c/d/e"),
        );

        assert_eq!(
            names(&found, &t.home),
            ["Code/a/b/c/d/e", "Code/a/b/c/d/e/f"]
        );
        assert!(found.folders.iter().all(|f| f.walked));
    }

    #[test]
    fn a_folder_of_the_home_folder_under_no_root_lists_with_the_home_rules() {
        let t = tree();
        fs::create_dir_all(t.home.join("music/rock")).unwrap();
        fs::create_dir_all(t.home.join("music/private")).unwrap();

        let found = list_below(
            &walk(&t, &["Code"], &["music/private"]),
            Some(&t.home),
            &t.home.join("music"),
        );

        assert_eq!(names(&found, &t.home), ["music", "music/rock"]);
    }

    #[test]
    fn a_hidden_folder_a_folder_outside_and_a_missing_folder_list_nothing() {
        let t = tree();
        fs::create_dir_all(t.home.join("Code/.git/objects")).unwrap();
        fs::create_dir_all(t.home.join(".ssh/keys")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(outside.path().join("other")).unwrap();
        let w = walk(&t, &["Code"], &[]);

        for folder in [
            t.home.join("Code/.git"),
            t.home.join(".ssh"),
            outside.path().to_path_buf(),
            t.home.join("Code/missing"),
        ] {
            let found = list_below(&w, Some(&t.home), &folder);
            assert!(found.folders.is_empty(), "{folder:?}");
        }
    }

    #[test]
    fn with_no_home_folder_only_a_folder_in_a_root_lists() {
        let t = tree();
        fs::create_dir_all(t.home.join("music/rock")).unwrap();
        let found = list_below(&walk(&t, &["Code"], &[]), None, &t.home.join("music"));
        assert!(found.folders.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_roots_lists_nothing() {
        let t = tree();
        fs::create_dir_all(t.home.join("Code")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(outside.path().join("secret")).unwrap();
        std::os::unix::fs::symlink(outside.path(), t.home.join("Code/link")).unwrap();

        let found = list_below(
            &walk(&t, &["Code"], &[]),
            Some(&t.home),
            &t.home.join("Code/link"),
        );

        assert!(found.folders.is_empty());
    }
}
