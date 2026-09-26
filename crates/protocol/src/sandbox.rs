//! The policy of the sandbox for the commands of a run from the game. See `SPEC.md`
//! 6.6.4 and 14.1, S31.
//!
//! A path is hidden with the predicate of the classifier (6.6.3): inside a `deny` folder,
//! or a run of its parts matches a `desktop` pattern, with no regard to ASCII case.

use crate::ascii::copy_bytes;
use crate::path_rules::{is_clean, is_denied, matches_any_pattern};

/// Commands have none. The agent process runs outside the sandbox and reaches its API.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Network {
    Off,
}

/// Every path that is not writable and not hidden is read-only.
pub struct SandboxPolicy {
    /// The chat folder and the private temp folder, unless a folder is hidden.
    pub writable: Vec<Vec<u8>>,
    /// The `deny` folders.
    pub hidden_folders: Vec<Vec<u8>>,
    /// The `desktop` patterns for reads and writes.
    pub hidden_paths: Vec<Vec<u8>>,
    /// The `desktop` patterns for writes. Commands only read them, so they hide too.
    pub hidden_writes: Vec<Vec<u8>>,
    pub network: Network,
}

fn copy_all(list: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        out.push(copy_bytes(&list[i]));
        i += 1;
    }
    out
}

#[must_use]
pub fn is_hidden(policy: &SandboxPolicy, path: &[u8]) -> bool {
    is_denied(path, &policy.hidden_folders)
        || matches_any_pattern(path, &policy.hidden_paths)
        || matches_any_pattern(path, &policy.hidden_writes)
}

/// A path of another form has no exact "inside", so it never becomes writable.
fn can_write(policy: &SandboxPolicy, path: &[u8]) -> bool {
    is_clean(path) && !is_hidden(policy, path)
}

fn keep_writable(mut list: Vec<Vec<u8>>, policy: &SandboxPolicy, path: &[u8]) -> Vec<Vec<u8>> {
    if can_write(policy, path) {
        list.push(copy_bytes(path));
    }
    list
}

/// The bridge refuses a run when `chat` is not in `writable`.
#[must_use]
pub fn sandbox_policy(
    chat: &[u8],
    temp: &[u8],
    deny_folders: &[Vec<u8>],
    desktop_paths: &[Vec<u8>],
    desktop_writes: &[Vec<u8>],
) -> SandboxPolicy {
    let mut policy = SandboxPolicy {
        writable: Vec::new(),
        hidden_folders: copy_all(deny_folders),
        hidden_paths: copy_all(desktop_paths),
        hidden_writes: copy_all(desktop_writes),
        network: Network::Off,
    };
    let writable = keep_writable(Vec::new(), &policy, chat);
    policy.writable = keep_writable(writable, &policy, temp);
    policy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    fn policy(chat: &str) -> SandboxPolicy {
        sandbox_policy(
            chat.as_bytes(),
            b"/tmp/gnomish-relay-run-1",
            &list(&["/home/x/.config/gnomish-relay"]),
            &list(&[".ssh", ".env.*"]),
            &list(&[".git/hooks"]),
        )
    }

    #[test]
    fn the_chat_folder_and_the_temp_folder_are_writable() {
        let p = policy("/home/x/Code/app");
        assert_eq!(
            p.writable,
            list(&["/home/x/Code/app", "/tmp/gnomish-relay-run-1"])
        );
        assert_eq!(p.network, Network::Off);
    }

    #[test]
    fn the_deny_folders_and_both_pattern_lists_are_hidden() {
        let p = policy("/home/x/Code/app");
        assert!(is_hidden(&p, b"/home/x/.config/gnomish-relay/strip.key"));
        assert!(is_hidden(&p, b"/home/x/.SSH/id_ed25519"));
        assert!(is_hidden(&p, b"/home/x/Code/app/.env.local"));
        assert!(is_hidden(&p, b"/home/x/Code/app/.git/hooks/pre-commit"));
        assert!(!is_hidden(&p, b"/home/x/Code/app/.git/config2"));
        assert!(!is_hidden(&p, b"/home/x/.config/gnomish-relay2"));
    }

    #[test]
    fn a_chat_folder_inside_a_hidden_path_is_not_writable() {
        assert_eq!(
            policy("/home/x/.ssh/app").writable,
            list(&["/tmp/gnomish-relay-run-1"])
        );
        assert_eq!(
            policy("/home/x/.config/gnomish-relay").writable,
            list(&["/tmp/gnomish-relay-run-1"])
        );
    }

    #[test]
    fn a_chat_folder_that_is_not_clean_is_not_writable() {
        assert_eq!(
            policy("/home/x/Code/../.ssh").writable,
            list(&["/tmp/gnomish-relay-run-1"])
        );
        assert_eq!(
            policy("home/x/Code").writable,
            list(&["/tmp/gnomish-relay-run-1"])
        );
    }

    #[test]
    fn a_hidden_path_inside_the_chat_folder_keeps_the_folder_writable() {
        let p = policy("/home/x");
        assert_eq!(p.writable[0], b"/home/x");
        assert!(is_hidden(&p, b"/home/x/.ssh"));
    }
}
