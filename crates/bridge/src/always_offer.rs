//! The "Always allow" choice of a game popup (SPEC.md 6.6.5): the rules that one click
//! adds, and the line that names them. `offer` of `protocol` picks the rules (S36 to S39).

use std::path::{Path, PathBuf};

use protocol::action::{Policy, ToolCall};

use crate::always_rules::{
    MAX_PER_FOLDER, Rule, Scope, count_in, is_rule, rule_line, shown_folder, words_for,
};

/// Where the chat runs.
pub struct Place<'a> {
    /// Resolved.
    pub chat: &'a Path,
    pub roots: &'a [PathBuf],
    pub home: &'a Path,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Offer {
    pub rules: Vec<Vec<String>>,
    /// `cargo test * in Code/app`, the label of the choice.
    pub line: String,
    pub folder: PathBuf,
    pub scope: Scope,
}

/// A rule of a root or of the home folder covers only that folder.
pub fn scope_of(place: &Place) -> Scope {
    let wide = place.roots.iter().any(|r| r == place.chat) || place.chat == place.home;
    if wide { Scope::Exact } else { Scope::Tree }
}

fn as_words(rule: &[Vec<u8>]) -> Option<Vec<String>> {
    rule.iter()
        .map(|w| String::from_utf8(w.clone()).ok())
        .collect()
}

/// `None` when the popup offers no "Always allow": the call has a part with no rule, the
/// folder is full, or the line does not fit.
pub fn offer_for(tool: &ToolCall, policy: &Policy, rules: &[Rule], place: &Place) -> Option<Offer> {
    let game = words_for(rules, place.chat);
    let new = protocol::always::offer(tool, policy, &game)?;
    let words: Vec<Vec<String>> = new.iter().map(|r| as_words(r)).collect::<Option<_>>()?;
    if !words.iter().all(|w| is_rule(w)) {
        return None;
    }
    if count_in(rules, place.chat) + words.len() > MAX_PER_FOLDER {
        return None;
    }
    let line = rule_line(&words, &shown_folder(place.chat, place.roots, place.home))?;
    Some(Offer {
        rules: words,
        line,
        folder: place.chat.to_owned(),
        scope: scope_of(place),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::always_rules::day_of;

    const NOW: u32 = 1_790_000_000;

    fn policy() -> Policy {
        crate::action_input::policy(
            &[PathBuf::from("/h/Code")],
            Path::new("/h/Code/app"),
            &[PathBuf::from("/h/.config/gnomish-relay")],
            &[vec!["cargo".into(), "fmt".into()]],
        )
    }

    fn command(raw: &str) -> ToolCall {
        ToolCall::Command {
            raw: raw.as_bytes().to_vec(),
            cwd: b"/h/Code/app".to_vec(),
        }
    }

    fn rule(folder: &str, words: &[&str]) -> Rule {
        Rule {
            id: "a1b2".into(),
            folder: PathBuf::from(folder),
            scope: Scope::Tree,
            words: words.iter().map(|w| (*w).to_owned()).collect(),
            added: NOW,
            used_day: day_of(NOW),
        }
    }

    fn place<'a>(chat: &'a Path, roots: &'a [PathBuf]) -> Place<'a> {
        Place {
            chat,
            roots,
            home: Path::new("/h"),
        }
    }

    #[test]
    fn an_offer_names_its_rules_and_the_folder() {
        let roots = [PathBuf::from("/h/Code")];
        let chat = Path::new("/h/Code/app");
        let got = offer_for(
            &command("cargo test 2>&1 | tail -5"),
            &policy(),
            &[],
            &place(chat, &roots),
        );
        let got = got.unwrap();
        assert_eq!(
            got.rules,
            vec![
                vec!["cargo".to_owned(), "test".to_owned()],
                vec!["tail".to_owned()]
            ]
        );
        assert_eq!(got.line, "cargo test *, tail * in Code/app");
        assert_eq!(got.scope, Scope::Tree);
    }

    #[test]
    fn a_rule_in_a_root_or_the_home_folder_covers_only_that_folder() {
        let roots = [PathBuf::from("/h/Code")];
        assert_eq!(scope_of(&place(Path::new("/h/Code"), &roots)), Scope::Exact);
        assert_eq!(scope_of(&place(Path::new("/h"), &roots)), Scope::Exact);
        assert_eq!(
            scope_of(&place(Path::new("/h/Code/app"), &roots)),
            Scope::Tree
        );
    }

    #[test]
    fn a_full_folder_gets_no_offer() {
        let roots = [PathBuf::from("/h/Code")];
        let chat = Path::new("/h/Code/app");
        let full: Vec<Rule> = (0..MAX_PER_FOLDER)
            .map(|i| rule("/h/Code/app", &[&format!("t{i}")]))
            .collect();
        assert_eq!(
            offer_for(&command("make"), &policy(), &full, &place(chat, &roots)),
            None
        );
        let one_left = &full[1..];
        assert!(offer_for(&command("make"), &policy(), one_left, &place(chat, &roots)).is_some());
    }

    #[test]
    fn a_command_that_a_rule_covers_or_that_has_no_rule_gets_no_offer() {
        let roots = [PathBuf::from("/h/Code")];
        let chat = Path::new("/h/Code/app");
        let rules = [rule("/h/Code", &["make"])];
        assert_eq!(
            offer_for(&command("make"), &policy(), &rules, &place(chat, &roots)),
            None
        );
        assert_eq!(
            offer_for(&command("git push"), &policy(), &[], &place(chat, &roots)),
            None
        );
    }
}
