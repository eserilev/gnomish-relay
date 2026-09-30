//! The CI checks of the pull request of a branch, through `gh` on the host (SPEC.md 9.11).
//! Only the bridge runs it, never the agent, and only with `[git] ci_checks = true`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(50);
/// `statusCheckRollup` of a large pull request is far below this.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
const MAX_NAMES: usize = 2;
const MAX_NAME: usize = 60;
const ODD_BRANCH: &str = "This branch name starts with -, so GitHub can't look it up.";

pub const NO_GH: &str =
    "Checks need the GitHub CLI. On your desktop, install gh and run gh auth login.";
pub const OFF: &str =
    "Checks are off. To turn them on, set ci_checks = true under [git] in config.toml.";

const PASSED_ENV: [&str; 12] = [
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_HOST",
    "GH_CONFIG_DIR",
    "SYSTEMROOT",
];

/// Whether the bridge asks GitHub for the checks of a chat branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CiChecks {
    #[default]
    Off,
    /// `gh`, from `PATH` of the bridge, or a fake one in the tests.
    On { program: PathBuf },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiCounts {
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    /// The names of the first failed checks, at most two.
    pub failed_names: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CiError {
    /// No `gh`, or `gh` with no login.
    NoGh,
    Failed(String),
}

impl CiError {
    pub fn text(&self) -> String {
        match self {
            CiError::NoGh => NO_GH.into(),
            CiError::Failed(why) => format!("Couldn't get the checks: {why}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Passed,
    Failed,
    Running,
}

/// A `CheckRun` has a status and a conclusion, a `StatusContext` a state.
fn verdict(check: &Value) -> Verdict {
    let text = |key: &str| check.get(key).and_then(Value::as_str).unwrap_or("");
    if let Some(state) = check.get("state").and_then(Value::as_str) {
        return match state {
            "SUCCESS" => Verdict::Passed,
            "PENDING" | "EXPECTED" => Verdict::Running,
            _ => Verdict::Failed,
        };
    }
    if text("status") != "COMPLETED" {
        return Verdict::Running;
    }
    match text("conclusion") {
        "SUCCESS" | "NEUTRAL" | "SKIPPED" => Verdict::Passed,
        _ => Verdict::Failed,
    }
}

/// A name from GitHub, short and with no control character. The block of the reply
/// doubles each `|` (S10).
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect()
}

fn name_of(check: &Value) -> String {
    let name = ["name", "context"]
        .iter()
        .find_map(|key| check.get(*key).and_then(Value::as_str))
        .unwrap_or("a check");
    clean_name(name)
}

/// The counts of the `statusCheckRollup` of `gh pr view --json`.
pub fn parse(json: &str) -> Option<CiCounts> {
    let value: Value = serde_json::from_str(json).ok()?;
    let checks = value.get("statusCheckRollup")?.as_array()?;
    let mut counts = CiCounts::default();
    for check in checks {
        match verdict(check) {
            Verdict::Passed => counts.passed += 1,
            Verdict::Running => counts.running += 1,
            Verdict::Failed => {
                counts.failed += 1;
                if counts.failed_names.len() < MAX_NAMES {
                    counts.failed_names.push(name_of(check));
                }
            }
        }
    }
    Some(counts)
}

fn command(program: &Path, folder: &Path, branch: &str) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for name in PASSED_ENV {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .envs([
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
            ("NO_COLOR", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
        ])
        .current_dir(folder)
        .args(["pr", "view", branch, "--json", "statusCheckRollup"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn read_all(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut bytes);
        }
        bytes
    })
}

/// `Ok(None)` when the branch has no pull request.
pub fn checks(program: &Path, folder: &Path, branch: &str) -> Result<Option<CiCounts>, CiError> {
    // git refuses such a name, but the agent can write `.git/HEAD` by hand, and gh then
    // takes the name as a flag.
    if branch.starts_with('-') {
        return Err(CiError::Failed(ODD_BRANCH.into()));
    }
    let mut child = command(program, folder, branch)
        .spawn()
        .map_err(|_| CiError::NoGh)?;
    let out = read_all(child.stdout.take());
    let err = read_all(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CiError::Failed("GitHub didn't answer in time.".into()));
        }
        thread::sleep(POLL);
    };
    let out = out.join().unwrap_or_default();
    let err = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if status.success() {
        return parse(&String::from_utf8_lossy(&out))
            .map(Some)
            .ok_or_else(|| CiError::Failed("gh gave no checks.".into()));
    }
    if err.contains("no pull requests found") {
        return Ok(None);
    }
    if err.contains("gh auth login") || err.contains("not logged") {
        return Err(CiError::NoGh);
    }
    let line = crate::git_host::first_line(err.as_bytes()).unwrap_or_else(|| "gh failed".into());
    Err(CiError::Failed(line))
}

/// The reply of Checks when the branch has no pull request.
pub fn no_pull_request(branch: &str) -> String {
    format!("No pull request for {branch} yet.")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLUP: &str = r#"{"statusCheckRollup":[
        {"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS"},
        {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED"},
        {"__typename":"CheckRun","name":"lint|fmt","status":"COMPLETED","conclusion":"FAILURE"},
        {"__typename":"CheckRun","name":"e2e","status":"IN_PROGRESS","conclusion":""},
        {"__typename":"StatusContext","context":"ci/old","state":"ERROR"},
        {"__typename":"StatusContext","context":"ci/deploy","state":"PENDING"},
        {"__typename":"CheckRun","name":"third","status":"COMPLETED","conclusion":"TIMED_OUT"}
    ]}"#;

    #[test]
    fn the_rollup_counts_passed_failed_and_running_checks() {
        let counts = parse(ROLLUP).unwrap();

        assert_eq!(
            counts,
            CiCounts {
                passed: 2,
                failed: 3,
                running: 2,
                failed_names: vec!["lint|fmt".into(), "ci/old".into()],
            }
        );
    }

    #[test]
    fn a_name_loses_its_control_characters() {
        assert_eq!(clean_name("a\nb\x1bc|d"), "abc|d");
    }

    #[test]
    fn output_that_is_not_a_rollup_has_no_counts() {
        assert_eq!(parse("{}"), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(
            parse(r#"{"statusCheckRollup":[]}"#),
            Some(CiCounts::default())
        );
    }

    #[test]
    fn a_missing_gh_asks_for_the_github_cli() {
        let folder = tempfile::tempdir().unwrap();

        let result = checks(Path::new("/no/such/gh"), folder.path(), "main");

        assert_eq!(result, Err(CiError::NoGh));
        assert_eq!(CiError::NoGh.text(), NO_GH);
    }

    #[cfg(unix)]
    fn fake_gh(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("gh");
        crate::fake_program::write(&path, &format!("#!/bin/sh\n{script}\n")).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn gh_gets_the_branch_and_its_answer_becomes_counts() {
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let gh = fake_gh(
            dir.path(),
            &format!(
                "echo \"$@\" > {}\necho '{{\"statusCheckRollup\":[{{\"name\":\"b\",\"status\":\"COMPLETED\",\"conclusion\":\"SUCCESS\"}}]}}'",
                args.display()
            ),
        );

        let counts = checks(&gh, dir.path(), "gnomish/x").unwrap().unwrap();

        assert_eq!(counts.passed, 1);
        let seen = std::fs::read_to_string(args).unwrap();
        assert_eq!(seen.trim(), "pr view gnomish/x --json statusCheckRollup");
    }

    #[cfg(unix)]
    #[test]
    fn a_branch_that_starts_with_a_dash_never_reaches_gh() {
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let gh = fake_gh(dir.path(), &format!("echo \"$@\" > {}", args.display()));

        let result = checks(&gh, dir.path(), "--web");

        assert_eq!(result, Err(CiError::Failed(ODD_BRANCH.into())));
        assert!(!args.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_branch_with_no_pull_request_has_no_checks() {
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            "echo 'no pull requests found for branch \"x\"' >&2\nexit 1",
        );

        assert_eq!(checks(&gh, dir.path(), "x"), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn gh_with_no_login_asks_for_the_github_cli() {
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            "echo 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4",
        );

        assert_eq!(checks(&gh, dir.path(), "x"), Err(CiError::NoGh));
    }

    #[cfg(unix)]
    #[test]
    fn gh_gets_no_other_environment_than_its_list() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join("env");
        let gh = fake_gh(
            dir.path(),
            &format!(
                "env > {}\necho '{{\"statusCheckRollup\":[]}}'",
                env.display()
            ),
        );

        checks(&gh, dir.path(), "x").unwrap();

        let seen = std::fs::read_to_string(env).unwrap();
        assert!(seen.contains("GH_PROMPT_DISABLED=1"));
        assert!(!seen.contains("CARGO_PKG_NAME"), "{seen}");
    }
}
