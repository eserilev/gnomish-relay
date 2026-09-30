//! The folder of a chat at the start of a run: a new one is made (SPEC.md 9.9), and each
//! one is checked again with its links resolved (6.2, rule 10).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::folder_path::{is_inside_folder, path_bytes};
use crate::folder_walk::{Walk, is_shown};

/// The longest file name on the file systems that the bridge runs on.
const MAX_NAME: usize = 255;
const MISSING: &str = "This chat's folder is gone. Pick another folder.";
const OUTSIDE_ROOTS: &str =
    "That folder isn't allowed: it links to a place outside allowed_roots in config.toml.";

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
            NewFolderError::BadName => "Couldn't create the folder: the name can't contain / or \\. Pick another name.".into(),
            NewFolderError::NoParent => "Couldn't create the folder: the folder it goes in is gone. Pick one that exists.".into(),
            NewFolderError::OutsideRoots => "Couldn't create the folder: it's outside allowed_roots in config.toml. Pick a folder inside them.".into(),
            NewFolderError::NotAllowed => "Couldn't create a folder there. Pick another folder.".into(),
            NewFolderError::NotAFolder => "Couldn't create the folder: a file already has that name. Pick another name.".into(),
            NewFolderError::Failed(e) => format!("Couldn't create the folder: {e}"),
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

/// The real path of the folder of a chat, inside a root. The relay checks only the text
/// of the folder, and a link in it can leave every root.
pub fn real_chat_folder(walk: &Walk, folder: &Path) -> Result<String, String> {
    let real = folder.canonicalize().map_err(|_| MISSING.to_owned())?;
    let bytes = path_bytes(&real);
    let inside = |root: &PathBuf| is_inside_folder(&bytes, &path_bytes(root));
    if !walk.roots.iter().any(inside) {
        return Err(OUTSIDE_ROOTS.into());
    }
    let text = real.to_str().ok_or_else(|| MISSING.to_owned())?;
    Ok(without_verbatim(text).to_owned())
}

/// `canonicalize` on Windows starts a path with `\\?\`, which many programs refuse.
/// A network path keeps it, because it has no other form with the same meaning.
fn without_verbatim(path: &str) -> &str {
    match path.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest,
        _ => path,
    }
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
    fn the_real_folder_of_a_chat_inside_a_root_is_its_canonical_path() {
        let h = home();
        fs::create_dir_all(h.root.join("app/src")).unwrap();

        let real = real_chat_folder(&walk(&h), &h.root.join("app/src/.."));

        assert_eq!(real, Ok(h.root.join("app").to_string_lossy().into_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_chat_folder_that_is_a_link_out_of_every_root_is_refused() {
        let h = home();
        let outside = h.root.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, h.root.join("app")).unwrap();

        let real = real_chat_folder(&walk(&h), &h.root.join("app"));

        assert_eq!(real, Err(OUTSIDE_ROOTS.into()));
    }

    #[cfg(unix)]
    #[test]
    fn a_chat_folder_that_is_a_link_to_another_folder_in_a_root_runs_there() {
        let h = home();
        fs::create_dir(h.root.join("real")).unwrap();
        std::os::unix::fs::symlink(h.root.join("real"), h.root.join("app")).unwrap();

        let real = real_chat_folder(&walk(&h), &h.root.join("app"));

        assert_eq!(real, Ok(h.root.join("real").to_string_lossy().into_owned()));
    }

    #[test]
    fn a_missing_chat_folder_is_refused() {
        let h = home();

        let real = real_chat_folder(&walk(&h), &h.root.join("gone"));

        assert_eq!(real, Err(MISSING.into()));
    }

    #[test]
    fn a_verbatim_prefix_goes_only_before_a_drive() {
        assert_eq!(without_verbatim(r"\\?\C:\Code\app"), r"C:\Code\app");
        assert_eq!(
            without_verbatim(r"\\?\UNC\server\share"),
            r"\\?\UNC\server\share"
        );
        assert_eq!(without_verbatim("/home/x/Code"), "/home/x/Code");
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
        let made = make_folder(&walk(&h), &h.root.join("a\u{7}b"));
        assert_eq!(made, Err(NewFolderError::BadName));
        // On Windows, `join("..")` on a canonical `\\?\` path goes up by itself, so the
        // function gets the parent of the root, which is outside the roots.
        let up = make_folder(&walk(&h), &h.root.join(".."));
        assert!(
            matches!(
                up,
                Err(NewFolderError::BadName | NewFolderError::OutsideRoots)
            ),
            "{up:?}"
        );
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
        assert!(texts.iter().all(|t| t.starts_with("Couldn't create")));
    }

    #[test]
    fn each_refusal_of_the_bridge_says_what_to_do_next() {
        let refusals = [
            NewFolderError::BadName,
            NewFolderError::NoParent,
            NewFolderError::OutsideRoots,
            NewFolderError::NotAllowed,
            NewFolderError::NotAFolder,
        ];
        for refusal in refusals {
            let text = refusal.text();
            assert!(text.contains(" Pick ") || text.contains(" Use "), "{text}");
        }
    }
}
