//! The input of the action classifier (SPEC.md 6.6.3): the policy from the config, and
//! the resolved paths of a tool call. The backends call the classifier later.

use std::path::{Path, PathBuf};

use protocol::action::{Policy, ToolCall};

use crate::config::path_bytes;

/// Credentials: reads and writes ask on the desktop. Each is a list of whole parts that
/// match anywhere in a path, without ASCII case. A last `*` matches the rest of a part.
pub const DESKTOP_PATHS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".env",
    ".env.*",
    ".netrc",
    ".git-credentials",
    ".config/gh",
    ".docker/config.json",
    ".kube",
    // Keychains
    "Library/Keychains",
    ".local/share/keyrings",
    ".password-store",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Local/Microsoft/Credentials",
    // Browser profiles
    ".mozilla",
    "snap/firefox",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/microsoft-edge",
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Firefox",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Microsoft Edge",
    "Library/Safari",
    "Library/Cookies",
    "AppData/Local/Google/Chrome",
    "AppData/Roaming/Mozilla",
    "AppData/Local/BraveSoftware",
    "AppData/Local/Microsoft/Edge",
];

/// Files that code on the host runs later, outside the sandbox: writes ask on the desktop.
pub const DESKTOP_WRITES: &[&str] = &[
    ".claude",
    ".git/hooks",
    ".git/config",
    ".envrc",
    ".vscode",
    ".github/workflows",
];

/// The resolver of S5 starts a path with `/`. On Windows the drive is the first part.
fn with_leading_slash(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.first() == Some(&b'/') {
        return bytes;
    }
    [b"/".as_slice(), &bytes].concat()
}

/// A path that `resolve` returned, in the form that the classifier takes.
pub fn resolved_bytes(path: &Path) -> Vec<u8> {
    with_leading_slash(path_bytes(path))
}

/// `canonicalize` resolves every link. A new file resolves through its nearest folder
/// that exists, so a write to a link inside the chat folder shows its real target. The
/// missing parts cannot be links, because they do not exist. A `..` among them gives `None`.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(real) = existing.canonicalize() {
            return Some(missing.iter().rev().fold(real, |p, part| p.join(part)));
        }
        missing.push(existing.file_name()?);
        existing = existing.parent()?;
    }
}

fn patterns(list: &[&str]) -> Vec<Vec<u8>> {
    list.iter().map(|p| p.as_bytes().to_vec()).collect()
}

/// `roots`, `chat`, and `deny` are resolved. `deny` holds the config folder and the data
/// folder of the bridge. `allow` is the allow table of the config: each rule is the first
/// words of a command.
pub fn policy(roots: &[PathBuf], chat: &Path, deny: &[PathBuf], allow: &[Vec<String>]) -> Policy {
    Policy {
        roots: roots.iter().map(|r| resolved_bytes(r)).collect(),
        chat: resolved_bytes(chat),
        deny_folders: deny.iter().map(|d| resolved_bytes(d)).collect(),
        desktop_paths: patterns(DESKTOP_PATHS),
        desktop_writes: patterns(DESKTOP_WRITES),
        allow: allow
            .iter()
            .map(|rule| rule.iter().map(|w| w.as_bytes().to_vec()).collect())
            .collect(),
    }
}

/// A path that does not resolve keeps its text. The classifier then answers `desktop`
/// unless the text is a clean path inside the folders.
fn call_path(path: &Path) -> Vec<u8> {
    match resolve(path) {
        Some(real) => resolved_bytes(&real),
        None => path_bytes(path),
    }
}

pub fn file_call(reads: &[PathBuf], writes: &[PathBuf]) -> ToolCall {
    ToolCall::Files {
        reads: reads.iter().map(|p| call_path(p)).collect(),
        writes: writes.iter().map(|p| call_path(p)).collect(),
    }
}

pub fn command_call(command: &str, cwd: &Path) -> ToolCall {
    ToolCall::Command {
        raw: command.as_bytes().to_vec(),
        cwd: call_path(cwd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::action::{Verdict, classify};

    struct Folders {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        chat: PathBuf,
        config: PathBuf,
        data: PathBuf,
    }

    fn folders() -> Folders {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("Code");
        let chat = root.join("app");
        let config = base.join("config").join("gnomish-relay");
        let data = base.join("data").join("gnomish-relay");
        std::fs::create_dir_all(&chat).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        Folders {
            _tmp: tmp,
            root,
            chat,
            config,
            data,
        }
    }

    fn deny(f: &Folders) -> [PathBuf; 2] {
        [f.config.clone(), f.data.clone()]
    }

    /// `Verdict` has no `Debug` outside the tests of `protocol`.
    fn answer(v: Verdict) -> &'static str {
        match v {
            Verdict::Deny => "deny",
            Verdict::Desktop => "desktop",
            Verdict::Ask => "ask",
            Verdict::Allow => "allow",
        }
    }

    fn classify_files(f: &Folders, reads: &[PathBuf], writes: &[PathBuf]) -> &'static str {
        let policy = policy(std::slice::from_ref(&f.root), &f.chat, &deny(f), &[]);
        answer(classify(&file_call(reads, writes), &policy, &[]))
    }

    #[test]
    fn a_path_gets_a_leading_slash_before_a_drive() {
        assert_eq!(with_leading_slash(b"C:/Users/x".to_vec()), b"/C:/Users/x");
        assert_eq!(with_leading_slash(b"/home/x".to_vec()), b"/home/x");
    }

    #[test]
    fn a_new_file_resolves_through_its_folder() {
        let f = folders();
        let new = f.chat.join("new.rs");
        assert_eq!(resolve(&new), Some(f.chat.join("new.rs")));
    }

    #[test]
    fn a_file_in_a_missing_folder_resolves_through_its_nearest_folder() {
        let f = folders();
        let new = f.chat.join("no").join("x");
        assert_eq!(resolve(&new), Some(f.chat.join("no").join("x")));
    }

    // Windows removes a `..` by its text before it opens a path, so there the path is
    // `chat/x`, which is also the file that the OS opens.
    #[cfg(unix)]
    #[test]
    fn a_dot_dot_in_the_missing_parts_does_not_resolve() {
        let f = folders();
        assert_eq!(resolve(&f.chat.join("no").join("..").join("x")), None);
    }

    #[test]
    fn a_write_inside_the_chat_folder_is_allowed() {
        let f = folders();
        let v = classify_files(&f, &[], &[f.chat.join("src.rs")]);
        assert_eq!(v, "allow");
    }

    #[test]
    fn a_read_inside_a_root_is_allowed() {
        let f = folders();
        let v = classify_files(&f, &[f.root.join("lib.rs")], &[]);
        assert_eq!(v, "allow");
    }

    #[test]
    fn a_path_in_the_config_folder_is_denied() {
        let f = folders();
        let v = classify_files(&f, &[f.config.join("strip.key")], &[]);
        assert_eq!(v, "deny");
    }

    #[test]
    fn a_read_of_the_timeways_key_in_the_config_folder_is_denied() {
        let f = folders();
        let v = classify_files(&f, &[f.config.join("timeways.key")], &[]);
        assert_eq!(v, "deny");
    }

    /// The bridge writes these files itself. An agent that changed one could clear the
    /// replay store, answer its own desktop request, or change the story state.
    fn data_file_is_denied_for_reads_and_writes(f: &Folders, file: &Path) {
        assert_eq!(classify_files(f, &[file.to_owned()], &[]), "deny");
        assert_eq!(classify_files(f, &[], &[file.to_owned()]), "deny");
    }

    #[test]
    fn an_agent_access_to_the_state_file_is_denied() {
        let f = folders();
        data_file_is_denied_for_reads_and_writes(&f, &f.data.join("state.json"));
    }

    #[test]
    fn an_agent_access_to_a_desktop_approval_is_denied() {
        let f = folders();
        let approvals = f.data.join("approvals");
        std::fs::create_dir_all(&approvals).unwrap();
        data_file_is_denied_for_reads_and_writes(&f, &approvals.join("a1b2c3d4e5f6.json"));
        data_file_is_denied_for_reads_and_writes(&f, &approvals.join("a1b2c3d4e5f6.answer"));
    }

    #[test]
    fn an_agent_access_to_the_timeways_state_is_denied() {
        let f = folders();
        data_file_is_denied_for_reads_and_writes(&f, &f.data.join("timeways").join("state.json"));
    }

    #[test]
    fn an_agent_access_to_the_bridge_lock_is_denied() {
        let f = folders();
        data_file_is_denied_for_reads_and_writes(&f, &f.data.join("bridge.lock"));
    }

    #[test]
    fn an_agent_access_to_the_bridge_pid_is_denied() {
        let f = folders();
        data_file_is_denied_for_reads_and_writes(&f, &f.data.join("bridge.pid"));
    }

    #[test]
    fn an_agent_access_to_the_bridge_log_is_denied() {
        let f = folders();
        data_file_is_denied_for_reads_and_writes(&f, &f.data.join("bridge.log"));
    }

    #[test]
    fn a_command_that_redirects_into_the_data_folder_is_denied() {
        let f = folders();
        let policy = policy(std::slice::from_ref(&f.root), &f.chat, &deny(&f), &[]);
        // A relative target has the same form on every OS. A Windows path such as
        // `C:\x` does not resolve in a shell command, so it is `desktop` there.
        let call = command_call("echo x > ../../data/gnomish-relay/state.json", &f.chat);
        assert_eq!(answer(classify(&call, &policy, &[])), "deny");
    }

    #[test]
    fn a_git_hook_write_is_desktop() {
        let f = folders();
        std::fs::create_dir_all(f.chat.join(".git").join("hooks")).unwrap();
        let hook = f.chat.join(".git").join("hooks").join("pre-commit");
        assert_eq!(classify_files(&f, &[], &[hook]), "desktop");
    }

    #[test]
    fn an_env_file_is_desktop_for_reads() {
        let f = folders();
        let v = classify_files(&f, &[f.chat.join(".env.local")], &[]);
        assert_eq!(v, "desktop");
    }

    #[test]
    fn a_relative_path_that_does_not_resolve_is_desktop() {
        let f = folders();
        let v = classify_files(&f, &[PathBuf::from("no/such/file")], &[]);
        assert_eq!(v, "desktop");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_chat_folder_resolves_to_its_target() {
        let f = folders();
        let outside = f.root.join("other");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, f.chat.join("link")).unwrap();
        let v = classify_files(&f, &[], &[f.chat.join("link").join("x")]);
        assert_eq!(v, "desktop");
    }

    #[test]
    fn the_allow_table_of_the_config_covers_a_command() {
        let f = folders();
        let allow = [vec!["cargo".to_owned(), "test".to_owned()]];
        let policy = policy(std::slice::from_ref(&f.root), &f.chat, &deny(&f), &allow);
        let call = command_call("cargo test -q", &f.chat);
        assert_eq!(answer(classify(&call, &policy, &[])), "allow");
        let call = command_call("cargo build", &f.chat);
        assert_eq!(answer(classify(&call, &policy, &[])), "ask");
    }
}
