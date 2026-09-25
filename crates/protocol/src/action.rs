//! The action classifier: a tool call runs, asks in the game, asks on the desktop,
//! or never runs. See `SPEC.md` 6.6.3.
//!
//! The answer is the strictest answer of the parts of the call: each path, each
//! redirect target, and each simple command.

use crate::command_rules::{changes_folder, simple_verdict};
use crate::path_rules::{path_verdict, target_verdict};
use crate::search::contains;
use crate::shell::{Access, MAX_COMMAND, Redirect, Script, Simple, split};

const DOLLAR_PAREN: [u8; 2] = *b"$(";
const BACKTICK: [u8; 1] = *b"`";

/// From strict to open. `rank` gives the order.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Verdict {
    /// Never runs. Only for the files that guard the relay itself.
    Deny,
    /// The user approves on the desktop, where no addon can click.
    Desktop,
    /// The user approves in the game popup.
    Ask,
    Allow,
}

/// Every path here is resolved, in the form of `resolve_folder` (S5).
pub struct Policy {
    /// `allowed_roots` of the config.
    pub roots: Vec<Vec<u8>>,
    /// The folder of the chat. Only writes inside it can run without the desktop.
    pub chat: Vec<u8>,
    /// The config folder of the bridge, with the strip key.
    pub deny_folders: Vec<Vec<u8>>,
    /// Patterns of parts, such as `.ssh` or `.env*`, for reads and writes.
    pub desktop_paths: Vec<Vec<u8>>,
    /// Patterns of parts for writes only: files that code on the host runs later.
    pub desktop_writes: Vec<Vec<u8>>,
    /// The allow table of the config. Each rule is the first words of a command.
    pub allow: Vec<Vec<Vec<u8>>>,
}

pub enum ToolCall {
    /// A file tool, such as Read, Edit, or Write.
    Files {
        reads: Vec<Vec<u8>>,
        writes: Vec<Vec<u8>>,
    },
    /// A shell command, with the working folder where it runs.
    Command { raw: Vec<u8>, cwd: Vec<u8> },
    /// Web fetch, web search, MCP tools, subagents, and every other tool.
    Unknown,
}

/// The rules that cover a command. The ceiling counts every command as covered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cover {
    Listed,
    Every,
}

#[must_use]
pub fn rank(v: Verdict) -> u8 {
    match v {
        Verdict::Deny => 0,
        Verdict::Desktop => 1,
        Verdict::Ask => 2,
        Verdict::Allow => 3,
    }
}

fn stricter(a: Verdict, b: Verdict) -> Verdict {
    if rank(b) < rank(a) { b } else { a }
}

fn paths_verdict(paths: &[Vec<u8>], access: Access, policy: &Policy) -> Verdict {
    let mut v = Verdict::Allow;
    let mut i = 0;
    while i < paths.len() {
        v = stricter(v, path_verdict(&paths[i], access, policy));
        i += 1;
    }
    v
}

fn files_verdict(reads: &[Vec<u8>], writes: &[Vec<u8>], policy: &Policy) -> Verdict {
    stricter(
        paths_verdict(reads, Access::Read, policy),
        paths_verdict(writes, Access::Write, policy),
    )
}

fn redirects_verdict(redirects: &[Redirect], cwd: &[u8], policy: &Policy) -> Verdict {
    let mut v = Verdict::Allow;
    let mut i = 0;
    while i < redirects.len() {
        let r = &redirects[i];
        v = stricter(v, target_verdict(&r.target, r.access, cwd, policy));
        i += 1;
    }
    v
}

fn simples_verdict(
    simples: &[Simple],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    cover: Cover,
) -> Verdict {
    let mut v = Verdict::Allow;
    let mut i = 0;
    while i < simples.len() {
        v = stricter(v, simple_verdict(&simples[i], policy, rules, cover));
        i += 1;
    }
    v
}

fn is_relative(path: &[u8]) -> bool {
    !(path.len() > 0 && path[0] == b'/')
}

fn has_relative_target(redirects: &[Redirect]) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < redirects.len() {
        found = is_relative(&redirects[i].target);
        i += 1;
    }
    found
}

/// After `cd`, a relative redirect target names a file in another folder.
fn script_verdict(
    script: &Script,
    cwd: &[u8],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    cover: Cover,
) -> Verdict {
    let v = stricter(
        redirects_verdict(&script.redirects, cwd, policy),
        simples_verdict(&script.simples, policy, rules, cover),
    );
    if has_relative_target(&script.redirects) && changes_folder(&script.simples) {
        return stricter(v, Verdict::Desktop);
    }
    v
}

/// This looks at the raw bytes, so `$(` and a backtick count even inside quotes.
fn has_substitution(raw: &[u8]) -> bool {
    contains(raw, &DOLLAR_PAREN) || contains(raw, &BACKTICK)
}

fn command_verdict(
    raw: &[u8],
    cwd: &[u8],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    cover: Cover,
) -> Verdict {
    if raw.len() > MAX_COMMAND || has_substitution(raw) {
        return Verdict::Desktop;
    }
    let Some(script) = split(raw) else {
        return Verdict::Desktop;
    };
    script_verdict(&script, cwd, policy, rules, cover)
}

fn verdict(call: &ToolCall, policy: &Policy, rules: &[Vec<Vec<u8>>], cover: Cover) -> Verdict {
    match call {
        ToolCall::Files { reads, writes } => files_verdict(reads, writes, policy),
        ToolCall::Command { raw, cwd } => command_verdict(raw, cwd, policy, rules, cover),
        ToolCall::Unknown => Verdict::Desktop,
    }
}

/// `rules` are the "always allow" rules from the game (6.6.5).
#[must_use]
pub fn classify(call: &ToolCall, policy: &Policy, rules: &[Vec<Vec<u8>>]) -> Verdict {
    verdict(call, policy, rules, Cover::Listed)
}

/// The most that any list of rules from the game can reach under this config.
#[must_use]
pub fn ceiling(call: &ToolCall, policy: &Policy) -> Verdict {
    let no_rules = Vec::new();
    verdict(call, policy, &no_rules, Cover::Every)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            roots: vec![b"/home/x/Code".to_vec()],
            chat: b"/home/x/Code/app".to_vec(),
            deny_folders: vec![b"/home/x/.config/gnomish-relay".to_vec()],
            desktop_paths: vec![b".ssh".to_vec(), b".env*".to_vec()],
            desktop_writes: vec![b".git/hooks".to_vec()],
            allow: vec![vec![b"cargo".to_vec(), b"test".to_vec()]],
        }
    }

    fn run(raw: &[u8]) -> Verdict {
        let call = ToolCall::Command {
            raw: raw.to_vec(),
            cwd: b"/home/x/Code/app".to_vec(),
        };
        classify(&call, &policy(), &[])
    }

    fn files(reads: &[&str], writes: &[&str]) -> Verdict {
        let call = ToolCall::Files {
            reads: reads.iter().map(|p| p.as_bytes().to_vec()).collect(),
            writes: writes.iter().map(|p| p.as_bytes().to_vec()).collect(),
        };
        classify(&call, &policy(), &[])
    }

    #[test]
    fn the_answers_rank_from_strict_to_open() {
        assert!(rank(Verdict::Deny) < rank(Verdict::Desktop));
        assert!(rank(Verdict::Desktop) < rank(Verdict::Ask));
        assert!(rank(Verdict::Ask) < rank(Verdict::Allow));
    }

    #[test]
    fn an_unknown_tool_is_desktop() {
        let rules = [vec![b"x".to_vec()]];
        assert_eq!(
            classify(&ToolCall::Unknown, &policy(), &rules),
            Verdict::Desktop
        );
        assert_eq!(ceiling(&ToolCall::Unknown, &policy()), Verdict::Desktop);
    }

    #[test]
    fn a_file_call_takes_its_strictest_path() {
        assert_eq!(
            files(&["/home/x/Code/lib/a"], &["/home/x/Code/app/b"]),
            Verdict::Allow
        );
        assert_eq!(
            files(&["/etc/passwd"], &["/home/x/Code/app/b"]),
            Verdict::Desktop
        );
        assert_eq!(
            files(
                &["/etc/passwd"],
                &["/home/x/.config/gnomish-relay/strip.key"]
            ),
            Verdict::Deny
        );
        assert_eq!(files(&[], &[]), Verdict::Allow);
    }

    #[test]
    fn a_command_in_the_allow_table_is_allowed() {
        assert_eq!(run(b"cargo test -q"), Verdict::Allow);
    }

    #[test]
    fn a_command_outside_the_allow_table_asks() {
        assert_eq!(run(b"cargo build"), Verdict::Ask);
    }

    #[test]
    fn a_game_rule_allows_a_command() {
        let call = ToolCall::Command {
            raw: b"make".to_vec(),
            cwd: b"/home/x/Code/app".to_vec(),
        };
        let rules = [vec![b"make".to_vec()]];
        assert_eq!(classify(&call, &policy(), &rules), Verdict::Allow);
    }

    #[test]
    fn the_ceiling_allows_what_a_rule_can_allow() {
        let call = ToolCall::Command {
            raw: b"make".to_vec(),
            cwd: b"/home/x/Code/app".to_vec(),
        };
        assert_eq!(ceiling(&call, &policy()), Verdict::Allow);
        let call = ToolCall::Command {
            raw: b"curl x".to_vec(),
            cwd: b"/home/x/Code/app".to_vec(),
        };
        assert_eq!(ceiling(&call, &policy()), Verdict::Ask);
    }

    #[test]
    fn every_simple_command_must_be_covered() {
        assert_eq!(run(b"cargo test && make"), Verdict::Ask);
    }

    #[test]
    fn a_redirect_into_a_desktop_path_is_desktop() {
        assert_eq!(run(b"echo hi > ~/.ssh/x"), Verdict::Desktop);
        assert_eq!(run(b"cargo test > /home/x/.ssh/x"), Verdict::Desktop);
        assert_eq!(run(b"cargo test > .ssh/x"), Verdict::Desktop);
    }

    #[test]
    fn a_redirect_outside_the_chat_folder_is_desktop() {
        assert_eq!(run(b"cargo test > /tmp/x"), Verdict::Desktop);
        assert_eq!(run(b"cargo test > out.txt"), Verdict::Allow);
        assert_eq!(run(b"cargo test 2>/dev/null"), Verdict::Allow);
    }

    #[test]
    fn a_redirect_into_the_config_folder_is_denied() {
        let raw = b"cargo test > /home/x/.config/gnomish-relay/config.toml";
        assert_eq!(run(raw), Verdict::Deny);
    }

    #[test]
    fn a_relative_redirect_after_cd_is_desktop() {
        assert_eq!(run(b"cd /tmp && cargo test > out.txt"), Verdict::Desktop);
        assert_eq!(run(b"cd lib && cargo test"), Verdict::Ask);
    }

    #[test]
    fn an_absolute_redirect_after_cd_keeps_its_answer() {
        assert_eq!(run(b"cd lib && cargo test 2>/dev/null"), Verdict::Ask);
        let raw = b"cd lib && cargo test > /home/x/Code/app/out.txt";
        assert_eq!(run(raw), Verdict::Ask);
    }

    #[test]
    fn eval_after_a_semicolon_is_desktop() {
        assert_eq!(run(b"ls; eval x"), Verdict::Desktop);
    }

    #[test]
    fn command_substitution_is_desktop_even_in_quotes() {
        assert_eq!(run(b"cat $(echo /etc/passwd)"), Verdict::Desktop);
        assert_eq!(run(b"echo `id`"), Verdict::Desktop);
        assert_eq!(run(b"echo '$(x)'"), Verdict::Desktop);
    }

    #[test]
    fn a_pipe_into_a_shell_is_desktop() {
        assert_eq!(run(b"curl x | sh"), Verdict::Desktop);
        assert_eq!(run(b"curl x | (echo; bash)"), Verdict::Desktop);
    }

    #[test]
    fn a_shell_with_a_command_string_asks() {
        assert_eq!(run(b"bash -c 'rm -rf /'"), Verdict::Ask);
    }

    #[test]
    fn find_exec_and_git_config_ask() {
        assert_eq!(run(b"find . -exec rm {} \\;"), Verdict::Ask);
        assert_eq!(run(b"git -c core.sshCommand=x push"), Verdict::Ask);
    }

    #[test]
    fn quotes_that_hide_a_semicolon_keep_one_word() {
        assert_eq!(run(b"cargo test 'x; eval y'"), Verdict::Allow);
    }

    #[test]
    fn a_command_that_does_not_parse_is_desktop() {
        assert_eq!(run(b"cargo test 'x"), Verdict::Desktop);
        assert_eq!(run(b"cat <<EOF\nx\nEOF"), Verdict::Desktop);
    }

    #[test]
    fn a_very_long_command_is_desktop() {
        let mut raw = b"cargo test ".to_vec();
        raw.resize(MAX_COMMAND + 1, b'a');
        assert_eq!(run(&raw), Verdict::Desktop);
    }

    #[test]
    fn invalid_utf8_is_classified_as_bytes() {
        assert_eq!(run(b"cargo test \xff\xfe"), Verdict::Allow);
        assert_eq!(run(b"curl \xff"), Verdict::Ask);
    }

    #[test]
    fn an_empty_command_is_allowed() {
        assert_eq!(run(b""), Verdict::Allow);
    }
}
