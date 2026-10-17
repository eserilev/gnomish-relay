//! Roots that no agent ever gets: the whole disk, the home folder, a folder above it, and
//! a folder that holds the config folder or the data folder (SPEC.md 12). While a root is missing, an
//! agent can put a link in its place, so every start checks the real path of each root.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::config::with_tilde;
use crate::folder_path::{is_inside_folder, path_bytes, real_path};
use crate::folder_trust::holds_private_files;
use crate::folder_walk::Walk;
use crate::gate::Places;
use crate::roots::Roots;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WideRoot {
    WholeDisk,
    Home,
    AboveHome,
    Settings,
    Data,
    /// A folder that the classifier keeps private, as for a click (SPEC.md 9.12).
    Private,
}

impl WideRoot {
    pub fn text(self) -> &'static str {
        match self {
            WideRoot::WholeDisk => "the whole disk",
            WideRoot::Home => "your whole home folder",
            WideRoot::AboveHome => "a folder that holds your home folder",
            WideRoot::Settings => "a folder with the settings of the desktop app",
            WideRoot::Data => "a folder with the data of the desktop app",
            WideRoot::Private => "a folder with private files",
        }
    }
}

/// Whether the entry of the config leads to another folder. Only then is it "now" wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Plain,
    Link,
}

impl Entry {
    /// Only the entry itself counts. A link in a parent folder, such as `/var` on macOS,
    /// or a short path on Windows, makes no entry a link. On Windows, std counts a
    /// junction as a link too.
    pub fn of(root: &Path) -> Entry {
        let meta = std::fs::symlink_metadata(root);
        if meta.is_ok_and(|m| m.file_type().is_symlink()) {
            return Entry::Link;
        }
        Entry::Plain
    }

    pub fn is(self) -> &'static str {
        match self {
            Entry::Plain => "is",
            Entry::Link => "is now",
        }
    }
}

/// `root` is a real path. A folder that does not resolve stays as it is.
pub fn wide_root(root_path: &Path, places: &Places) -> Option<WideRoot> {
    if root_path.parent().is_none() {
        return Some(WideRoot::WholeDisk);
    }
    let root = path_bytes(root_path);
    let real = |dir: &Path| path_bytes(&real_path(dir).unwrap_or_else(|_| dir.to_owned()));
    let home = real(places.home);
    if home == root {
        return Some(WideRoot::Home);
    }
    if is_inside_folder(&home, &root) {
        return Some(WideRoot::AboveHome);
    }
    if is_inside_folder(&real(places.config_dir), &root) {
        return Some(WideRoot::Settings);
    }
    if is_inside_folder(&real(places.data_dir), &root) {
        return Some(WideRoot::Data);
    }
    let in_home = is_inside_folder(&root, &home);
    (in_home && holds_private(root_path, places)).then_some(WideRoot::Private)
}

/// Only the private rule of a click (SPEC.md 9.12). The name rule stays out: cloud
/// folders on macOS live in `~/Library/CloudStorage`, and many roots are dot folders.
fn holds_private(root: &Path, places: &Places) -> bool {
    let real = |dir: &Path| real_path(dir).unwrap_or_else(|_| dir.to_owned());
    let walk = Walk {
        roots: Roots::default(),
        deny: vec![real(places.config_dir), real(places.data_dir)],
        home: Some(real(places.home)),
    };
    holds_private_files(&walk, &real(places.home), root)
}

/// The line of the log for a missing root that comes back as a wide one.
pub fn wide_root_line(root: &Path, wide: WideRoot, home: &Path) -> String {
    format!(
        "{} {} {}, so agents can't work there.",
        with_tilde(root, home),
        Entry::of(root).is(),
        wide.text()
    )
}

/// `roots` are the roots of the config as it names them, so the line names the entry
/// and not the folder that a link leads to.
pub fn refuse_wide_roots(roots: &[PathBuf], places: &Places) -> Result<()> {
    for root in roots {
        let Ok(real) = real_path(root) else {
            continue;
        };
        if let Some(wide) = wide_root(&real, places) {
            bail!(
                "allowed_roots has {}, which {} {}. Agents can't work there. Run \
                 gnomish-relay setup to remove it.",
                with_tilde(root, places.home),
                Entry::of(root).is(),
                wide.text()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Computer {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        config: PathBuf,
        data: PathBuf,
    }

    fn computer() -> Computer {
        let tmp = tempfile::tempdir().unwrap();
        let home = real_path(tmp.path()).unwrap();
        let config = home.join(".config/gnomish-relay");
        let data = home.join(".local/share/gnomish-relay");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(home.join("Code")).unwrap();
        Computer {
            _tmp: tmp,
            home,
            config,
            data,
        }
    }

    impl Computer {
        fn places(&self) -> Places<'_> {
            Places {
                config_dir: &self.config,
                data_dir: &self.data,
                home: &self.home,
            }
        }

        fn wide(&self, root: &Path) -> Option<WideRoot> {
            wide_root(root, &self.places())
        }
    }

    #[test]
    fn the_whole_disk_the_home_folder_and_the_folders_above_the_app_folders_are_wide() {
        let c = computer();

        assert_eq!(c.wide(Path::new("/")), Some(WideRoot::WholeDisk));
        assert_eq!(c.wide(&c.home), Some(WideRoot::Home));
        assert_eq!(c.wide(&c.home.join(".config")), Some(WideRoot::Settings));
        assert_eq!(c.wide(&c.config), Some(WideRoot::Settings));
        assert_eq!(c.wide(&c.home.join(".local")), Some(WideRoot::Data));
    }

    #[test]
    fn a_folder_above_the_home_folder_is_wide() {
        let c = computer();

        let wide = c.wide(c.home.parent().unwrap());

        assert_eq!(wide, Some(WideRoot::AboveHome));
    }

    /// Cloud folders on macOS link into `~/Library/CloudStorage`.
    #[cfg(unix)]
    #[test]
    fn a_root_that_links_into_a_system_or_dot_folder_starts() {
        let c = computer();
        let cloud = c.home.join("Library/CloudStorage/Dropbox");
        std::fs::create_dir_all(&cloud).unwrap();
        std::fs::create_dir_all(c.home.join(".dotfiles")).unwrap();
        std::os::unix::fs::symlink(&cloud, c.home.join("Dropbox")).unwrap();
        let roots = [c.home.join("Dropbox"), c.home.join(".dotfiles")];

        assert!(refuse_wide_roots(&roots, &c.places()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_root_that_links_to_the_ssh_folder_is_refused_as_private() {
        let c = computer();
        std::fs::create_dir_all(c.home.join(".ssh")).unwrap();
        std::os::unix::fs::symlink(c.home.join(".ssh"), c.home.join("keys")).unwrap();

        let error = refuse_wide_roots(&[c.home.join("keys")], &c.places()).unwrap_err();

        assert_eq!(c.wide(&c.home.join(".ssh")), Some(WideRoot::Private));
        assert!(error.to_string().contains("private files"), "{error}");
    }

    /// A link in a parent folder, such as `/var` on macOS, does not make the entry a link.
    #[cfg(unix)]
    #[test]
    fn only_an_entry_that_is_a_link_itself_is_a_link() {
        let c = computer();
        std::fs::create_dir_all(c.home.join("real/app")).unwrap();
        std::os::unix::fs::symlink(c.home.join("real"), c.home.join("parent")).unwrap();
        std::os::unix::fs::symlink(c.home.join("real/app"), c.home.join("app")).unwrap();

        assert_eq!(Entry::of(&c.home.join("parent/app")), Entry::Plain);
        assert_eq!(Entry::of(&c.home.join("app")), Entry::Link);
        assert_eq!(Entry::of(&c.home.join("real/app")), Entry::Plain);
    }

    #[test]
    fn a_project_folder_is_no_wide_root() {
        let c = computer();

        assert_eq!(c.wide(&c.home.join("Code")), None);
        std::fs::create_dir_all(c.home.join("Code/app")).unwrap();
        assert_eq!(c.wide(&c.home.join("Code/app")), None);
    }

    #[test]
    fn a_config_with_the_home_folder_as_a_root_is_refused_and_names_it() {
        let c = computer();
        let roots = [c.home.join("Code"), c.home.clone()];

        let error = refuse_wide_roots(&roots, &c.places()).unwrap_err();

        assert!(
            error
                .to_string()
                .starts_with("allowed_roots has ~, which is your whole home folder."),
            "{error}"
        );
    }

    #[test]
    fn a_config_with_project_roots_is_fine() {
        let c = computer();

        assert!(refuse_wide_roots(&[c.home.join("Code")], &c.places()).is_ok());
    }
}
