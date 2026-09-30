//! The folders of the bridge on each OS (SPEC.md 8.3).

use std::path::PathBuf;

use anyhow::{Context, Result};

const APP: &str = "gnomish-relay";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

pub struct Dirs {
    pub home: PathBuf,
    /// Holds `config.toml`, `strip.key`, and `timeways.key`.
    pub config: PathBuf,
    /// Holds `state.json`, the lock, the log, and the desktop requests.
    pub data: PathBuf,
}

/// An environment variable as a path, or `None` when it is not set.
pub type EnvVar<'a> = &'a dyn Fn(&str) -> Option<PathBuf>;

impl Dirs {
    pub fn from_env() -> Result<Dirs> {
        Dirs::of(Os::this(), &|name| {
            std::env::var_os(name).map(PathBuf::from)
        })
    }

    pub fn of(os: Os, var: EnvVar) -> Result<Dirs> {
        let home = var("HOME")
            .or_else(|| var("USERPROFILE"))
            .context("HOME is not set")?;
        let config = match os {
            Os::Windows => var("APPDATA").context("APPDATA is not set")?,
            Os::MacOs => home.join("Library").join("Application Support"),
            Os::Linux => var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
        };
        let data = match os {
            Os::Windows => var("LOCALAPPDATA").context("LOCALAPPDATA is not set")?,
            Os::MacOs => home.join("Library").join("Application Support"),
            Os::Linux => var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share")),
        };
        Ok(Dirs {
            home,
            config: config.join(APP),
            data: data.join(APP),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<PathBuf> {
        let pairs: Vec<(String, PathBuf)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), PathBuf::from(v)))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn linux_takes_the_xdg_folders_or_the_ones_below_home() {
        let plain = Dirs::of(Os::Linux, &env(&[("HOME", "/home/x")])).unwrap();
        assert_eq!(plain.config, PathBuf::from("/home/x/.config/gnomish-relay"));
        assert_eq!(
            plain.data,
            PathBuf::from("/home/x/.local/share/gnomish-relay")
        );

        let xdg = [
            ("HOME", "/home/x"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_DATA_HOME", "/d"),
        ];
        let xdg = Dirs::of(Os::Linux, &env(&xdg)).unwrap();
        assert_eq!(xdg.config, PathBuf::from("/c/gnomish-relay"));
        assert_eq!(xdg.data, PathBuf::from("/d/gnomish-relay"));
    }

    #[test]
    fn macos_keeps_both_in_application_support() {
        let dirs = Dirs::of(Os::MacOs, &env(&[("HOME", "/Users/x")])).unwrap();
        let support = PathBuf::from("/Users/x/Library/Application Support/gnomish-relay");
        assert_eq!(dirs.config, support);
        assert_eq!(dirs.data, support);
    }

    #[test]
    fn windows_takes_the_app_data_folders_and_the_user_profile() {
        let vars = [
            ("USERPROFILE", r"C:\Users\x"),
            ("APPDATA", r"C:\Roaming"),
            ("LOCALAPPDATA", r"C:\Local"),
        ];
        let dirs = Dirs::of(Os::Windows, &env(&vars)).unwrap();
        assert_eq!(dirs.home, PathBuf::from(r"C:\Users\x"));
        assert_eq!(
            dirs.config,
            PathBuf::from(r"C:\Roaming").join("gnomish-relay")
        );
        assert_eq!(dirs.data, PathBuf::from(r"C:\Local").join("gnomish-relay"));
    }

    #[test]
    fn a_missing_home_or_app_data_folder_is_an_error() {
        assert!(Dirs::of(Os::Linux, &env(&[])).is_err());
        let no_local = [("USERPROFILE", r"C:\Users\x"), ("APPDATA", r"C:\Roaming")];
        assert!(Dirs::of(Os::Windows, &env(&no_local)).is_err());
    }
}
