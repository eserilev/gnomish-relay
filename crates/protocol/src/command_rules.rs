//! The command rules of the action classifier, for one simple command.
//! See `SPEC.md` 6.6.3.
//!
//! Most lists match any word, not only the command name. So a wrapper such as
//! `timeout 5 sudo x` or `xargs curl` cannot hide a name. A name matches after
//! its folder, its ASCII case, and a last `.exe` come off: `/usr/bin/SUDO.exe` is `sudo`.

use crate::action::{Cover, Policy, Verdict};
use crate::ascii::{bytes_equal, push_range, to_lower};
use crate::search::{equal_run, has_byte, listed, starts_listed};
use crate::shell::{Link, Simple};

// Each list has a space before and after every name (see `listed`).

/// Commands that the user approves on the desktop: `eval`, a change of user, and the
/// shells of Windows, which have no parser here.
const DESKTOP_NAMES: [u8; 84] =
    *b" eval sudo sudoedit doas su pkexec run0 gsudo runas cmd command.com powershell pwsh ";
/// A shell after a `|` runs text that another command wrote.
const SHELLS: [u8; 53] = *b" sh bash zsh dash ksh mksh fish csh tcsh ash busybox ";
const NETWORK: [u8; 91] =
    *b" curl wget nc ncat netcat socat ssh scp sftp rsync ftp tftp telnet aria2c lftp mosh rclone ";
/// Commands that run other commands or code from their arguments. A wrapper such as
/// `strace` is here, so that a rule for it cannot cover `strace rm -rf x`.
const RUNNERS: [u8; 408] = *b" xargs env exec nohup time timeout nice ionice setsid stdbuf watch parallel flock script strace ltrace valgrind gdb unshare nsenter chroot taskset numactl firejail bwrap runuser sg tmux screen caffeinate wsl crontab systemd-run launchctl schtasks sh bash zsh dash ksh mksh fish csh tcsh ash busybox python python2 python3 pypy pypy3 node nodejs deno bun perl ruby php lua luajit awk gawk mawk nawk osascript ";
/// Runners with a version after the name, such as `python3.12` or `node18`.
const VERSIONED_RUNNERS: [u8; 36] = *b" python pypy perl ruby php lua node ";
/// Runners that count only as the command name: `.` is also the current folder, and
/// `set` or `local` are common words. Each changes how later commands run. `open`
/// runs a file, but `grep -rn open src` and `man open` only name it.
const HEAD_RUNNERS: [u8; 108] = *b" . source command builtin enable trap export declare typeset local readonly set unset shopt alias hash open ";
const NEVER_ALWAYS: [u8; 19] = *b" chmod chown chgrp ";
/// `git` flags and words that run a command, compared before any `=`.
const GIT_RUNS: [u8; 156] = *b" -c --config-env --exec-path --upload-pack --receive-pack --exec -x config submodule bisect filter-branch difftool mergetool --open-files-in-pager --extcmd ";
const GIT_FORCE: [u8; 64] = *b" --force --force-with-lease --force-if-includes --hard --mirror ";
const FIND_RUNS: [u8; 35] = *b" -exec -execdir -ok -okdir -delete ";
const FOLDER_CHANGES: [u8; 15] = *b" cd pushd popd ";

const RM: [u8; 2] = *b"rm";
const GIT: [u8; 3] = *b"git";
const FIND: [u8; 4] = *b"find";
const DOT_EXE: [u8; 4] = *b".exe";
const DASH_DASH: [u8; 2] = *b"--";
/// `git grep -O<pager>` runs the pager.
const DASH_O: [u8; 2] = *b"-O";
/// For the tests that need no list.
const NO_LIST: [u8; 0] = [];

/// What a word is checked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Test {
    /// Its name is in the list.
    Name,
    /// Its name is in the list after its version comes off: `python3.12` is `python`.
    Versioned,
    /// Its flag, the part before any `=`, is in the list.
    Flag,
    /// Its flag is in the list, or starts a long flag of the list (see `starts_long_flag`).
    Option,
    /// It starts with `names`, which holds one prefix here, not a list.
    Prefix,
    /// It is a flag with `r` or `R`, as in `rm -rf`.
    Recursive,
    /// It forces a `git` push or reset.
    GitForce,
}

/// A `/` or `\` starts the name over.
fn name_step(mut name: Vec<u8>, b: u8) -> Vec<u8> {
    if b == b'/' || b == b'\\' {
        return Vec::new();
    }
    name.push(to_lower(b));
    name
}

fn without_exe(name: Vec<u8>) -> Vec<u8> {
    let n = name.len();
    if n < DOT_EXE.len() || !equal_run(&name, n - DOT_EXE.len(), &DOT_EXE, DOT_EXE.len()) {
        return name;
    }
    let mut out = Vec::new();
    push_range(&mut out, &name, 0, n - DOT_EXE.len());
    out
}

pub(crate) fn name_of(word: &[u8]) -> Vec<u8> {
    let mut name = Vec::new();
    let mut i = 0;
    while i < word.len() {
        name = name_step(name, word[i]);
        i += 1;
    }
    without_exe(name)
}

fn is_version_byte(b: u8) -> bool {
    (b'0' <= b && b <= b'9') || b == b'.'
}

/// The length of `name` before its last run of digits and dots: 6 for `python3.12`.
fn stem_len(name: &[u8]) -> usize {
    let mut k = name.len();
    while k > 0 && is_version_byte(name[k - 1]) {
        k -= 1;
    }
    k
}

/// A name with a version, such as `node18`, whose stem is in the list.
fn is_versioned(name: &[u8], names: &[u8]) -> bool {
    let k = stem_len(name);
    if k == name.len() {
        return false;
    }
    let mut stem = Vec::new();
    push_range(&mut stem, name, 0, k);
    listed(names, &stem)
}

fn flag_of(word: &[u8]) -> Vec<u8> {
    let mut flag = Vec::new();
    let mut i = 0;
    while i < word.len() && word[i] != b'=' {
        flag.push(word[i]);
        i += 1;
    }
    flag
}

fn starts_with(word: &[u8], b: u8) -> bool {
    word.len() > 0 && word[0] == b
}

fn has_prefix(word: &[u8], prefix: &[u8]) -> bool {
    prefix.len() <= word.len() && equal_run(word, 0, prefix, prefix.len())
}

/// GNU and `git` take the start of a long flag: `--upload` is `--upload-pack`.
fn starts_long_flag(flag: &[u8], names: &[u8]) -> bool {
    flag.len() > DASH_DASH.len() && has_prefix(flag, &DASH_DASH) && starts_listed(names, flag)
}

fn is_option(word: &[u8], names: &[u8]) -> bool {
    let flag = flag_of(word);
    listed(names, &flag) || starts_long_flag(&flag, names)
}

/// `-fu` is two short flags. `--force` is one long flag.
fn is_short_flags(word: &[u8]) -> bool {
    starts_with(word, b'-') && !(word.len() > 1 && word[1] == b'-')
}

fn is_recursive(word: &[u8]) -> bool {
    starts_with(word, b'-') && (has_byte(word, b'r') || has_byte(word, b'R'))
}

/// `+main` in a refspec forces the push, as `--force` does.
fn is_git_force(word: &[u8]) -> bool {
    listed(&GIT_FORCE, &flag_of(word))
        || starts_with(word, b'+')
        || (is_short_flags(word) && has_byte(word, b'f'))
}

fn word_passes(word: &[u8], test: Test, names: &[u8]) -> bool {
    match test {
        Test::Name => listed(names, &name_of(word)),
        Test::Versioned => is_versioned(&name_of(word), names),
        Test::Flag => listed(names, &flag_of(word)),
        Test::Option => is_option(word, names),
        Test::Prefix => has_prefix(word, names),
        Test::Recursive => is_recursive(word),
        Test::GitForce => is_git_force(word),
    }
}

fn any_word(words: &[Vec<u8>], test: Test, names: &[u8]) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < words.len() {
        found = word_passes(&words[i], test, names);
        i += 1;
    }
    found
}

fn head_is(words: &[Vec<u8>], name: &[u8]) -> bool {
    words.len() > 0 && bytes_equal(&name_of(&words[0]), name)
}

fn head_listed(words: &[Vec<u8>], names: &[u8]) -> bool {
    words.len() > 0 && listed(names, &name_of(&words[0]))
}

/// `A=1 cmd` sets a variable for `cmd`, and `LD_PRELOAD` or `GIT_SSH_COMMAND`
/// can run any code.
fn head_assigns(words: &[Vec<u8>]) -> bool {
    words.len() > 0 && has_byte(&words[0], b'=')
}

fn is_git_runner(words: &[Vec<u8>]) -> bool {
    head_is(words, &GIT)
        && (any_word(words, Test::Option, &GIT_RUNS) || any_word(words, Test::Prefix, &DASH_O))
}

fn is_runner(words: &[Vec<u8>]) -> bool {
    any_word(words, Test::Name, &RUNNERS)
        || any_word(words, Test::Versioned, &VERSIONED_RUNNERS)
        || head_listed(words, &HEAD_RUNNERS)
        || head_assigns(words)
        || (head_is(words, &FIND) && any_word(words, Test::Flag, &FIND_RUNS))
        || is_git_runner(words)
}

fn is_network(words: &[Vec<u8>]) -> bool {
    any_word(words, Test::Name, &NETWORK)
}

/// The "never always" commands. They get `ask` at most, even from the config.
pub(crate) fn is_capped(words: &[Vec<u8>]) -> bool {
    is_runner(words)
        || is_network(words)
        || any_word(words, Test::Name, &NEVER_ALWAYS)
        || (head_is(words, &RM) && any_word(words, Test::Recursive, &NO_LIST))
        || (head_is(words, &GIT) && any_word(words, Test::GitForce, &NO_LIST))
}

pub(crate) fn is_desktop(simple: &Simple) -> bool {
    any_word(&simple.words, Test::Name, &DESKTOP_NAMES)
        || (simple.link == Link::Pipe && any_word(&simple.words, Test::Name, &SHELLS))
}

/// A rule is a list of words that must start the command, as `cargo test` does
/// for `cargo test -q`. An empty rule matches nothing.
fn rule_matches(rule: &[Vec<u8>], words: &[Vec<u8>]) -> bool {
    if rule.len() == 0 || rule.len() > words.len() {
        return false;
    }
    let mut same = true;
    let mut k = 0;
    while same && k < rule.len() {
        same = bytes_equal(&rule[k], &words[k]);
        k += 1;
    }
    same
}

pub(crate) fn matches_any(rules: &[Vec<Vec<u8>>], words: &[Vec<u8>]) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < rules.len() {
        found = rule_matches(&rules[i], words);
        i += 1;
    }
    found
}

pub(crate) fn is_covered(
    words: &[Vec<u8>],
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    cover: Cover,
) -> bool {
    matches_any(&policy.allow, words) || cover == Cover::Every || matches_any(rules, words)
}

pub(crate) fn simple_verdict(
    simple: &Simple,
    policy: &Policy,
    rules: &[Vec<Vec<u8>>],
    cover: Cover,
) -> Verdict {
    if is_desktop(simple) {
        return Verdict::Desktop;
    }
    if is_capped(&simple.words) {
        return Verdict::Ask;
    }
    if is_covered(&simple.words, policy, rules, cover) {
        Verdict::Allow
    } else {
        Verdict::Ask
    }
}

/// `cd`, `pushd`, and `popd` change the folder of later commands.
pub(crate) fn changes_folder(simples: &[Simple]) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < simples.len() {
        found = head_listed(&simples[i].words, &FOLDER_CHANGES);
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(word: &str) -> String {
        String::from_utf8(name_of(word.as_bytes())).unwrap()
    }

    fn simple(words: &[&str], link: Link) -> Simple {
        Simple {
            words: words.iter().map(|w| w.as_bytes().to_vec()).collect(),
            link,
        }
    }

    fn policy(allow: &[&[&str]]) -> Policy {
        Policy {
            roots: Vec::new(),
            chat: Vec::new(),
            deny_folders: Vec::new(),
            desktop_paths: Vec::new(),
            desktop_writes: Vec::new(),
            allow: allow
                .iter()
                .map(|r| r.iter().map(|w| w.as_bytes().to_vec()).collect())
                .collect(),
        }
    }

    fn verdict(words: &[&str]) -> Verdict {
        let s = simple(words, Link::First);
        simple_verdict(&s, &policy(&[&[]]), &[], Cover::Every)
    }

    #[test]
    fn a_name_loses_its_folder_its_case_and_exe() {
        assert_eq!(name("/usr/bin/SUDO.exe"), "sudo");
        assert_eq!(name(r"C:\Windows\cmd.EXE"), "cmd");
        assert_eq!(name("a/"), "");
        assert_eq!(name(".exe"), "");
        assert_eq!(name("exe"), "exe");
        assert_eq!(name("a.exf"), "a.exf");
    }

    #[test]
    fn a_flag_is_the_part_before_the_equals_sign() {
        assert_eq!(flag_of(b"--upload-pack=x"), b"--upload-pack");
        assert_eq!(flag_of(b"-c"), b"-c");
    }

    #[test]
    fn eval_sudo_cmd_and_powershell_are_desktop_in_any_word() {
        assert_eq!(verdict(&["eval", "x"]), Verdict::Desktop);
        assert_eq!(verdict(&["timeout", "5", "sudo", "ls"]), Verdict::Desktop);
        assert_eq!(verdict(&["cmd.exe", "/c", "dir"]), Verdict::Desktop);
        assert_eq!(verdict(&["pwsh", "-c", "x"]), Verdict::Desktop);
        assert_eq!(verdict(&["PowerShell.exe"]), Verdict::Desktop);
    }

    #[test]
    fn a_shell_after_a_pipe_is_desktop() {
        let s = simple(&["sh"], Link::Pipe);
        assert_eq!(
            simple_verdict(&s, &policy(&[]), &[], Cover::Every),
            Verdict::Desktop
        );
        let s = simple(&["xargs", "bash"], Link::Pipe);
        assert_eq!(
            simple_verdict(&s, &policy(&[]), &[], Cover::Every),
            Verdict::Desktop
        );
    }

    #[test]
    fn a_shell_with_no_pipe_is_a_runner() {
        assert_eq!(verdict(&["bash", "-c", "rm -rf /"]), Verdict::Ask);
    }

    #[test]
    fn network_tools_ask_even_with_a_rule() {
        assert_eq!(verdict(&["curl", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["/usr/bin/wget", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["xargs", "ssh"]), Verdict::Ask);
    }

    #[test]
    fn commands_that_run_commands_ask_even_with_a_rule() {
        assert_eq!(
            verdict(&["find", ".", "-exec", "rm", "{}", ";"]),
            Verdict::Ask
        );
        assert_eq!(verdict(&["find", ".", "-delete"]), Verdict::Ask);
        assert_eq!(
            verdict(&["git", "-c", "core.sshCommand=x", "push"]),
            Verdict::Ask
        );
        assert_eq!(verdict(&["git", "fetch", "--upload-pack=x"]), Verdict::Ask);
        assert_eq!(verdict(&["env", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["python3", "-c", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["node", "-e", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["perl", "-e", "x"]), Verdict::Ask);
        assert_eq!(verdict(&[".", "x.sh"]), Verdict::Ask);
        assert_eq!(verdict(&["A=1", "cargo", "test"]), Verdict::Ask);
    }

    #[test]
    fn pypy_and_versioned_interpreters_are_runners() {
        assert_eq!(verdict(&["pypy3", "x.py"]), Verdict::Ask);
        assert_eq!(verdict(&["python3.12", "-c", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["/usr/bin/node18", "x.js"]), Verdict::Ask);
        assert_eq!(verdict(&["xargs", "lua5.4"]), Verdict::Ask);
        assert_eq!(verdict(&["pypy3.10", "x.py"]), Verdict::Ask);
        assert_eq!(verdict(&["python3", "-m", "pytest"]), Verdict::Ask);
    }

    #[test]
    fn a_name_with_digits_but_no_listed_stem_is_not_a_runner() {
        assert_eq!(verdict(&["ls", "3.12"]), Verdict::Allow);
        assert_eq!(verdict(&["sha256sum", "x"]), Verdict::Allow);
        assert_eq!(verdict(&["cat", "nodes2"]), Verdict::Allow);
    }

    #[test]
    fn open_is_a_runner_only_as_the_command_name() {
        assert_eq!(verdict(&["open", "x.command"]), Verdict::Ask);
        assert_eq!(verdict(&["/usr/bin/open", "-a", "Terminal"]), Verdict::Ask);
        assert_eq!(verdict(&["grep", "-rn", "open", "src"]), Verdict::Allow);
        assert_eq!(verdict(&["man", "open"]), Verdict::Allow);
    }

    #[test]
    fn git_commands_that_run_a_program_are_runners() {
        assert_eq!(verdict(&["git", "submodule", "foreach", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "bisect", "run", "x"]), Verdict::Ask);
        assert_eq!(
            verdict(&["git", "filter-branch", "--tree-filter", "x"]),
            Verdict::Ask
        );
        assert_eq!(verdict(&["git", "difftool", "-x", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "mergetool"]), Verdict::Ask);
        assert_eq!(
            verdict(&["git", "grep", "--open-files-in-pager=x", "a"]),
            Verdict::Ask
        );
        assert_eq!(verdict(&["git", "grep", "-Ox", "a"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "diff", "--extcmd=x"]), Verdict::Ask);
    }

    #[test]
    fn a_start_of_a_long_git_flag_is_the_flag() {
        assert_eq!(verdict(&["git", "fetch", "--upload=x"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "grep", "--open-files", "a"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "--exec-p=x", "status"]), Verdict::Ask);
    }

    #[test]
    fn common_git_commands_are_not_runners() {
        assert_eq!(verdict(&["git", "log", "--oneline"]), Verdict::Allow);
        assert_eq!(verdict(&["git", "log", "--", "src"]), Verdict::Allow);
        assert_eq!(verdict(&["git", "status"]), Verdict::Allow);
    }

    #[test]
    fn a_wrapper_cannot_hide_a_never_always_command() {
        assert_eq!(verdict(&["strace", "rm", "-rf", "x"]), Verdict::Ask);
        assert_eq!(verdict(&["flock", "f", "git", "push", "-f"]), Verdict::Ask);
    }

    #[test]
    fn builtins_that_change_later_commands_ask() {
        assert_eq!(verdict(&["export", "PATH=/tmp"]), Verdict::Ask);
        assert_eq!(verdict(&["trap", "x", "EXIT"]), Verdict::Ask);
        assert_eq!(verdict(&["echo", "set"]), Verdict::Allow);
    }

    #[test]
    fn a_change_of_user_is_desktop() {
        assert_eq!(verdict(&["sudoedit", "x"]), Verdict::Desktop);
        assert_eq!(verdict(&["gsudo", "x"]), Verdict::Desktop);
    }

    #[test]
    fn a_dot_argument_is_not_a_runner() {
        assert_eq!(verdict(&["git", "add", "."]), Verdict::Allow);
    }

    #[test]
    fn never_always_commands_ask_even_with_a_rule() {
        assert_eq!(verdict(&["rm", "-rf", "target"]), Verdict::Ask);
        assert_eq!(verdict(&["rm", "-R", "target"]), Verdict::Ask);
        assert_eq!(verdict(&["chmod", "+x", "a"]), Verdict::Ask);
        assert_eq!(verdict(&["chown", "a", "b"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "push", "--force"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "push", "-uf", "o", "m"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "push", "o", "+main"]), Verdict::Ask);
        assert_eq!(verdict(&["git", "reset", "--hard"]), Verdict::Ask);
    }

    #[test]
    fn plain_rm_and_git_can_be_allowed() {
        assert_eq!(verdict(&["rm", "a.txt"]), Verdict::Allow);
        assert_eq!(
            verdict(&["git", "push", "--set-upstream", "o", "m"]),
            Verdict::Allow
        );
    }

    #[test]
    fn a_rule_covers_the_commands_that_start_with_its_words() {
        let s = simple(&["cargo", "test", "-q"], Link::First);
        let rules = [vec![b"cargo".to_vec(), b"test".to_vec()]];
        let v = simple_verdict(&s, &policy(&[]), &rules, Cover::Listed);
        assert_eq!(v, Verdict::Allow);
        let s = simple(&["cargo", "run"], Link::First);
        let v = simple_verdict(&s, &policy(&[]), &rules, Cover::Listed);
        assert_eq!(v, Verdict::Ask);
    }

    #[test]
    fn the_allow_table_of_the_config_covers_a_command() {
        let s = simple(&["cargo", "test"], Link::First);
        let v = simple_verdict(&s, &policy(&[&["cargo"]]), &[], Cover::Listed);
        assert_eq!(v, Verdict::Allow);
    }

    #[test]
    fn an_empty_rule_covers_nothing() {
        let s = simple(&["ls"], Link::First);
        let v = simple_verdict(&s, &policy(&[&[]]), &[Vec::new()], Cover::Listed);
        assert_eq!(v, Verdict::Ask);
    }

    #[test]
    fn a_rule_longer_than_the_command_covers_nothing() {
        let s = simple(&["cargo"], Link::First);
        let rules = [vec![b"cargo".to_vec(), b"test".to_vec()]];
        let v = simple_verdict(&s, &policy(&[]), &rules, Cover::Listed);
        assert_eq!(v, Verdict::Ask);
    }

    #[test]
    fn cd_pushd_and_popd_change_the_folder() {
        assert!(changes_folder(&[simple(&["cd", "x"], Link::First)]));
        assert!(changes_folder(&[
            simple(&["ls"], Link::First),
            simple(&["popd"], Link::First)
        ]));
        assert!(!changes_folder(&[simple(&["ls", "cd"], Link::First)]));
    }
}
