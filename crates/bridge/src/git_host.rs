//! `git` on the host, run by the bridge and never by the agent (SPEC.md 9.11). Hooks,
//! `core.fsmonitor`, prompts, and editors are off, so no code of the chat folder runs.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result};
use tempfile::TempDir;

/// Git takes a pathspec as it is, so a file name with `*` names only that file.
const FIXED_ENV: [(&str, &str); 6] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_LITERAL_PATHSPECS", "1"),
    ("GIT_EDITOR", "true"),
    ("GIT_PAGER", "cat"),
    ("LC_ALL", "C"),
];

/// What git needs from the environment: the login folder for the config of the user, and
/// the programs that sign a commit.
const PASSED_ENV: [&str; 16] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "SYSTEMROOT",
    "APPDATA",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "GNUPGHOME",
    "SSH_AUTH_SOCK",
    "TMPDIR",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
];

/// A failed git command, in the words of git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError {
    pub status: Option<i32>,
    /// The first line of the error output, or of the output when that is empty.
    pub line: String,
    /// The error output and the output, for a caller that looks for a known message.
    pub all: String,
}

impl GitError {
    /// A failure before or around git, with no output of git.
    pub fn other(line: String) -> GitError {
        GitError {
            status: None,
            all: line.clone(),
            line,
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.line)
    }
}

/// Whether git reads the config files of the user and of the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserConfig {
    Read,
    /// For tests: a `commit.gpgSign` of the developer must not reach them.
    Skip,
}

pub struct GitHost {
    /// An empty folder: `core.hooksPath` points into it, so no hook exists.
    hooks_off: TempDir,
    user_config: UserConfig,
}

impl GitHost {
    pub fn new() -> Result<GitHost> {
        GitHost::with_config(UserConfig::Read)
    }

    pub fn with_config(user_config: UserConfig) -> Result<GitHost> {
        let hooks_off = tempfile::Builder::new()
            .prefix("gnomish-relay-git-")
            .tempdir()
            .context("cannot make the private folder for git")?;
        Ok(GitHost {
            hooks_off,
            user_config,
        })
    }

    /// A private folder that lives as long as this host, for a copy of an index.
    pub fn scratch(&self) -> &Path {
        self.hooks_off.path()
    }

    fn hooks_path(&self) -> PathBuf {
        self.hooks_off.path().join("no-hooks")
    }

    pub fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new("git");
        command.env_clear();
        for name in PASSED_ENV {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.envs(FIXED_ENV);
        if self.user_config == UserConfig::Skip {
            command.env("GIT_CONFIG_GLOBAL", null_file());
            command.env("GIT_CONFIG_NOSYSTEM", "1");
        }
        let mut hooks = OsString::from("core.hooksPath=");
        hooks.push(self.hooks_path());
        command
            .arg("-C")
            .arg(dir)
            .arg("-c")
            .arg(hooks)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
            ])
            .stdin(Stdio::null());
        command
    }

    /// The raw output, whatever the exit status.
    pub fn output<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Result<Output, GitError> {
        self.command(dir)
            .args(args)
            .output()
            .map_err(|e| GitError::other(format!("git does not start: {e}")))
    }

    /// The output of a command that must succeed.
    pub fn bytes<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Result<Vec<u8>, GitError> {
        let output = self.output(dir, args)?;
        if !output.status.success() {
            return Err(error_of(&output));
        }
        Ok(output.stdout)
    }

    /// The output of a command that must succeed, as trimmed text.
    pub fn text<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Result<String, GitError> {
        let bytes = self.bytes(dir, args)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    }

    /// A command that must succeed, on another index than the one of the user.
    pub fn with_index<S: AsRef<OsStr>>(
        &self,
        dir: &Path,
        index: &Path,
        args: &[S],
    ) -> Result<Vec<u8>, GitError> {
        let output = self
            .command(dir)
            .env("GIT_INDEX_FILE", index)
            .args(args)
            .output()
            .map_err(|e| GitError::other(format!("git does not start: {e}")))?;
        if !output.status.success() {
            return Err(error_of(&output));
        }
        Ok(output.stdout)
    }

    /// True for exit status 0, false for 1, and an error for anything else. Many git
    /// questions answer this way, for example `merge-base --is-ancestor`.
    pub fn yes<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Result<bool, GitError> {
        let output = self.output(dir, args)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(error_of(&output)),
        }
    }
}

#[cfg(unix)]
fn null_file() -> &'static str {
    "/dev/null"
}

#[cfg(not(unix))]
fn null_file() -> &'static str {
    "NUL"
}

pub fn error_of(output: &Output) -> GitError {
    let line = first_line(&output.stderr)
        .or_else(|| first_line(&output.stdout))
        .unwrap_or_else(|| "git failed".to_owned());
    let all = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    GitError {
        status: output.status.code(),
        line,
        all,
    }
}

/// The first line with text, with no `error:` or `fatal:` in front: the player reads it.
pub fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = ["fatal: ", "error: "]
        .iter()
        .fold(line, |l, prefix| l.strip_prefix(prefix).unwrap_or(l));
    Some(line.chars().take(200).collect())
}

/// The parts of `-z` output, with no empty last part.
pub fn nul_parts(bytes: &[u8]) -> Vec<&[u8]> {
    bytes
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (TempDir, GitHost) {
        let dir = tempfile::tempdir().unwrap();
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        git.bytes(dir.path(), &["init", "-q", "-b", "main"])
            .unwrap();
        (dir, git)
    }

    #[test]
    fn the_first_line_of_an_error_loses_the_word_fatal() {
        assert_eq!(
            first_line(b"\nfatal: not a git repository\nmore\n").as_deref(),
            Some("not a git repository")
        );
        assert_eq!(first_line(b"  \n"), None);
    }

    #[test]
    fn a_failed_command_names_the_first_line_of_git() {
        let dir = tempfile::tempdir().unwrap();
        let git = GitHost::with_config(UserConfig::Skip).unwrap();

        let error = git.text(dir.path(), &["rev-parse", "HEAD"]).unwrap_err();

        assert!(error.line.contains("not a git repository"), "{error}");
    }

    #[test]
    fn a_hook_of_the_repository_never_runs() {
        let (dir, git) = repo();
        let hook = dir.path().join(".git/hooks/pre-commit");
        let mark = dir.path().join("hook-ran");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", mark.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dir.path().join("a"), "a").unwrap();
        git.bytes(dir.path(), &["add", "a"]).unwrap();

        git.bytes(
            dir.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "x",
            ],
        )
        .unwrap();

        assert!(!mark.exists());
    }

    #[test]
    fn a_question_of_git_answers_yes_or_no() {
        let (dir, git) = repo();

        let unborn = git.yes(dir.path(), &["rev-parse", "-q", "--verify", "HEAD"]);

        assert_eq!(unborn, Ok(false));
    }

    #[test]
    fn nul_parts_drop_the_empty_last_part() {
        assert_eq!(nul_parts(b"a\0b\0\0"), [b"a".as_slice(), b"b"]);
    }
}
