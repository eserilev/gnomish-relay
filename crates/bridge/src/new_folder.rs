//! The new folder of a chat, made before its first run (SPEC.md 9.9).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::config::{is_inside_folder, path_bytes};
use crate::folder_walk::{Walk, is_shown};

/// The longest file name on the file systems that the bridge runs on.
const MAX_NAME: usize = 255;

#[derive(Debug, PartialEq, Eq)]
pub enum NewFolderError {
    BadName,
    NoParent,
    OutsideRoots,
    NotAllowed,
    NotAFolder,
    Failed(String),
}

impl NewFolderError {
    pub fn text(&self) -> String {
        match self {
            NewFolderError::BadName => "Folder not made: bad name.".into(),
            NewFolderError::NoParent => "Folder not made: its parent is missing.".into(),
            NewFolderError::OutsideRoots => "Folder not made: not in the allowed roots.".into(),
            NewFolderError::NotAllowed => "Folder not made: not allowed.".into(),
            NewFolderError::NotAFolder => "Folder not made: a file has its name.".into(),
            NewFolderError::Failed(e) => format!("Folder not made: {e}"),
        }
    }
}

/// The same rules as the name check of the addon.
pub fn is_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

/// Makes the last part of `folder` with `create_dir`, never a parent. A folder that
/// is already there is fine: a run after a bridge restart asks again.
pub fn make_folder(walk: &Walk, folder: &Path) -> Result<(), NewFolderError> {
    let name = folder.file_name().and_then(|n| n.to_str());
    let Some(name) = name.filter(|n| is_folder_name(n)) else {
        return Err(NewFolderError::BadName);
    };
    let parent = folder.parent().ok_or(NewFolderError::NoParent)?;
    let real = parent
        .canonicalize()
        .map_err(|_| NewFolderError::NoParent)?;
    let real_bytes = path_bytes(&real);
    let inside = |root: &PathBuf| is_inside_folder(&real_bytes, &path_bytes(root));
    if !walk.roots.iter().any(inside) {
        return Err(NewFolderError::OutsideRoots);
    }
    let target = real.join(name);
    if !is_shown(walk, &target) {
        return Err(NewFolderError::NotAllowed);
    }
    match fs::create_dir(&target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => is_real_folder(&target),
        Err(e) => Err(NewFolderError::Failed(e.kind().to_string())),
    }
}

fn is_real_folder(path: &Path) -> Result<(), NewFolderError> {
    let meta = fs::symlink_metadata(path).map_err(|e| NewFolderError::Failed(e.to_string()))?;
    if meta.is_dir() {
        Ok(())
    } else {
        Err(NewFolderError::NotAFolder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Home {
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    fn home() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("Code");
        fs::create_dir_all(&root).unwrap();
        Home { _tmp: tmp, root }
    }

    fn walk(h: &Home) -> Walk {
        Walk {
            roots: vec![h.root.clone()],
            deny: vec![h.root.join("bridge-config")],
            home: None,
        }
    }

    #[test]
    fn the_last_part_is_made_inside_a_root() {
        let h = home();
        let made = make_folder(&walk(&h), &h.root.join("new"));
        assert_eq!(made, Ok(()));
        assert!(h.root.join("new").is_dir());
    }

    #[test]
    fn a_folder_that_is_there_already_is_fine() {
        let h = home();
        fs::create_dir(h.root.join("app")).unwrap();
        assert_eq!(make_folder(&walk(&h), &h.root.join("app")), Ok(()));
    }

    #[test]
    fn a_file_with_the_name_is_refused() {
        let h = home();
        fs::write(h.root.join("notes"), "x").unwrap();
        assert_eq!(
            make_folder(&walk(&h), &h.root.join("notes")),
            Err(NewFolderError::NotAFolder)
        );
    }

    #[test]
    fn a_missing_parent_is_refused_and_nothing_is_made() {
        let h = home();
        let made = make_folder(&walk(&h), &h.root.join("a/b/new"));
        assert_eq!(made, Err(NewFolderError::NoParent));
        assert!(!h.root.join("a").exists());
    }

    #[test]
    fn a_parent_outside_the_roots_is_refused() {
        let h = home();
        let outside = h.root.parent().unwrap().join("new");
        assert_eq!(
            make_folder(&walk(&h), &outside),
            Err(NewFolderError::OutsideRoots)
        );
        assert!(!outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_parent_that_links_out_of_the_roots_is_refused() {
        let h = home();
        let outside = h.root.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, h.root.join("link")).unwrap();

        let made = make_folder(&walk(&h), &h.root.join("link/new"));

        assert_eq!(made, Err(NewFolderError::OutsideRoots));
        assert!(!outside.join("new").exists());
    }

    #[test]
    fn a_folder_in_a_deny_folder_or_a_credential_folder_is_refused() {
        let h = home();
        fs::create_dir_all(h.root.join("bridge-config")).unwrap();
        fs::create_dir_all(h.root.join("snap")).unwrap();
        for folder in ["bridge-config/new", "snap/firefox"] {
            let made = make_folder(&walk(&h), &h.root.join(folder));
            assert_eq!(made, Err(NewFolderError::NotAllowed), "{folder}");
            assert!(!h.root.join(folder).exists());
        }
    }

    #[test]
    fn a_bad_name_is_refused() {
        let h = home();
        for name in ["..", "a\u{7}b"] {
            let made = make_folder(&walk(&h), &h.root.join(name));
            assert_eq!(made, Err(NewFolderError::BadName), "{name:?}");
        }
    }

    #[test]
    fn a_name_follows_the_rules_of_the_addon() {
        let longest = "a".repeat(MAX_NAME);
        for good in ["app", "my app", "\u{e9}t\u{e9}", longest.as_str()] {
            assert!(is_folder_name(good), "{good}");
        }
        let long = "a".repeat(MAX_NAME + 1);
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\tb",
            "a\u{85}b",
            long.as_str(),
        ] {
            assert!(!is_folder_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn each_refusal_has_its_own_text() {
        let all = [
            NewFolderError::BadName,
            NewFolderError::NoParent,
            NewFolderError::OutsideRoots,
            NewFolderError::NotAllowed,
            NewFolderError::NotAFolder,
            NewFolderError::Failed("denied".into()),
        ];
        let texts: std::collections::BTreeSet<String> =
            all.iter().map(NewFolderError::text).collect();
        assert_eq!(texts.len(), all.len());
        assert!(texts.iter().all(|t| t.starts_with("Folder not made")));
    }
}
