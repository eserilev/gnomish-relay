//! The roots that `gnomish-relay setup` asks about: a missing root, and a root that is
//! now too wide (SPEC.md 12). The default answer keeps each root: a missing one can be on
//! a drive that isn't connected.

use std::path::Path;

use anyhow::Result;

use crate::config;
use crate::config_edit;
use crate::folder_path::real_path;
use crate::fs_safe::write_private;
use crate::gate::Places;
use crate::setup::Product;
use crate::wide_roots::{Entry, WideRoot, wide_root};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Missing,
    /// The check of the folder fails, for example with no permission.
    Unreadable,
    Wide(WideRoot, Entry),
}

/// A root of the config, as the config writes it, that agents can't use now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadRoot {
    pub root: String,
    pub problem: Problem,
}

impl BadRoot {
    pub fn question(&self) -> String {
        let root = &self.root;
        match self.problem {
            Problem::Missing => format!(
                "Can't find {root}. If it's on a drive that isn't connected, keep it. Remove \
                 it from the folders agents can work in? (y/n)"
            ),
            Problem::Unreadable => {
                format!("Can't open {root}. Remove it from the folders agents can work in? (y/n)")
            }
            Problem::Wide(wide, entry) => format!(
                "{root} {} {}, so agents can't work there, and the desktop app won't \
                 start. Remove it from the folders agents can work in? (y/n)",
                entry.is(),
                wide.text()
            ),
        }
    }
}

/// Only a yes removes a root. With no terminal there is no answer, and the root stays.
pub fn remove_on(answer: Option<String>) -> bool {
    let Some(answer) = answer else {
        return false;
    };
    matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The roots belong to the relay part, so `setup --timeways` never asks.
pub fn ask_about_roots(
    places: &Places,
    product: Product,
    remove: &mut dyn FnMut(&BadRoot) -> bool,
) -> Result<Vec<String>> {
    if product == Product::Timeways {
        return Ok(Vec::new());
    }
    let text = match config::read_text(places.config_dir) {
        Ok(text) => text,
        Err(e) if e.is::<config::SetupUnfinished>() => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut text = text;
    let mut lines = Vec::new();
    let mut removed_any = false;
    for bad in bad_roots(&text, places)? {
        if !remove(&bad) {
            continue;
        }
        match config_edit::without_root(&text, &bad.root) {
            Ok(changed) => {
                text = changed;
                removed_any = true;
                lines.push(format!(
                    "Removed {} from the folders agents can work in.",
                    bad.root
                ));
            }
            Err(e) => lines.push(format!(
                "Can't remove {} from config.toml, so it stays: {e:#}",
                bad.root
            )),
        }
    }
    if removed_any {
        write_private(places.config_dir, config::FILE, &text)?;
    }
    Ok(lines)
}

fn bad_roots(text: &str, places: &Places) -> Result<Vec<BadRoot>> {
    let mut bad = Vec::new();
    for (root, path) in config::allowed_roots_in(text, places.home)? {
        if let Some(problem) = problem_of(&path, places) {
            bad.push(BadRoot { root, problem });
        }
    }
    Ok(bad)
}

fn problem_of(path: &Path, places: &Places) -> Option<Problem> {
    match path.try_exists() {
        Ok(false) => return Some(Problem::Missing),
        Err(e) if config::means_missing(&e) => return Some(Problem::Missing),
        Err(_) => return Some(Problem::Unreadable),
        Ok(true) => {}
    }
    let real = real_path(path).ok()?;
    let wide = wide_root(&real, places)?;
    Some(Problem::Wide(wide, Entry::of(path)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    const TWO_ROOTS: &str = "allowed_roots = [\"~/Code\", \"~/Drive\"]\ndefault_agent = \"echo\"\n\
        [wow]\npath = \"~/wow\"\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n";

    /// The home is a real path, as the temp folder of macOS is under a link.
    struct Computer {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        config: PathBuf,
        data: PathBuf,
    }

    impl Computer {
        fn with_config(text: &str) -> Computer {
            let tmp = tempfile::tempdir().unwrap();
            let home = real_path(tmp.path()).unwrap();
            fs::create_dir_all(home.join("Code")).unwrap();
            let config = home.join("config");
            let data = home.join("data");
            fs::create_dir_all(&config).unwrap();
            fs::create_dir_all(&data).unwrap();
            fs::write(config.join(config::FILE), text).unwrap();
            Computer {
                _tmp: tmp,
                home,
                config,
                data,
            }
        }

        fn places(&self) -> Places<'_> {
            Places {
                config_dir: &self.config,
                data_dir: &self.data,
                home: &self.home,
            }
        }

        fn config_text(&self) -> String {
            fs::read_to_string(self.config.join(config::FILE)).unwrap()
        }
    }

    #[test]
    fn setup_asks_only_about_a_missing_root_and_keeps_it_on_no() {
        let c = Computer::with_config(TWO_ROOTS);
        let mut asked = Vec::new();

        let lines = ask_about_roots(&c.places(), Product::Relay, &mut |bad: &BadRoot| {
            asked.push(bad.clone());
            false
        })
        .unwrap();

        let missing = BadRoot {
            root: "~/Drive".into(),
            problem: Problem::Missing,
        };
        assert_eq!(asked, [missing]);
        assert!(lines.is_empty());
        assert_eq!(c.config_text(), TWO_ROOTS);
    }

    #[test]
    fn setup_removes_a_missing_root_on_yes_and_says_so() {
        let c = Computer::with_config(TWO_ROOTS);

        let lines = ask_about_roots(&c.places(), Product::Relay, &mut |_: &BadRoot| true).unwrap();

        assert_eq!(
            lines,
            ["Removed ~/Drive from the folders agents can work in."]
        );
        assert!(
            c.config_text()
                .starts_with("allowed_roots = [\"~/Code\"]\n"),
            "{}",
            c.config_text()
        );
    }

    #[test]
    fn setup_for_timeways_never_asks_about_a_root() {
        let c = Computer::with_config(TWO_ROOTS);

        let lines = ask_about_roots(&c.places(), Product::Timeways, &mut |_: &BadRoot| {
            panic!("setup --timeways asked about a root")
        })
        .unwrap();

        assert!(lines.is_empty());
        assert_eq!(c.config_text(), TWO_ROOTS);
    }

    #[cfg(unix)]
    #[test]
    fn setup_offers_to_remove_a_root_that_links_to_the_home_folder() {
        let c = Computer::with_config(&TWO_ROOTS.replace("\"~/Drive\"", "\"~/Link\""));
        std::os::unix::fs::symlink(&c.home, c.home.join("Link")).unwrap();
        let mut questions = Vec::new();

        let lines = ask_about_roots(&c.places(), Product::Relay, &mut |bad: &BadRoot| {
            questions.push(bad.question());
            true
        })
        .unwrap();

        assert_eq!(
            questions,
            [
                "~/Link is now your whole home folder, so agents can't work there, and the \
              desktop app won't start. Remove it from the folders agents can work in? (y/n)"
            ]
        );
        assert_eq!(
            lines,
            ["Removed ~/Link from the folders agents can work in."]
        );
    }

    #[test]
    fn setup_says_is_and_not_is_now_for_a_root_that_is_no_link() {
        let c = Computer::with_config(&TWO_ROOTS.replace("\"~/Drive\"", "\"~\""));
        let mut questions = Vec::new();

        ask_about_roots(&c.places(), Product::Relay, &mut |bad: &BadRoot| {
            questions.push(bad.question());
            false
        })
        .unwrap();

        assert_eq!(
            questions,
            [
                "~ is your whole home folder, so agents can't work there, and the desktop app \
              won't start. Remove it from the folders agents can work in? (y/n)"
            ]
        );
    }

    #[test]
    fn a_roots_list_on_many_lines_stays_and_setup_goes_on() {
        let text = TWO_ROOTS.replace(
            "[\"~/Code\", \"~/Drive\"]",
            "[\n  \"~/Code\",\n  \"~/Drive\",\n]",
        );
        let c = Computer::with_config(&text);

        let lines = ask_about_roots(&c.places(), Product::Relay, &mut |_: &BadRoot| true).unwrap();

        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("Can't remove ~/Drive from config.toml, so it stays: "),
            "{lines:?}"
        );
        assert_eq!(c.config_text(), text);
    }

    #[cfg(unix)]
    #[test]
    fn setup_offers_to_remove_a_root_that_it_cant_open() {
        use std::os::unix::fs::PermissionsExt;
        let c = Computer::with_config(&TWO_ROOTS.replace("\"~/Drive\"", "\"~/locked/in\""));
        let locked = c.home.join("locked");
        fs::create_dir_all(locked.join("in")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let mut asked = Vec::new();

        ask_about_roots(&c.places(), Product::Relay, &mut |bad: &BadRoot| {
            asked.push(bad.problem);
            false
        })
        .unwrap();

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let can_open_all = asked.is_empty();
        assert!(can_open_all || asked == [Problem::Unreadable], "{asked:?}");
    }

    #[test]
    fn with_no_terminal_a_root_stays() {
        assert!(!remove_on(None));
    }

    #[test]
    fn only_yes_removes_a_root() {
        assert!(remove_on(Some("y".into())));
        assert!(remove_on(Some("Yes".into())));
        assert!(!remove_on(Some("n".into())));
        assert!(!remove_on(Some(String::new())));
        assert!(!remove_on(Some("maybe".into())));
    }
}
