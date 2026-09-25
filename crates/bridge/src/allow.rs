//! The allow table of `config.toml` (SPEC.md 12): commands that run from the game with
//! no question. The classifier still refuses a `deny`, `desktop`, or "never always"
//! command that a pattern names (S17).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config::expand;

/// The words of a pattern cannot hold these, so a pattern is plain words only.
const SPECIAL: &[char] = &[
    '*', '?', '[', ']', '$', '`', '\'', '"', '\\', ';', '&', '|', '<', '>', '(', ')', '{', '}',
    '~', '#', '=',
];

/// The `[allow]` table as the file has it.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AllowFile {
    #[serde(default)]
    commands: Vec<String>,
    /// Patterns for the chats inside one folder, by folder.
    #[serde(default)]
    folders: BTreeMap<String, Vec<String>>,
}

/// One rule: the first words of a command. `cargo test` covers `cargo test -q`.
pub type Rule = Vec<String>;

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct AllowTable {
    everywhere: Vec<Rule>,
    /// Each folder is resolved, as the chat folder is.
    folders: Vec<(PathBuf, Vec<Rule>)>,
}

impl AllowTable {
    /// The rules for a chat in `chat`, a resolved folder.
    pub fn rules_for(&self, chat: &Path) -> Vec<Rule> {
        let mut rules = self.everywhere.clone();
        for (folder, more) in &self.folders {
            if chat.starts_with(folder) {
                rules.extend(more.iter().cloned());
            }
        }
        rules
    }
}

/// A pattern is words with a space between them, and a last `*` that shows that more
/// words can follow. `cargo test *` and `cargo test` give the same rule.
pub fn parse_pattern(pattern: &str) -> Result<Rule> {
    let mut words: Vec<&str> = pattern.split_whitespace().collect();
    if words.last() == Some(&"*") {
        words.pop();
    }
    if words.is_empty() {
        bail!("allow pattern {pattern:?} names no command");
    }
    if let Some(bad) = words.iter().find(|w| w.contains(SPECIAL)) {
        bail!("allow pattern {pattern:?}: {bad:?} is not a plain word");
    }
    Ok(words.into_iter().map(str::to_owned).collect())
}

fn parse_patterns(patterns: &[String]) -> Result<Vec<Rule>> {
    patterns.iter().map(|p| parse_pattern(p)).collect()
}

pub fn parse(file: &AllowFile, home: &Path) -> Result<AllowTable> {
    let mut folders = Vec::new();
    for (folder, patterns) in &file.folders {
        let path = expand(folder, home)?;
        let real = path
            .canonicalize()
            .with_context(|| format!("allow folder {} does not exist", path.display()))?;
        folders.push((real, parse_patterns(patterns)?));
    }
    Ok(AllowTable {
        everywhere: parse_patterns(&file.commands)?,
        folders,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(rule: &[&str]) -> Rule {
        rule.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn a_pattern_is_its_words_with_or_without_a_last_star() {
        assert_eq!(
            parse_pattern("cargo test *").unwrap(),
            words(&["cargo", "test"])
        );
        assert_eq!(
            parse_pattern("  git  status ").unwrap(),
            words(&["git", "status"])
        );
    }

    #[test]
    fn a_pattern_with_shell_syntax_or_no_words_is_an_error() {
        for bad in [
            "",
            "*",
            "cargo * test",
            "rm -rf ~",
            "a; b",
            "x=1 make",
            "echo $HOME",
            "a | sh",
        ] {
            assert!(parse_pattern(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_rules_of_a_folder_apply_to_the_chats_inside_it() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("Code/app/src")).unwrap();
        std::fs::create_dir_all(home.path().join("Code/lib")).unwrap();
        let file: AllowFile = toml::from_str(
            "commands = [\"cargo test *\"]\n[folders]\n\"~/Code/app\" = [\"npm test *\"]\n",
        )
        .unwrap();
        let table = parse(&file, home.path()).unwrap();
        let app = home.path().join("Code/app/src").canonicalize().unwrap();
        let lib = home.path().join("Code/lib").canonicalize().unwrap();
        assert_eq!(
            table.rules_for(&app),
            [words(&["cargo", "test"]), words(&["npm", "test"])]
        );
        assert_eq!(table.rules_for(&lib), [words(&["cargo", "test"])]);
    }

    #[test]
    fn a_missing_folder_or_an_unknown_key_is_an_error() {
        let home = tempfile::tempdir().unwrap();
        let file: AllowFile = toml::from_str("[folders]\n\"~/nope\" = [\"make\"]\n").unwrap();
        assert!(parse(&file, home.path()).is_err());
        assert!(toml::from_str::<AllowFile>("comands = []").is_err());
    }

    #[test]
    fn no_table_is_an_empty_table() {
        let table = parse(&AllowFile::default(), Path::new("/h")).unwrap();
        assert_eq!(table, AllowTable::default());
        assert!(table.rules_for(Path::new("/h/x")).is_empty());
    }
}
