//! Approvals on the desktop (SPEC.md 6.6.3): a tool call that only the user at the
//! computer can allow. The bridge has no window, so each open request is a file in the
//! data folder, and `gnomish-relay approve` or `gnomish-relay deny` answers it. No
//! addon can write these files.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fs_safe::check_real_dir;
use crate::program::find_program;

const FOLDER: &str = "approvals";
const REQUEST: &str = "json";
const ALLOW: &str = "allow";
const DENY: &str = "deny";
/// The request files are small. A bigger file is not ours.
const MAX_REQUEST: u64 = 16 * 1024;

/// One open request, as `gnomish-relay approve` lists it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    /// Unix seconds.
    pub created: u32,
    pub agent: String,
    pub folder: String,
    /// The popup text of the tool call (S15).
    pub text: String,
}

/// An answer from the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Deny,
}

impl Verdict {
    fn extension(self) -> &'static str {
        match self {
            Verdict::Approve => ALLOW,
            Verdict::Deny => DENY,
        }
    }
}

/// Whether the bridge shows a notice of the OS for each new request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    System,
    Off,
}

#[derive(Clone, Debug)]
pub struct Approvals {
    dir: PathBuf,
    notice: Notice,
}

fn is_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no random bytes from the OS: {e}"))?;
    Ok(bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// Mode 0600, and never through a link: `create_new` fails on any existing name.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

impl Approvals {
    /// The requests live in `approvals` inside the data folder `data`.
    pub fn new(data: &Path, notice: Notice) -> Approvals {
        Approvals {
            dir: data.join(FOLDER),
            notice,
        }
    }

    fn file(&self, id: &str, extension: &str) -> PathBuf {
        self.dir.join(format!("{id}.{extension}"))
    }

    fn ready_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("cannot make {}", self.dir.display()))?;
        check_real_dir(&self.dir)
    }

    /// Writes a new request, shows a notice, and returns its id.
    pub fn open(&self, agent: &str, folder: &str, text: &str, now: u32) -> Result<String> {
        self.ready_dir()?;
        let id = new_id()?;
        let pending = Pending {
            id: id.clone(),
            created: now,
            agent: agent.to_owned(),
            folder: folder.to_owned(),
            text: text.to_owned(),
        };
        write_new(&self.file(&id, REQUEST), &serde_json::to_vec(&pending)?)?;
        eprintln!(
            "{now} approve on the desktop: gnomish-relay approve {id} ({})",
            text.escape_debug()
        );
        if self.notice == Notice::System {
            show_notice(&format!("{text}\nRun: gnomish-relay approve {id}"));
        }
        Ok(id)
    }

    /// The answer of the desktop, once it exists.
    pub fn answer_of(&self, id: &str) -> Option<Verdict> {
        [Verdict::Deny, Verdict::Approve]
            .into_iter()
            .find(|v| self.file(id, v.extension()).exists())
    }

    /// Removes the request and its answer. The bridge calls it for every request it opened.
    pub fn close(&self, id: &str) {
        for extension in [REQUEST, ALLOW, DENY] {
            let _ = fs::remove_file(self.file(id, extension));
        }
    }

    /// A bridge that stopped leaves its requests. Nothing waits for them.
    pub fn clear(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }

    pub fn list(&self) -> Vec<Pending> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut pending: Vec<Pending> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == REQUEST))
            .filter_map(|p| read_pending(&p))
            .collect();
        pending.sort_by_key(|p| p.created);
        pending
    }

    /// Answers an open request, for `gnomish-relay approve` and `gnomish-relay deny`.
    pub fn answer(&self, id: &str, verdict: Verdict) -> Result<()> {
        if !is_id(id) || !self.file(id, REQUEST).is_file() {
            bail!("no open request {id}. Run: gnomish-relay approve");
        }
        if self.answer_of(id).is_some() {
            bail!("request {id} has an answer already");
        }
        check_real_dir(&self.dir)?;
        write_new(&self.file(id, verdict.extension()), b"")
    }
}

fn read_pending(path: &Path) -> Option<Pending> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_REQUEST {
        return None;
    }
    let pending: Pending = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let named = path.file_stem().is_some_and(|s| s == pending.id.as_str());
    (named && is_id(&pending.id)).then_some(pending)
}

/// A program, its arguments, and its extra environment variables.
pub type NoticeCommand = (String, Vec<String>, Vec<(String, String)>);

/// The program and the arguments that show `text` as a notice of the OS. The text goes
/// in an argument or a variable, never into a script, so it cannot run as code.
pub fn notice_command(os: &str, text: &str) -> Option<NoticeCommand> {
    let title = "Gnomish Relay".to_owned();
    match os {
        "linux" => Some(("notify-send".into(), vec![title, text.into()], Vec::new())),
        "macos" => Some((
            "osascript".into(),
            [
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title \"Gnomish Relay\"",
                "-e",
                "end run",
                text,
            ]
            .map(str::to_owned)
            .into(),
            Vec::new(),
        )),
        "windows" => Some((
            "powershell".into(),
            ["-NoProfile", "-NonInteractive", "-Command", WINDOWS_TOAST]
                .map(str::to_owned)
                .into(),
            vec![("GNOMISH_NOTICE".into(), text.into())],
        )),
        _ => None,
    }
}

/// A toast through the app id of PowerShell, which every Windows 10 and 11 has.
const WINDOWS_TOAST: &str = "\
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
$t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
$x = $t.GetElementsByTagName('text'); \
$x[0].AppendChild($t.CreateTextNode('Gnomish Relay')) > $null; \
$x[1].AppendChild($t.CreateTextNode($env:GNOMISH_NOTICE)) > $null; \
$app = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($app).Show([Windows.UI.Notifications.ToastNotification]::new($t))";

/// Best effort: with no tool for notices, the log line is the notice.
fn show_notice(text: &str) {
    let Some((program, args, env)) = notice_command(std::env::consts::OS, text) else {
        return;
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(found) = find_program(&program, &path, cfg!(windows)) else {
        return;
    };
    let mut command = std::process::Command::new(found);
    command
        .args(args)
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approvals() -> (tempfile::TempDir, Approvals) {
        let data = tempfile::tempdir().unwrap();
        let approvals = Approvals::new(data.path(), Notice::Off);
        (data, approvals)
    }

    #[test]
    fn an_open_request_is_listed_until_it_closes() {
        let (_data, approvals) = approvals();
        let id = approvals
            .open("claude", "/w/app", "cat ~/.ssh/id_rsa", 7)
            .unwrap();
        let listed = approvals.list();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id.as_str(), listed[0].created), (id.as_str(), 7));
        assert_eq!(listed[0].text, "cat ~/.ssh/id_rsa");
        approvals.close(&id);
        assert!(approvals.list().is_empty());
    }

    #[test]
    fn approve_and_deny_answer_an_open_request_once() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        assert_eq!(approvals.answer_of(&id), None);
        approvals.answer(&id, Verdict::Deny).unwrap();
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
        assert!(approvals.answer(&id, Verdict::Approve).is_err());
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[test]
    fn an_unknown_or_malformed_id_gets_no_answer() {
        let (_data, approvals) = approvals();
        approvals.open("claude", "/w", "x", 1).unwrap();
        assert!(approvals.answer("0123456789ab", Verdict::Approve).is_err());
        assert!(approvals.answer("../../etc", Verdict::Approve).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_request_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        let path = data.path().join(FOLDER).join(format!("{id}.json"));
        let mode = fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn clear_removes_the_requests_of_an_old_bridge() {
        let (_data, approvals) = approvals();
        approvals.open("claude", "/w", "x", 1).unwrap();
        approvals.clear();
        assert!(approvals.list().is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_request_is_not_listed() {
        let (data, approvals) = approvals();
        approvals.open("claude", "/w", "x", 1).unwrap();
        let dir = data.path().join(FOLDER);
        fs::write(dir.join("0123456789ab.json"), "not json").unwrap();
        fs::write(
            dir.join("aaaaaaaaaaaa.json"),
            r#"{"id":"bbbbbbbbbbbb","created":1,"agent":"a","folder":"f","text":"t"}"#,
        )
        .unwrap();
        assert_eq!(approvals.list().len(), 1);
    }

    #[test]
    fn the_text_of_a_notice_stays_an_argument() {
        let text = "\"; rm -rf ~; \"";
        let (program, args, _) = notice_command("linux", text).unwrap();
        assert_eq!((program.as_str(), args[1].as_str()), ("notify-send", text));
        let (_, args, _) = notice_command("macos", text).unwrap();
        assert_eq!(args.last().map(String::as_str), Some(text));
        assert!(!args[..args.len() - 1].iter().any(|a| a.contains("rm -rf")));
        let (_, args, env) = notice_command("windows", text).unwrap();
        assert!(!args.iter().any(|a| a.contains("rm -rf")));
        assert_eq!(env, [("GNOMISH_NOTICE".to_owned(), text.to_owned())]);
        assert!(notice_command("haiku", text).is_none());
    }
}
