//! Roots that no agent ever gets: the whole disk, the home folder, a folder above it, and
//! a folder that holds the config folder or the data folder (SPEC.md 12). While a root is missing, an
//! agent can put a link in its place, so every start checks the real path of each root.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::config::with_tilde;
use crate::folder_path::{is_inside_folder, path_bytes, real_path};
use crate::gate::Places;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WideRoot {
    WholeDisk,
    Home,
    AboveHome,
    Settings,
    Data,
}

impl WideRoot {
    fn text(self) -> &'static str {
        match self {
            WideRoot::WholeDisk => "the whole disk",
            WideRoot::Home => "your whole home folder",
            WideRoot::AboveHome => "a folder that holds your home folder",
            WideRoot::Settings => "a folder with the settings of the desktop app",
            WideRoot::Data => "a folder with the data of the desktop app",
        }
    }
}

/// `root` is a real path. A folder that does not resolve stays as it is.
pub fn wide_root(root: &Path, places: &Places) -> Option<WideRoot> {
    if root.parent().is_none() {
        return Some(WideRoot::WholeDisk);
    }
    let root = path_bytes(root);
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
    None
}

/// The line of the log for a missing root that comes back as a wide one.
pub fn wide_root_line(root: &Path, wide: WideRoot, home: &Path) -> String {
    format!(
        "{} is now {}, so agents can't work there.",
        with_tilde(root, home),
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
                "allowed_roots has {}, which is now {}. Agents can't work there. Remove it \
                 from allowed_roots in config.toml, then run gnomish-relay restart.",
                with_tilde(root, places.home),
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

    #[test]
    fn a_project_folder_is_no_wide_root() {
        let c = computer();

        assert_eq!(c.wide(&c.home.join("Code")), None);
        assert_eq!(c.wide(&c.config.join("inner")), None);
    }

    #[test]
    fn a_config_with_the_home_folder_as_a_root_is_refused_and_names_it() {
        let c = computer();
        let roots = [c.home.join("Code"), c.home.clone()];

        let error = refuse_wide_roots(&roots, &c.places()).unwrap_err();

        assert!(
            error
                .to_string()
                .starts_with("allowed_roots has ~, which is now your whole home folder."),
            "{error}"
        );
    }

    #[test]
    fn a_config_with_project_roots_is_fine() {
        let c = computer();

        assert!(refuse_wide_roots(&[c.home.join("Code")], &c.places()).is_ok());
    }
}
