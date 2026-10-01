//! The rules of a new folder: a folder from the game under no root, which a click on
//! the desktop can add to the roots (SPEC.md 9.12).

use std::path::Path;

use crate::folder_path::{is_inside_folder, path_bytes, path_parts, real_path};
use crate::folder_walk::{Walk, is_shown, is_skipped};
use crate::roots::Roots;

/// Why a folder can never become a root. The game gets the text, and no dialog shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Untrusted {
    Home,
    OutsideHome,
    /// A hidden folder, or one that the walk skips, such as `node_modules` or `Library`.
    Hidden,
    /// A `deny` or `desktop` path of the classifier, or a folder that holds a `deny` folder.
    Private,
}

impl Untrusted {
    pub fn text(self) -> &'static str {
        match self {
            Untrusted::Home => {
                "Agents can't work in your whole home folder. Pick a project folder inside it."
            }
            Untrusted::OutsideHome => {
                "That folder is outside your home folder. Pick a folder inside it."
            }
            Untrusted::Hidden => {
                "Agents can't work in hidden or system folders. Pick another folder."
            }
            Untrusted::Private => {
                "Agents can't work in that folder: it holds private files. Pick another folder."
            }
        }
    }
}

/// Rules 1 and 2 of SPEC.md 9.12, on the text alone. Both paths are resolved.
pub fn check_text(home: &[u8], folder: &[u8]) -> Result<(), Untrusted> {
    if !is_inside_folder(folder, home) {
        return Err(Untrusted::OutsideHome);
    }
    let parts = path_parts(folder);
    let below = &parts[path_parts(home).len()..];
    if below.is_empty() {
        return Err(Untrusted::Home);
    }
    if below
        .iter()
        .any(|part| is_skipped(&String::from_utf8_lossy(part)))
    {
        return Err(Untrusted::Hidden);
    }
    Ok(())
}

/// All four rules, on the real path. `home` is resolved. `walk` gives the `deny` folders.
pub fn check_real(walk: &Walk, home: &Path, real: &Path) -> Result<(), Untrusted> {
    check_text(&path_bytes(home), &path_bytes(real))?;
    let in_home = Walk {
        roots: Roots::new(vec![home.to_owned()]),
        deny: walk.deny.clone(),
        home: Some(home.to_owned()),
    };
    let real_bytes = path_bytes(real);
    let holds_deny = walk
        .deny
        .iter()
        .any(|deny| is_inside_folder(&path_bytes(deny), &real_bytes));
    if holds_deny || !is_shown(&in_home, real) {
        return Err(Untrusted::Private);
    }
    Ok(())
}

/// True when Resume shows a session of `folder`: the folder exists, and it is in a root
/// or can become one. With no `home`, only a root counts (SPEC.md 9.6).
pub fn may_list(walk: &Walk, home: Option<&Path>, folder: &Path) -> bool {
    let Ok(real) = real_path(folder) else {
        return false;
    };
    if !real.is_dir() {
        return false;
    }
    if walk.roots.hold(&real) {
        return true;
    }
    home.is_some_and(|home| check_real(walk, home, &real).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn a_project_folder_in_the_home_folder_passes_the_text_rules() {
        assert_eq!(check_text(b"/home/x", b"/home/x/lighthouse"), Ok(()));
        assert_eq!(check_text(b"/home/x", b"/home/x/Documents/app"), Ok(()));
    }

    #[test]
    fn the_home_folder_and_a_folder_above_or_beside_it_fail_the_text_rules() {
        assert_eq!(check_text(b"/home/x", b"/home/x"), Err(Untrusted::Home));
        assert_eq!(
            check_text(b"/home/x", b"/home"),
            Err(Untrusted::OutsideHome)
        );
        assert_eq!(check_text(b"/home/x", b"/"), Err(Untrusted::OutsideHome));
        assert_eq!(
            check_text(b"/home/x", b"/home/xy/app"),
            Err(Untrusted::OutsideHome)
        );
        assert_eq!(check_text(b"/home/x", b"/etc"), Err(Untrusted::OutsideHome));
    }

    #[test]
    fn a_hidden_or_skipped_part_below_the_home_folder_fails_the_text_rules() {
        for folder in [
            "/home/x/.ssh",
            "/home/x/.config/app",
            "/home/x/app/node_modules",
            "/home/x/Library/Keychains",
        ] {
            assert_eq!(
                check_text(b"/home/x", folder.as_bytes()),
                Err(Untrusted::Hidden),
                "{folder}"
            );
        }
    }

    #[test]
    fn a_hidden_home_folder_is_no_reason_to_refuse() {
        assert_eq!(check_text(b"/var/.home/x", b"/var/.home/x/app"), Ok(()));
    }

    struct Place {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        walk: Walk,
    }

    fn place() -> Place {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let walk = Walk {
            roots: Roots::new(vec![home.join("Code")]),
            deny: vec![home.join("app-data/gnomish-relay")],
            home: Some(home.clone()),
        };
        Place {
            _tmp: tmp,
            home,
            walk,
        }
    }

    #[test]
    fn a_real_project_folder_passes_every_rule() {
        let h = place();
        fs::create_dir_all(h.home.join("lighthouse")).unwrap();

        assert_eq!(
            check_real(&h.walk, &h.home, &h.home.join("lighthouse")),
            Ok(())
        );
    }

    #[test]
    fn a_folder_of_the_bridge_or_a_folder_that_holds_it_is_private() {
        let h = place();
        fs::create_dir_all(h.home.join("app-data/gnomish-relay/approvals")).unwrap();

        for folder in [
            "app-data",
            "app-data/gnomish-relay",
            "app-data/gnomish-relay/approvals",
        ] {
            assert_eq!(
                check_real(&h.walk, &h.home, &h.home.join(folder)),
                Err(Untrusted::Private),
                "{folder}"
            );
        }
    }

    #[test]
    fn a_credential_folder_of_the_classifier_is_private() {
        let h = place();
        fs::create_dir_all(h.home.join("snap/firefox")).unwrap();

        assert_eq!(
            check_real(&h.walk, &h.home, &h.home.join("snap/firefox")),
            Err(Untrusted::Private)
        );
    }

    #[test]
    fn resume_lists_a_folder_of_a_root_or_a_new_folder_that_exists() {
        let h = place();
        fs::create_dir_all(h.home.join("Code/app")).unwrap();
        fs::create_dir_all(h.home.join("lighthouse")).unwrap();

        assert!(may_list(&h.walk, Some(&h.home), &h.home.join("Code/app")));
        assert!(may_list(&h.walk, Some(&h.home), &h.home.join("lighthouse")));
    }

    #[test]
    fn resume_never_lists_a_gone_folder_or_with_no_home_a_new_folder() {
        let h = place();
        fs::create_dir_all(h.home.join("lighthouse")).unwrap();

        assert!(!may_list(&h.walk, Some(&h.home), &h.home.join("Code/gone")));
        assert!(!may_list(&h.walk, None, &h.home.join("lighthouse")));
    }

    #[test]
    fn resume_never_lists_a_private_folder() {
        let h = place();
        fs::create_dir_all(h.home.join("snap/firefox")).unwrap();
        fs::create_dir_all(h.home.join("app-data/gnomish-relay")).unwrap();

        for folder in ["snap/firefox", "app-data/gnomish-relay", ""] {
            assert!(
                !may_list(&h.walk, Some(&h.home), &h.home.join(folder)),
                "{folder}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resume_never_lists_a_link_that_leads_out_of_the_home_folder() {
        let h = place();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), h.home.join("out")).unwrap();

        assert!(!may_list(&h.walk, Some(&h.home), &h.home.join("out")));
    }
}
