//! The free local model that setup offers for Timeways when it finds no model (SPEC.md
//! 11.6): Ollama from its official installer, and one small model.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;

use crate::agent::StopSignal;
use crate::config::Config;
use crate::model::ModelChoice;
use crate::model_local;
use crate::process;

/// Timeways picks this model. It needs about 2 GB.
pub const LOCAL_STORY_MODEL: &str = "llama3.2:3b";

pub const START_WAIT: Duration = Duration::from_mins(1);
/// The first answer loads the model into memory, so it takes longer than a later one.
const CHECK_TIME: Duration = Duration::from_mins(2);
const CHECK_PROMPT: &str = "Say hello in five words.";
/// Ollama sends a status line at least every few seconds while it downloads.
const PULL_STALL_SECONDS: &str = "300";

/// Linux and macOS share the official script. It installs the app of Ollama on macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    LinuxOrMac,
    Windows,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else {
            Os::LinuxOrMac
        }
    }
}

/// An official installer of Ollama.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Installer {
    pub url: &'static str,
    pub file_name: &'static str,
    /// What the player sees before the question.
    pub shown: &'static str,
}

pub fn installer(os: Os) -> Installer {
    match os {
        Os::LinuxOrMac => Installer {
            url: "https://ollama.com/install.sh",
            file_name: "install.sh",
            shown: "curl -fsSL https://ollama.com/install.sh | sh",
        },
        Os::Windows => Installer {
            url: "https://ollama.com/download/OllamaSetup.exe",
            file_name: "OllamaSetup.exe",
            shown: "https://ollama.com/download/OllamaSetup.exe",
        },
    }
}

/// The official URL redirects to GitHub. Each hop must stay on HTTPS.
pub fn download_command(curl: &Path, url: &str, to: &Path) -> Command {
    let mut command = process::allowlisted(curl, &[]);
    command.args([
        "-q",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--location",
        "--max-redirs",
        "5",
        "--fail",
        "--silent",
        "--show-error",
        "--output",
    ]);
    command.arg(to).arg(url);
    command
}

/// The installer runs in the terminal of the player, so `sudo` can ask for a password.
/// The Windows installer needs no admin rights and opens no window with these flags.
pub fn run_command(os: Os, file: &Path) -> Command {
    match os {
        Os::LinuxOrMac => {
            let mut command = Command::new("/bin/sh");
            command.arg(file);
            command
        }
        Os::Windows => {
            let mut command = Command::new(file);
            command.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]);
            command
        }
    }
}

/// Where Ollama listens, and the `curl` that talks to it.
#[derive(Clone, Debug)]
pub struct Ollama {
    pub curl: PathBuf,
    pub url: String,
}

/// The installer goes into `folder`, which the caller makes private.
pub fn download_installer(curl: &Path, os: Os, folder: &Path) -> Result<Command> {
    let installer = installer(os);
    let file = folder.join(installer.file_name);
    let output = download_command(curl, installer.url, &file)
        .output()
        .context("Couldn't start curl")?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        bail!("Couldn't download the Ollama installer: {}", reason.trim());
    }
    Ok(run_command(os, &file))
}

/// Runs the installer, then waits up to `wait` for Ollama to answer.
pub fn install_and_start(ollama: &Ollama, mut installer: Command, wait: Duration) -> Result<()> {
    let status = installer
        .status()
        .context("Couldn't start the Ollama installer")?;
    if !status.success() {
        bail!("The Ollama installer stopped with {status}.");
    }
    wait_for_server(ollama, wait)
}

pub fn wait_for_server(ollama: &Ollama, wait: Duration) -> Result<()> {
    let start = Instant::now();
    while !server_answers(ollama) {
        if start.elapsed() >= wait {
            bail!("Ollama didn't start.");
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

/// Ollama with no model answers here, but `/v1/models` gives an empty list.
pub fn server_answers(ollama: &Ollama) -> bool {
    let status = process::allowlisted(&ollama.curl, &[])
        .args([
            "-q",
            "--proto",
            "=http",
            "--max-redirs",
            "0",
            "--noproxy",
            "*",
            "--max-time",
            "2",
            "--silent",
            "--fail",
            "--output",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
            &format!("{}/api/version", ollama.url),
        ])
        .stdin(Stdio::null())
        .status();
    status.is_ok_and(|s| s.success())
}

/// One line of the stream of `POST /api/pull`.
#[derive(Debug, PartialEq, Eq)]
pub enum PullLine {
    Layer {
        digest: String,
        completed: u64,
        total: u64,
    },
    Success,
    Error(String),
    /// Any other status, such as "verifying sha256 digest", or a line it cannot read.
    Other,
}

#[derive(Deserialize)]
struct PullStatus {
    status: Option<String>,
    digest: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
    error: Option<String>,
}

pub fn read_pull_line(line: &[u8]) -> PullLine {
    let Ok(pulled) = serde_json::from_slice::<PullStatus>(line) else {
        return PullLine::Other;
    };
    if let Some(error) = pulled.error {
        return PullLine::Error(error);
    }
    if pulled.status.as_deref() == Some("success") {
        return PullLine::Success;
    }
    match (pulled.digest, pulled.total) {
        (Some(digest), Some(total)) => PullLine::Layer {
            digest,
            completed: pulled.completed.unwrap_or(0),
            total,
        },
        _ => PullLine::Other,
    }
}

/// The model comes in layers, each with its own count.
#[derive(Default)]
pub struct PullProgress {
    layers: BTreeMap<String, (u64, u64)>,
}

impl PullProgress {
    pub fn add(&mut self, digest: String, completed: u64, total: u64) {
        self.layers.insert(digest, (completed, total));
    }

    /// The megabytes so far, and of every layer that Ollama named.
    pub fn megabytes(&self) -> (u64, u64) {
        let completed: u64 = self.layers.values().map(|(c, _)| c).sum();
        let total: u64 = self.layers.values().map(|(_, t)| t).sum();
        (completed / 1_000_000, total / 1_000_000)
    }
}

pub fn pull_command(ollama: &Ollama) -> Command {
    let mut command = process::allowlisted(&ollama.curl, &[]);
    command.args([
        "-q",
        "--proto",
        "=http",
        "--max-redirs",
        "0",
        "--noproxy",
        "*",
        "--no-buffer",
        "--speed-limit",
        "1",
        "--speed-time",
        PULL_STALL_SECONDS,
        "--silent",
        "--show-error",
        "--fail",
        "--header",
        "Content-Type: application/json",
        "--data-binary",
        "@-",
        &format!("{}/api/pull", ollama.url),
    ]);
    command
}

/// Downloads `model` into Ollama. `progress` gets the megabytes so far and in all.
pub fn pull(ollama: &Ollama, model: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    let mut child = pull_command(ollama)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Couldn't start curl")?;
    let body = json!({ "model": model, "stream": true }).to_string();
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.as_bytes())?;
    }
    let Some(mut stdout) = child.stdout.take() else {
        bail!("Couldn't read the answer of Ollama");
    };
    let ended = read_pull_stream(&mut stdout, progress);
    let mut reason = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        stderr.read_to_string(&mut reason)?;
    }
    child.wait()?;
    match ended {
        PullLine::Success => Ok(()),
        PullLine::Error(error) => bail!("Ollama couldn't download {model}: {error}"),
        _ if !reason.trim().is_empty() => bail!("Couldn't download {model}: {}", reason.trim()),
        _ => bail!("The download of {model} stopped before its end."),
    }
}

/// The line that ended the stream: `Success`, `Error`, or `Other` for a cut stream.
fn read_pull_stream(stdout: &mut dyn Read, progress: &mut dyn FnMut(u64, u64)) -> PullLine {
    let mut counts = PullProgress::default();
    for line in BufReader::new(stdout).split(b'\n') {
        let Ok(line) = line else {
            break;
        };
        match read_pull_line(&line) {
            PullLine::Layer {
                digest,
                completed,
                total,
            } => {
                counts.add(digest, completed, total);
                let (so_far, all) = counts.megabytes();
                progress(so_far, all);
            }
            PullLine::Other => {}
            ended => return ended,
        }
    }
    PullLine::Other
}

/// Sends one short prompt to the local model of `config`, through the same code as a
/// model call of the story program.
pub fn check_answer(config: &Config) -> Result<()> {
    let choice = config.story.as_ref().map(|story| &story.model.choice);
    let Some(ModelChoice::Local(local)) = choice else {
        bail!("The config has no local model.");
    };
    let answer = model_local::ask(local, CHECK_PROMPT, CHECK_TIME, StopSignal::default())
        .map_err(anyhow::Error::msg)?;
    if answer.trim().is_empty() {
        bail!("The model sent an empty answer.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn each_installer_is_the_official_one_over_https() {
        for os in [Os::LinuxOrMac, Os::Windows] {
            let installer = installer(os);
            assert!(installer.url.starts_with("https://ollama.com/"), "{os:?}");
            assert!(installer.shown.contains(installer.url), "{os:?}");
        }
        assert_eq!(
            installer(Os::LinuxOrMac).url,
            "https://ollama.com/install.sh"
        );
        assert_eq!(
            installer(Os::Windows).url,
            "https://ollama.com/download/OllamaSetup.exe"
        );
    }

    #[test]
    fn the_download_takes_only_https_also_after_a_redirect() {
        let command = download_command(
            Path::new("/usr/bin/curl"),
            "https://ollama.com/install.sh",
            Path::new("/tmp/x/install.sh"),
        );
        let args = args(&command);
        let after = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].as_str()
        };
        assert_eq!(args[0], "-q");
        assert_eq!(after("--proto"), "=https");
        assert_eq!(after("--proto-redir"), "=https");
        assert_eq!(after("--output"), "/tmp/x/install.sh");
        assert_eq!(args.last().unwrap(), "https://ollama.com/install.sh");
    }

    #[test]
    fn the_script_runs_with_sh_and_the_windows_installer_runs_with_no_window() {
        let script = run_command(Os::LinuxOrMac, Path::new("/tmp/x/install.sh"));
        assert_eq!(script.get_program(), "/bin/sh");
        assert_eq!(args(&script), ["/tmp/x/install.sh"]);

        let setup = run_command(Os::Windows, Path::new("C:\\t\\OllamaSetup.exe"));
        assert_eq!(setup.get_program(), "C:\\t\\OllamaSetup.exe");
        assert!(args(&setup).contains(&"/VERYSILENT".to_owned()));
        assert!(args(&setup).contains(&"/SUPPRESSMSGBOXES".to_owned()));
    }

    #[test]
    fn the_model_is_one_small_model() {
        assert_eq!(LOCAL_STORY_MODEL, "llama3.2:3b");
        assert!(crate::config::is_model_name(LOCAL_STORY_MODEL));
    }

    #[test]
    fn a_pull_line_is_a_layer_a_success_an_error_or_other() {
        assert_eq!(
            read_pull_line(
                br#"{"status":"pulling abc","digest":"sha256:abc","total":2000,"completed":500}"#
            ),
            PullLine::Layer {
                digest: "sha256:abc".into(),
                completed: 500,
                total: 2000
            }
        );
        assert_eq!(
            read_pull_line(br#"{"status":"success"}"#),
            PullLine::Success
        );
        assert_eq!(
            read_pull_line(br#"{"error":"pull model manifest: file does not exist"}"#),
            PullLine::Error("pull model manifest: file does not exist".into())
        );
        assert_eq!(
            read_pull_line(br#"{"status":"pulling manifest"}"#),
            PullLine::Other
        );
        assert_eq!(read_pull_line(b"<html>"), PullLine::Other);
    }

    #[test]
    fn the_progress_adds_the_layers_and_counts_each_layer_once() {
        let mut progress = PullProgress::default();
        progress.add("a".into(), 1_000_000_000, 2_000_000_000);
        progress.add("b".into(), 5_000_000, 10_000_000);
        progress.add("a".into(), 1_500_000_000, 2_000_000_000);

        assert_eq!(progress.megabytes(), (1505, 2010));
    }

    #[test]
    fn a_stream_ends_at_its_success_or_error_line() {
        let mut seen = Vec::new();
        let stream = b"{\"status\":\"pulling manifest\"}\n\
            {\"status\":\"pulling a\",\"digest\":\"a\",\"total\":3000000,\"completed\":1000000}\n\
            {\"status\":\"success\"}\n";
        let ended = read_pull_stream(&mut &stream[..], &mut |done, all| seen.push((done, all)));
        assert_eq!(ended, PullLine::Success);
        assert_eq!(seen, [(1, 3)]);

        let failed = read_pull_stream(&mut &b"{\"error\":\"no space left\"}\n"[..], &mut |_, _| {});
        assert_eq!(failed, PullLine::Error("no space left".into()));

        let cut = read_pull_stream(
            &mut &b"{\"status\":\"pulling manifest\"}\n"[..],
            &mut |_, _| {},
        );
        assert_eq!(cut, PullLine::Other);
    }
}
