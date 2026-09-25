//! The path rules of the action classifier. See `SPEC.md` 6.6.3.
//!
//! A path arrives resolved: the bridge ran `canonicalize` on it, and it has the form
//! of `resolve_folder` (S5), for example `/home/x/Code` or `/C:/Users/x/Code`.
//! A path in any other form is `desktop`.

use crate::action::{Policy, Verdict};
use crate::ascii::{bytes_equal, to_lower};
use crate::folder::{inside_any, is_dot, is_dot_dot, is_prefix, join, resolve_folder, split_parts};
use crate::search::{equal_run, has_byte};
use crate::shell::Access;

/// The longest path that the rules check: 1 MiB.
pub const MAX_PATH: usize = 1_048_576;

/// A redirect to `/dev/null` writes nothing.
const DEV_NULL: [u8; 9] = *b"/dev/null";

/// macOS and Windows compare paths without case, so `~/.SSH` is `~/.ssh` there.
/// The `deny` and `desktop` rules compare without ASCII case on every OS.
fn lower_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        out.push(to_lower(bytes[i]));
        i += 1;
    }
    out
}

fn lower_all(list: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        out.push(lower_bytes(&list[i]));
        i += 1;
    }
    out
}

fn is_named_part(part: &[u8]) -> bool {
    !is_dot(part) && !is_dot_dot(part)
}

fn no_dot_parts(parts: &[Vec<u8>]) -> bool {
    let mut clean = true;
    let mut i = 0;
    while clean && i < parts.len() {
        clean = is_named_part(&parts[i]);
        i += 1;
    }
    clean
}

/// Starts with `/`, has no empty part, no `.` or `..`, and no trailing `/`.
fn is_clean(path: &[u8]) -> bool {
    if path.len() > MAX_PATH {
        return false;
    }
    let parts = split_parts(path);
    parts.len() > 0 && no_dot_parts(&parts) && bytes_equal(&join(&parts, parts.len()), path)
}

/// A pattern part that ends with `*` matches every part that starts with the rest,
/// so `.env*` matches `.env.local`.
fn part_matches(pattern_part: &[u8], part: &[u8]) -> bool {
    let n = pattern_part.len();
    if n > 0 && pattern_part[n - 1] == b'*' {
        return n - 1 <= part.len() && equal_run(part, 0, pattern_part, n - 1);
    }
    bytes_equal(pattern_part, part)
}

/// The caller keeps `at + pattern.len() <= parts.len()`.
fn pattern_at(pattern: &[Vec<u8>], parts: &[Vec<u8>], at: usize) -> bool {
    let mut same = true;
    let mut k = 0;
    while same && k < pattern.len() {
        same = part_matches(&pattern[k], &parts[at + k]);
        k += 1;
    }
    same
}

/// The parts of the pattern appear in a row somewhere in `parts`.
fn pattern_in(pattern: &[Vec<u8>], parts: &[Vec<u8>]) -> bool {
    if pattern.len() > parts.len() {
        return false;
    }
    let last = parts.len() - pattern.len();
    let mut found = false;
    let mut at = 0;
    while !found && at <= last {
        found = pattern_at(pattern, parts, at);
        at += 1;
    }
    found
}

fn matches_any_pattern(path: &[u8], patterns: &[Vec<u8>]) -> bool {
    let parts = split_parts(&lower_bytes(path));
    let mut found = false;
    let mut i = 0;
    while !found && i < patterns.len() {
        found = pattern_in(&split_parts(&lower_bytes(&patterns[i])), &parts);
        i += 1;
    }
    found
}

fn is_denied(path: &[u8], folders: &[Vec<u8>]) -> bool {
    let parts = split_parts(&lower_bytes(path));
    inside_any(&lower_all(folders), &parts, parts.len())
}

fn path_allowed(path: &[u8], access: Access, policy: &Policy) -> bool {
    if !is_clean(path) || matches_any_pattern(path, &policy.desktop_paths) {
        return false;
    }
    let parts = split_parts(path);
    if access == Access::Read {
        return inside_any(&policy.roots, &parts, parts.len());
    }
    is_prefix(&split_parts(&policy.chat), &parts, parts.len())
        && !matches_any_pattern(path, &policy.desktop_writes)
}

pub(crate) fn path_verdict(path: &[u8], access: Access, policy: &Policy) -> Verdict {
    if is_denied(path, &policy.deny_folders) {
        return Verdict::Deny;
    }
    if path_allowed(path, access, policy) {
        Verdict::Allow
    } else {
        Verdict::Desktop
    }
}

fn has_glob(bytes: &[u8]) -> bool {
    has_byte(bytes, b'*') || has_byte(bytes, b'?') || has_byte(bytes, b'[')
}

/// The shell expands `~` and globs before it opens the file, and this check cannot.
fn is_literal_target(target: &[u8]) -> bool {
    target.len() > 0 && target[0] != b'~' && !has_glob(target)
}

fn top_root() -> Vec<Vec<u8>> {
    let mut slash = Vec::new();
    slash.push(b'/');
    let mut roots = Vec::new();
    roots.push(slash);
    roots
}

/// A redirect target is relative to the working folder of the command.
pub(crate) fn target_verdict(
    target: &[u8],
    access: Access,
    cwd: &[u8],
    policy: &Policy,
) -> Verdict {
    if bytes_equal(target, &DEV_NULL) {
        return Verdict::Allow;
    }
    if !is_literal_target(target) || cwd.len() > MAX_PATH || target.len() > MAX_PATH - cwd.len() {
        return Verdict::Desktop;
    }
    let Some(path) = resolve_folder(&top_root(), cwd, target) else {
        return Verdict::Desktop;
    };
    path_verdict(&path, access, policy)
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
            desktop_writes: vec![b".git/hooks".to_vec(), b".claude".to_vec()],
            allow: Vec::new(),
        }
    }

    fn read(path: &str) -> Verdict {
        path_verdict(path.as_bytes(), Access::Read, &policy())
    }

    fn write(path: &str) -> Verdict {
        path_verdict(path.as_bytes(), Access::Write, &policy())
    }

    #[test]
    fn a_read_inside_a_root_is_allowed() {
        assert_eq!(read("/home/x/Code/lib/a.rs"), Verdict::Allow);
    }

    #[test]
    fn a_read_outside_every_root_is_desktop() {
        assert_eq!(read("/etc/passwd"), Verdict::Desktop);
        assert_eq!(read("/home/x/Code2/a"), Verdict::Desktop);
    }

    #[test]
    fn a_write_inside_the_chat_folder_is_allowed() {
        assert_eq!(write("/home/x/Code/app/src/a.rs"), Verdict::Allow);
    }

    #[test]
    fn a_write_outside_the_chat_folder_is_desktop() {
        assert_eq!(write("/home/x/Code/lib/a.rs"), Verdict::Desktop);
    }

    #[test]
    fn every_path_in_the_config_folder_is_denied() {
        assert_eq!(
            read("/home/x/.config/gnomish-relay/strip.key"),
            Verdict::Deny
        );
        assert_eq!(
            write("/home/x/.config/gnomish-relay/config.toml"),
            Verdict::Deny
        );
        assert_eq!(read("/home/x/.config/gnomish-relay"), Verdict::Deny);
    }

    #[test]
    fn the_config_folder_is_denied_in_any_letter_case() {
        assert_eq!(
            read("/home/x/.CONFIG/Gnomish-Relay/strip.key"),
            Verdict::Deny
        );
    }

    #[test]
    fn a_desktop_path_is_desktop_for_reads_and_writes() {
        assert_eq!(read("/home/x/Code/app/.ssh/id"), Verdict::Desktop);
        assert_eq!(write("/home/x/Code/app/.ssh/id"), Verdict::Desktop);
    }

    #[test]
    fn a_desktop_path_matches_in_any_letter_case() {
        assert_eq!(read("/home/x/Code/app/.SSH/id"), Verdict::Desktop);
    }

    #[test]
    fn a_star_matches_the_rest_of_a_part() {
        assert_eq!(read("/home/x/Code/app/.env"), Verdict::Desktop);
        assert_eq!(read("/home/x/Code/app/.env.local"), Verdict::Desktop);
        assert_eq!(read("/home/x/Code/app/env"), Verdict::Allow);
    }

    #[test]
    fn a_desktop_write_path_is_desktop_only_for_writes() {
        assert_eq!(
            write("/home/x/Code/app/.git/hooks/pre-commit"),
            Verdict::Desktop
        );
        assert_eq!(
            read("/home/x/Code/app/.git/hooks/pre-commit"),
            Verdict::Allow
        );
        assert_eq!(
            write("/home/x/Code/app/.claude/settings.json"),
            Verdict::Desktop
        );
    }

    #[test]
    fn a_pattern_matches_whole_parts_in_a_row() {
        assert_eq!(write("/home/x/Code/app/.git/x/hooks"), Verdict::Allow);
        assert_eq!(write("/home/x/Code/app/.claude2/a"), Verdict::Allow);
    }

    #[test]
    fn a_path_that_is_not_clean_is_desktop() {
        assert_eq!(read("/home/x/Code/app/../../.ssh"), Verdict::Desktop);
        assert_eq!(read("/home/x/Code/./a"), Verdict::Desktop);
        assert_eq!(read("/home/x//Code/a"), Verdict::Desktop);
        assert_eq!(read("/home/x/Code/a/"), Verdict::Desktop);
        assert_eq!(read("home/x/Code/a"), Verdict::Desktop);
        assert_eq!(read(""), Verdict::Desktop);
    }

    #[test]
    fn a_path_over_the_limit_is_desktop() {
        let mut long = b"/home/x/Code/".to_vec();
        long.resize(MAX_PATH + 1, b'a');
        assert_eq!(
            path_verdict(&long, Access::Read, &policy()),
            Verdict::Desktop
        );
    }

    #[test]
    fn a_windows_path_has_its_drive_as_the_first_part() {
        let mut p = policy();
        p.roots = vec![b"/C:/Users/x/Code".to_vec()];
        let v = path_verdict(b"/C:/Users/x/Code/a", Access::Read, &p);
        assert_eq!(v, Verdict::Allow);
        let v = path_verdict(b"/D:/Users/x/Code/a", Access::Read, &p);
        assert_eq!(v, Verdict::Desktop);
    }

    fn target(t: &str, access: Access) -> Verdict {
        target_verdict(t.as_bytes(), access, b"/home/x/Code/app", &policy())
    }

    #[test]
    fn a_relative_target_resolves_from_the_working_folder() {
        assert_eq!(target("out.txt", Access::Write), Verdict::Allow);
        assert_eq!(target("../lib/a", Access::Write), Verdict::Desktop);
        assert_eq!(target("../lib/a", Access::Read), Verdict::Allow);
    }

    #[test]
    fn dev_null_is_always_allowed() {
        assert_eq!(target("/dev/null", Access::Write), Verdict::Allow);
    }

    #[test]
    fn a_target_with_a_tilde_or_a_glob_is_desktop() {
        assert_eq!(target("~/.bashrc", Access::Write), Verdict::Desktop);
        assert_eq!(target("a*", Access::Write), Verdict::Desktop);
        assert_eq!(target("a?", Access::Write), Verdict::Desktop);
        assert_eq!(target("[a]", Access::Write), Verdict::Desktop);
        assert_eq!(target("", Access::Write), Verdict::Desktop);
    }

    #[test]
    fn a_target_into_the_config_folder_is_denied() {
        let v = target("/home/x/.config/gnomish-relay/config.toml", Access::Write);
        assert_eq!(v, Verdict::Deny);
    }

    #[test]
    fn a_target_that_climbs_above_the_top_is_desktop() {
        assert_eq!(target("../../../../../..", Access::Read), Verdict::Desktop);
    }

    #[test]
    fn a_target_over_the_limit_is_desktop() {
        let long = vec![b'a'; MAX_PATH];
        let v = target_verdict(&long, Access::Write, b"/home/x/Code/app", &policy());
        assert_eq!(v, Verdict::Desktop);
    }
}
