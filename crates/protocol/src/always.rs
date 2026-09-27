//! "Always allow": the rule that one click in the game adds. See `SPEC.md` 6.6.5.
//!
//! The bridge makes each rule from the words of one simple command. The agent and the
//! game never choose it. S36 to S39 cover this file.

use crate::action::{Cover, Policy, ToolCall, Verdict, classify};
use crate::ascii::{copy_bytes, push_bytes};
use crate::command_rules::{is_capped, is_covered, is_desktop, matches_any, name_of};
use crate::search::{has_byte, listed};
use crate::shell::{Simple, split};

/// The most rules that one click adds, one for each simple command of the call.
pub const MAX_OFFER: usize = 3;
/// The longest word of a rule.
pub const MAX_WORD: usize = 64;
/// The bridge keeps at most 64 rules in a folder, so no chat has this many. The bound
/// keeps the join of the rules small.
pub const MAX_RULES: usize = 4096;

/// Tools whose first word picks what they do: the rule keeps that word.
const SUBCOMMAND_TOOLS: [u8; 72] =
    *b" git cargo npm pnpm yarn go uv pip poetry gradle mvn dotnet rustup just ";
/// Tools that run any program or download code, and tools that publish. A rule for
/// them is "all Bash", or sends work to other people.
const NO_RULE_TOOLS: [u8; 35] = *b" npx bunx uvx pipx docker twine gh ";
/// The same, as `name:word` for a tool with subcommands.
const NO_RULE_PAIRS: [u8; 101] = *b" npm:exec pnpm:exec pnpm:dlx yarn:dlx yarn:exec uv:run poetry:run git:push cargo:publish npm:publish ";
/// The shell syntax that the allow table of the config refuses in a pattern (SPEC.md 12).
const SPECIAL: [u8; 21] = *b"*?[]$`'\"\\;&|<>(){}~#=";

/// Printable ASCII with no space and no shell syntax.
fn is_plain_byte(b: u8) -> bool {
    b'!' <= b && b <= b'~' && !has_byte(&SPECIAL, b)
}

fn all_plain_bytes(word: &[u8]) -> bool {
    let mut plain = true;
    let mut i = 0;
    while plain && i < word.len() {
        plain = is_plain_byte(word[i]);
        i += 1;
    }
    plain
}

/// A word that `parse_pattern` of the bridge reads back as the same word, and that is
/// not a flag or a toolchain such as `+nightly`.
fn is_plain_word(word: &[u8]) -> bool {
    word.len() > 0
        && word.len() <= MAX_WORD
        && word[0] != b'-'
        && word[0] != b'+'
        && all_plain_bytes(word)
}

/// `name:second`, as `NO_RULE_PAIRS` lists it. The caller keeps both short.
fn pair_of(name: &[u8], second: &[u8]) -> Vec<u8> {
    let mut pair = copy_bytes(name);
    pair.push(b':');
    push_bytes(&mut pair, second);
    pair
}

/// A name that long is in no list, and the check keeps `pair_of` small.
fn is_short(name: &[u8]) -> bool {
    name.len() < NO_RULE_PAIRS.len()
}

fn is_no_rule_pair(name: &[u8], words: &[Vec<u8>]) -> bool {
    if words.len() < 2 || !is_short(name) {
        return false;
    }
    let second = name_of(&words[1]);
    is_short(&second) && listed(&NO_RULE_PAIRS, &pair_of(name, &second))
}

fn is_no_rule_tool(words: &[Vec<u8>]) -> bool {
    if words.len() == 0 {
        return false;
    }
    let name = name_of(&words[0]);
    listed(&NO_RULE_TOOLS, &name) || is_no_rule_pair(&name, words)
}

fn has_subcommands(word: &[u8]) -> bool {
    listed(&SUBCOMMAND_TOOLS, &name_of(word))
}

/// The first `n` words, as their own bytes. The caller keeps `n <= words.len()`.
fn first_words(words: &[Vec<u8>], n: usize) -> Vec<Vec<u8>> {
    let mut rule = Vec::new();
    let mut i = 0;
    while i < n {
        rule.push(copy_bytes(&words[i]));
        i += 1;
    }
    rule
}

fn is_ruled_out(simple: &Simple) -> bool {
    let words = &simple.words;
    words.len() == 0 || is_desktop(simple) || is_capped(words) || is_no_rule_tool(words)
}

/// The rule for one simple command: its name, and the first word for a tool with
/// subcommands. `None` when no rule can be narrow enough.
#[must_use]
pub fn propose(simple: &Simple) -> Option<Vec<Vec<u8>>> {
    if is_ruled_out(simple) {
        return None;
    }
    let words = &simple.words;
    let head = &words[0];
    if has_byte(head, b'/') || !is_plain_word(head) {
        return None;
    }
    if !has_subcommands(head) {
        return Some(first_words(words, 1));
    }
    // `cargo *` would also cover `cargo publish`.
    if words.len() < 2 || !is_plain_word(&words[1]) {
        return None;
    }
    Some(first_words(words, 2))
}

fn is_covered_by(
    words: &[Vec<u8>],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    new: &[Vec<Vec<u8>>],
) -> bool {
    is_covered(words, policy, rules, Cover::Listed) || matches_any(new, words)
}

/// Adds the rule of `simple` when nothing covers it yet. `false` when it has no rule.
fn add_rule(
    mut new: Vec<Vec<Vec<u8>>>,
    simple: &Simple,
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
) -> (Vec<Vec<Vec<u8>>>, bool) {
    if is_covered_by(&simple.words, policy, rules, &new) {
        return (new, true);
    }
    match propose(simple) {
        Some(rule) => {
            new.push(rule);
            (new, true)
        }
        None => (new, false),
    }
}

fn new_rules(
    simples: &[Simple],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
) -> Option<Vec<Vec<Vec<u8>>>> {
    let mut new = Vec::new();
    let mut ok = true;
    let mut i = 0;
    while ok && i < simples.len() {
        let (next, added) = add_rule(new, &simples[i], policy, rules);
        new = next;
        ok = added;
        i += 1;
    }
    if ok { Some(new) } else { None }
}

fn proposal(call: &ToolCall, policy: &Policy, rules: &[Vec<Vec<u8>>]) -> Option<Vec<Vec<Vec<u8>>>> {
    match call {
        ToolCall::Command { raw, cwd: _ } => match split(raw) {
            Some(script) => new_rules(&script.simples, policy, rules),
            None => None,
        },
        _ => None,
    }
}

fn allows_with(
    call: &ToolCall,
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    new: &[Vec<Vec<u8>>],
) -> bool {
    let mut all = rules.to_vec();
    all.extend_from_slice(new);
    classify(call, policy, &all) == Verdict::Allow
}

/// The rules that one click adds for `call`: 1 to 3 of them, and with them the call
/// runs with no question. `None` when the popup offers no "Always allow".
#[must_use]
pub fn offer(
    call: &ToolCall,
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
) -> Option<Vec<Vec<Vec<u8>>>> {
    if rules.len() > MAX_RULES {
        return None;
    }
    let Some(new) = proposal(call, policy, rules) else {
        return None;
    };
    if new.len() == 0 || new.len() > MAX_OFFER {
        return None;
    }
    if !allows_with(call, policy, rules, &new) {
        return None;
    }
    Some(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Link;

    fn simple(words: &[&str]) -> Simple {
        Simple {
            words: words.iter().map(|w| w.as_bytes().to_vec()).collect(),
            link: Link::First,
        }
    }

    fn rule(words: &[&str]) -> Vec<Vec<u8>> {
        words.iter().map(|w| w.as_bytes().to_vec()).collect()
    }

    fn proposed(words: &[&str]) -> Option<Vec<Vec<u8>>> {
        propose(&simple(words))
    }

    fn policy() -> Policy {
        Policy {
            roots: vec![b"/home/x/Code".to_vec()],
            chat: b"/home/x/Code/app".to_vec(),
            deny_folders: vec![b"/home/x/.config/gnomish-relay".to_vec()],
            desktop_paths: vec![b".ssh".to_vec()],
            desktop_writes: vec![b".git/hooks".to_vec()],
            allow: vec![rule(&["cargo", "fmt"])],
        }
    }

    fn offered(raw: &str, rules: &[Vec<Vec<u8>>]) -> Option<Vec<Vec<Vec<u8>>>> {
        let call = ToolCall::Command {
            raw: raw.as_bytes().to_vec(),
            cwd: b"/home/x/Code/app".to_vec(),
        };
        offer(&call, &policy(), rules)
    }

    #[test]
    fn a_tool_with_subcommands_keeps_its_first_word() {
        assert_eq!(
            proposed(&["cargo", "test", "-p", "x"]),
            Some(rule(&["cargo", "test"]))
        );
        assert_eq!(
            proposed(&["npm", "run", "build"]),
            Some(rule(&["npm", "run"]))
        );
        assert_eq!(proposed(&["git", "status"]), Some(rule(&["git", "status"])));
    }

    #[test]
    fn another_tool_keeps_only_its_name() {
        assert_eq!(proposed(&["rg", "foo", "src"]), Some(rule(&["rg"])));
        assert_eq!(proposed(&["make", "test"]), Some(rule(&["make"])));
        assert_eq!(proposed(&["ls"]), Some(rule(&["ls"])));
    }

    #[test]
    fn the_rule_keeps_the_literal_bytes_of_the_words() {
        assert_eq!(proposed(&["Cargo", "test"]), Some(rule(&["Cargo", "test"])));
    }

    #[test]
    fn a_tool_with_subcommands_and_no_plain_first_word_gets_no_rule() {
        assert_eq!(proposed(&["cargo"]), None);
        assert_eq!(proposed(&["git", "-C", "x", "status"]), None);
        assert_eq!(proposed(&["cargo", "+nightly", "test"]), None);
        assert_eq!(proposed(&["npm", "run build"]), None);
    }

    #[test]
    fn a_name_with_a_folder_gets_no_rule() {
        assert_eq!(proposed(&["./gradlew", "test"]), None);
        assert_eq!(proposed(&["/usr/bin/ls"]), None);
    }

    #[test]
    fn tools_that_run_any_program_get_no_rule() {
        for words in [
            &["npx", "x"][..],
            &["npm", "exec", "x"],
            &["pnpm", "dlx", "x"],
            &["yarn", "exec", "x"],
            &["uv", "run", "x"],
            &["poetry", "run", "x"],
            &["docker", "ps"],
            &["bunx", "x"],
        ] {
            assert_eq!(proposed(words), None, "{words:?}");
        }
    }

    #[test]
    fn commands_that_publish_get_no_rule() {
        for words in [
            &["git", "push"][..],
            &["cargo", "publish"],
            &["npm", "publish"],
            &["twine", "upload"],
            &["gh", "pr", "create"],
            &["/usr/bin/GIT.exe", "PUSH"],
        ] {
            assert_eq!(proposed(words), None, "{words:?}");
        }
    }

    #[test]
    fn capped_and_desktop_commands_get_no_rule() {
        assert_eq!(proposed(&["rm", "-rf", "target"]), None);
        assert_eq!(proposed(&["curl", "x"]), None);
        assert_eq!(proposed(&["sudo", "ls"]), None);
        assert_eq!(proposed(&["python3", "x.py"]), None);
        assert_eq!(proposed(&[]), None);
    }

    #[test]
    fn a_word_with_shell_syntax_or_a_long_word_gets_no_rule() {
        assert_eq!(proposed(&["ls*"]), None);
        assert_eq!(proposed(&["-x"]), None);
        let long = "a".repeat(MAX_WORD + 1);
        assert_eq!(proposed(&[long.as_str()]), None);
        let max = "a".repeat(MAX_WORD);
        assert_eq!(proposed(&[max.as_str()]), Some(rule(&[max.as_str()])));
        assert_eq!(proposed(&["t\u{e9}st"]), None);
    }

    #[test]
    fn an_offer_names_each_uncovered_command_once() {
        let rules = offered("cargo test 2>&1 | tail -40", &[]);
        assert_eq!(rules, Some(vec![rule(&["cargo", "test"]), rule(&["tail"])]));
        let rules = offered("cargo test && cargo test -q", &[]);
        assert_eq!(rules, Some(vec![rule(&["cargo", "test"])]));
    }

    #[test]
    fn an_offer_skips_what_the_config_or_a_rule_covers() {
        let rules = offered("cargo fmt && cargo test", &[rule(&["cargo", "test"])]);
        assert_eq!(rules, None);
        let rules = offered("cargo fmt && make", &[]);
        assert_eq!(rules, Some(vec![rule(&["make"])]));
    }

    #[test]
    fn an_offer_needs_every_part_to_get_a_rule() {
        assert_eq!(offered("cargo test && git push", &[]), None);
        assert_eq!(offered("cargo test > ~/.ssh/x", &[]), None);
        assert_eq!(offered("cargo test > /tmp/out", &[]), None);
        assert_eq!(offered("echo $(id)", &[]), None);
        assert_eq!(offered("echo 'x", &[]), None);
    }

    #[test]
    fn an_offer_holds_at_most_three_rules() {
        assert_eq!(offered("a && b && c", &[]).map(|r| r.len()), Some(3));
        assert_eq!(offered("a && b && c && d", &[]), None);
    }

    #[test]
    fn a_file_call_or_an_unknown_tool_gets_no_offer() {
        let files = ToolCall::Files {
            reads: vec![],
            writes: vec![b"/home/x/Code/app/a".to_vec()],
        };
        assert_eq!(offer(&files, &policy(), &[]), None);
        assert_eq!(offer(&ToolCall::Unknown, &policy(), &[]), None);
    }

    #[test]
    fn an_empty_command_gets_no_offer() {
        assert_eq!(offered("", &[]), None);
    }
}
