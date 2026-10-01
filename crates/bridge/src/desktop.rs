//! Approvals on the desktop (SPEC.md 6.6.3): a tool call that only the user at the
//! computer can allow. Each open request is a file in the data folder. A dialog of the
//! OS, `gnomish-relay approve`, or `gnomish-relay deny` answers it, and the first
//! answer wins. No addon can write these files or click the dialog.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Permission;
use crate::dialog::{self, Button, CloseReason, Dialog, Ended, Shown, Tool, show_notice};
use crate::fs_safe::check_real_dir;
use crate::ids::random_hex;
use crate::run::log;

const FOLDER: &str = "approvals";
const REQUEST: &str = "json";
const ALLOW: &str = "allow";
const DENY: &str = "deny";
/// The request files are small. A bigger file is not ours.
const MAX_REQUEST: u64 = 16 * 1024;
/// How often a dialog checks whether its request still waits.
const POLL: Duration = Duration::from_millis(100);

/// One open request, as `gnomish-relay approve` lists it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    /// Unix seconds.
    pub created: u32,
    pub agent: String,
    pub folder: String,
    /// The popup text of the tool call (S15), or the fixed text of a raise.
    pub text: String,
    #[serde(default)]
    pub kind: Kind,
    /// How long the run waits for an answer. 0 in a request of an older bridge.
    #[serde(default)]
    pub wait_minutes: u64,
}

impl Pending {
    /// Whole minutes, rounded up, so the last minute shows as 1 and not 0.
    pub fn minutes_left(&self, now: u32) -> Option<u64> {
        if self.wait_minutes == 0 {
            return None;
        }
        let end = u64::from(self.created) + self.wait_minutes * 60;
        Some(end.saturating_sub(u64::from(now)).div_ceil(60))
    }

    /// For `gnomish-relay approve`: the request, then its text, indented.
    pub fn lines(&self, now: u32) -> Vec<String> {
        let age = now.saturating_sub(self.created);
        let left = self
            .minutes_left(now)
            .map_or(String::new(), |minutes| format!("  {minutes} min left"));
        let mut lines = vec![format!(
            "{}  {age}s ago{left}  {} in {}",
            self.id, self.agent, self.folder
        )];
        lines.extend(self.text.lines().map(|line| format!("    {line}")));
        lines
    }
}

/// What a desktop request asks for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    ToolCall,
    /// A higher level in `config.toml` (SPEC.md 9.3).
    Raise,
    /// A merge of a chat branch into its start branch (SPEC.md 9.11).
    Merge,
    /// A new root in `config.toml` (SPEC.md 9.12).
    Folder,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::ToolCall => "tool call",
            Kind::Raise => "raise",
            Kind::Merge => "merge",
            Kind::Folder => "folder",
        }
    }
}

/// The command of a request in the log, so the user can see what was asked.
const LOG_COMMAND: usize = 300;
const CUT_MARK: &str = "...";

/// The first line of `text` with no control characters, cut to `max` bytes at a
/// character boundary. A cut text ends with "...".
pub fn first_line_cut(text: &str, max: usize) -> String {
    let line: String = text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if line.len() <= max {
        return line;
    }
    let mut end = max.saturating_sub(CUT_MARK.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CUT_MARK}", &line[..end])
}

/// One log line for each request. For a tool call it ends with the first line of the
/// popup text: the command, but not the words of the agent (SPEC.md 6.6.3, "The log of
/// requests").
fn request_line(pending: &Pending, summary: &str) -> String {
    let asks = if summary.is_empty() {
        String::new()
    } else {
        format!(": {summary}")
    };
    let line = format!(
        "desktop request {id} ({kind}): {agent} in {folder}{asks}. To approve, run gnomish-relay approve {id}",
        id = pending.id,
        kind = pending.kind.word(),
        agent = pending.agent,
        folder = pending.folder,
    );
    let command = first_line_cut(&pending.text, LOG_COMMAND);
    if pending.kind != Kind::ToolCall || command.is_empty() {
        return line;
    }
    format!("{line}. Wants to: {command}")
}

/// How one dialog of a request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watched {
    Answered(Verdict),
    /// Closed with no button press. The request still waits.
    NoAnswer(Option<CloseReason>),
    /// Another answer, Stop, a new message, or the timeout ended the request first.
    Ended,
    /// The program of the dialog did not start.
    NotShown,
}

fn watched_line(id: &str, watched: Watched) -> String {
    let how = match watched {
        Watched::Answered(verdict) => format!("{verdict:?} in the dialog"),
        Watched::NoAnswer(reason) => format!(
            "the dialog closed with no answer ({}). The request still waits",
            reason.map_or("reason unknown", CloseReason::words)
        ),
        Watched::Ended => "the request ended before the dialog answered".to_owned(),
        Watched::NotShown => "the dialog did not start".to_owned(),
    };
    format!("desktop request {id}: {how}")
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

/// How the desktop asks the user for one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompted {
    Dialog,
    /// No dialog tool: only `gnomish-relay approve <id>` answers.
    CommandLine,
}

/// A new request: its id, and how the desktop asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub id: String,
    pub prompted: Prompted,
}

/// Where a desktop request of a run stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiting {
    Open,
    Approved,
    Denied,
    NoAnswer,
}

/// The state of a desktop request for the game (SPEC.md 6.6.3). The game shows a row
/// and a whisper line, never a popup. No addon can answer it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: String,
    pub prompted: Prompted,
    pub waiting: Waiting,
    pub topic: Topic,
}

/// What a desktop request of a run asks for, as the game shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    /// A tool call or a merge.
    Action,
    /// A higher level in the config (SPEC.md 9.3).
    Raise(Permission),
    /// A new root (SPEC.md 9.12).
    Folder,
}

/// Only the bridge writes a progress line with this start.
pub const NOTICE: &str = "Desktop: ";

impl Notice {
    /// For example `Desktop: wait a1b2c3d4e5f6 dialog raise auto-edit`. The id comes
    /// from the bridge, so no agent text is in the line.
    pub fn line(&self) -> String {
        let state = match self.waiting {
            Waiting::Open => "wait",
            Waiting::Approved => "approved",
            Waiting::Denied => "denied",
            Waiting::NoAnswer => "none",
        };
        let how = match self.prompted {
            Prompted::Dialog => "dialog",
            Prompted::CommandLine => "command",
        };
        let line = format!("{NOTICE}{state} {} {how}", self.id);
        match self.topic {
            Topic::Action => line,
            Topic::Raise(level) => format!("{line} raise {}", level.word()),
            Topic::Folder => format!("{line} folder"),
        }
    }

    #[must_use]
    pub fn ended(&self, waiting: Waiting) -> Notice {
        Notice {
            waiting,
            ..self.clone()
        }
    }
}

/// Whether the bridge shows a dialog of the OS for each new request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    Dialog,
    Off,
}

#[derive(Clone, Debug)]
pub struct Approvals {
    dir: PathBuf,
    prompt: Prompt,
    /// `permission_timeout_minutes` of the config, for the text of the dialog.
    wait: Duration,
}

fn is_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn new_id() -> Result<String> {
    random_hex(6)
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
    pub fn new(data: &Path, prompt: Prompt) -> Approvals {
        Approvals {
            dir: data.join(FOLDER),
            prompt,
            wait: Duration::ZERO,
        }
    }

    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> Approvals {
        self.wait = wait;
        self
    }

    fn file(&self, id: &str, extension: &str) -> PathBuf {
        self.dir.join(format!("{id}.{extension}"))
    }

    fn ready_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("cannot make {}", self.dir.display()))?;
        check_real_dir(&self.dir)
    }

    /// A plain notice with no buttons, only when the bridge shows dialogs.
    pub fn notice(&self, text: &str) {
        if self.prompt == Prompt::Dialog {
            crate::dialog::show_notice(text);
        }
    }

    /// Writes a new request, and shows a dialog when the desktop has one. `summary` goes to
    /// the log in place of `text`.
    pub fn open(
        &self,
        agent: &str,
        folder: &str,
        text: &str,
        summary: &str,
        now: u32,
    ) -> Result<Opened> {
        self.open_kind(agent, folder, text, now, Kind::ToolCall, summary)
    }

    /// A request to write a higher level for `agent` into `config_file`.
    pub fn open_raise(
        &self,
        agent: &str,
        config_file: &str,
        text: &str,
        now: u32,
    ) -> Result<Opened> {
        self.open_kind(agent, config_file, text, now, Kind::Raise, "")
    }

    /// A request to add `folder` to the roots of `config.toml`.
    pub fn open_folder(&self, agent: &str, folder: &str, text: &str, now: u32) -> Result<Opened> {
        self.open_kind(agent, folder, text, now, Kind::Folder, "")
    }

    /// A request to merge a chat branch in the repository `repo`.
    pub fn open_merge(&self, repo: &str, text: &str, now: u32) -> Result<Opened> {
        self.open_kind("git", repo, text, now, Kind::Merge, "")
    }

    fn open_kind(
        &self,
        agent: &str,
        folder: &str,
        text: &str,
        now: u32,
        kind: Kind,
        summary: &str,
    ) -> Result<Opened> {
        self.ready_dir()?;
        let id = new_id()?;
        let pending = Pending {
            id: id.clone(),
            created: now,
            agent: agent.to_owned(),
            folder: folder.to_owned(),
            text: text.to_owned(),
            kind,
            wait_minutes: self.wait.as_secs().div_ceil(60),
        };
        write_new(&self.file(&id, REQUEST), &serde_json::to_vec(&pending)?)?;
        log(&request_line(&pending, summary));
        let tool = match self.prompt {
            Prompt::Dialog => dialog::find_tool(),
            Prompt::Off => None,
        };
        let prompted = if tool.is_some() {
            Prompted::Dialog
        } else {
            Prompted::CommandLine
        };
        if self.prompt == Prompt::Dialog {
            let approvals = self.clone();
            std::thread::spawn(move || approvals.ask_the_desktop(&pending, tool));
        }
        Ok(Opened { id, prompted })
    }

    /// With no dialog tool, a plain notice names the command that answers.
    fn ask_the_desktop(&self, pending: &Pending, tool: Option<Tool>) {
        let text = dialog_text(pending);
        let Some(tool) = tool else {
            log(&format!("desktop request {}: no dialog tool", pending.id));
            show_notice(&format!(
                "{text}\nRun: gnomish-relay approve {}",
                pending.id
            ));
            return;
        };
        let mut dialogs = vec![dialog::dialog(tool, &text)];
        dialogs.extend(dialog::find_next_tool(tool).map(|next| dialog::dialog(next, &text)));
        self.ask_in_turn(&pending.id, &dialogs);
    }

    /// Shows the dialogs one after the other while each closes with no answer. After
    /// the last one, only the command line and the game line ask (SPEC.md 6.6.3).
    pub fn ask_in_turn(&self, id: &str, dialogs: &[Dialog]) -> Watched {
        let mut watched = Watched::NotShown;
        for dialog in dialogs {
            if !self.is_waiting(id) {
                return Watched::Ended;
            }
            log(&format!("desktop request {id}: dialog {:?}", dialog.tool));
            watched = self.watch(id, dialog);
            log(&watched_line(id, watched));
            if !matches!(watched, Watched::NoAnswer(_)) {
                return watched;
            }
        }
        log(&format!(
            "desktop request {id}: no more dialogs. To approve, run gnomish-relay approve {id}"
        ));
        watched
    }

    /// Shows `dialog` until it ends, or until the request no longer waits: an
    /// answer from the command line, Stop, or the timeout.
    pub fn watch(&self, id: &str, dialog: &Dialog) -> Watched {
        let Some(mut shown) = Shown::start(dialog) else {
            return Watched::NotShown;
        };
        loop {
            match shown.ended() {
                Some(Ended::Pressed(button)) => return self.pressed(id, button),
                Some(Ended::NoButton) => return Watched::NoAnswer(shown.close_reason()),
                None => {}
            }
            if !self.is_waiting(id) {
                shown.stop();
                return Watched::Ended;
            }
            std::thread::sleep(POLL);
        }
    }

    /// A press counts only while the request waits: the first answer wins.
    fn pressed(&self, id: &str, button: Button) -> Watched {
        let verdict = match button {
            Button::Approve => Verdict::Approve,
            Button::Deny => Verdict::Deny,
        };
        match self.answer(id, verdict) {
            Ok(()) => Watched::Answered(verdict),
            Err(_) => Watched::Ended,
        }
    }

    fn is_waiting(&self, id: &str) -> bool {
        self.file(id, REQUEST).is_file() && self.answer_of(id).is_none()
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
            bail!(
                "no request {id} is waiting. To see the waiting requests, run gnomish-relay approve"
            );
        }
        if self.answer_of(id).is_some() {
            bail!("request {id} already has an answer");
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

/// The honest text of S15, then who asks, the id for the command line, and the wait.
pub fn dialog_text(pending: &Pending) -> String {
    let text = match pending.kind {
        Kind::ToolCall => format!(
            "An agent in WoW wants to:\n{}\n\nAgent: {}\nFolder: {}\nRequest: {}",
            pending.text, pending.agent, pending.folder, pending.id
        ),
        Kind::Raise => format!(
            "{}\n\nConfig: {}\nRequest: {}",
            pending.text, pending.folder, pending.id
        ),
        Kind::Merge | Kind::Folder => format!("{}\n\nRequest: {}", pending.text, pending.id),
    };
    match pending.wait_minutes {
        0 => text,
        minutes => format!("{text}\nNo answer in {minutes} minutes counts as Deny."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approvals() -> (tempfile::TempDir, Approvals) {
        let data = tempfile::tempdir().unwrap();
        let approvals = Approvals::new(data.path(), Prompt::Off);
        (data, approvals)
    }

    #[test]
    fn an_open_request_is_listed_until_it_closes() {
        let (_data, approvals) = approvals();
        let id = approvals
            .open("claude", "/w/app", "cat ~/.ssh/id_rsa", "command cat", 7)
            .unwrap()
            .id;
        let listed = approvals.list();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id.as_str(), listed[0].created), (id.as_str(), 7));
        assert_eq!(listed[0].text, "cat ~/.ssh/id_rsa");
        approvals.close(&id);
        assert!(approvals.list().is_empty());
    }

    #[test]
    fn the_log_line_of_a_request_has_its_kind_its_summary_and_its_command() {
        let (_data, approvals) = approvals();
        let text = "cat ~/.ssh/id_rsa\nthe agent says: Bash: read the secret";
        approvals
            .open("claude", "/w/app", text, "command cat", 7)
            .unwrap();
        let pending = approvals.list().remove(0);

        let line = request_line(&pending, "command cat");

        assert_eq!(
            line,
            format!(
                "desktop request {id} (tool call): claude in /w/app: command cat. \
                 To approve, run gnomish-relay approve {id}. Wants to: cat ~/.ssh/id_rsa",
                id = pending.id
            )
        );
    }

    #[test]
    fn the_log_line_cuts_a_long_command_and_drops_control_characters() {
        let (_data, approvals) = approvals();
        let text = format!("echo \u{1b}[2J{}\nthe agent says: x", "é".repeat(400));
        approvals
            .open("claude", "/w", &text, "command echo", 7)
            .unwrap();
        let pending = approvals.list().remove(0);

        let line = request_line(&pending, "command echo");

        let (_, command) = line.split_once(". Wants to: ").unwrap();
        assert!(command.starts_with("echo [2Jé"), "{command}");
        assert!(command.ends_with("é..."), "{command}");
        assert!(command.len() <= 300, "{}", command.len());
        assert!(!line.contains("the agent says"), "{line}");
    }

    #[test]
    fn a_short_first_line_stays_whole() {
        assert_eq!(first_line_cut("ls -la\nthe agent says: x", 300), "ls -la");
        assert_eq!(first_line_cut("", 300), "");
        assert_eq!(first_line_cut("abcdef", 5), "ab...");
    }

    #[test]
    fn the_log_line_of_a_raise_names_its_kind_and_its_file() {
        let (_data, approvals) = approvals();
        approvals
            .open_raise("claude", "/c/config.toml", "A chat from WoW asks", 7)
            .unwrap();
        let pending = approvals.list().remove(0);

        let line = request_line(&pending, "");

        assert!(
            line.starts_with(&format!(
                "desktop request {} (raise): claude in /c/config.toml. To approve",
                pending.id
            )),
            "{line}"
        );
    }

    #[test]
    fn approve_and_deny_answer_an_open_request_once() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        assert_eq!(approvals.answer_of(&id), None);
        approvals.answer(&id, Verdict::Deny).unwrap();
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
        assert!(approvals.answer(&id, Verdict::Approve).is_err());
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[test]
    fn an_unknown_or_malformed_id_gets_no_answer() {
        let (_data, approvals) = approvals();
        approvals.open("claude", "/w", "x", "", 1).unwrap();
        assert!(approvals.answer("0123456789ab", Verdict::Approve).is_err());
        assert!(approvals.answer("../../etc", Verdict::Approve).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_request_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let path = data.path().join(FOLDER).join(format!("{id}.json"));
        let mode = fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn clear_removes_the_requests_of_an_old_bridge() {
        let (_data, approvals) = approvals();
        approvals.open("claude", "/w", "x", "", 1).unwrap();
        approvals.clear();
        assert!(approvals.list().is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_request_is_not_listed() {
        let (data, approvals) = approvals();
        approvals.open("claude", "/w", "x", "", 1).unwrap();
        let dir = data.path().join(FOLDER);
        fs::write(dir.join("0123456789ab.json"), "not json").unwrap();
        fs::write(
            dir.join("aaaaaaaaaaaa.json"),
            r#"{"id":"bbbbbbbbbbbb","created":1,"agent":"a","folder":"f","text":"t"}"#,
        )
        .unwrap();
        assert_eq!(approvals.list().len(), 1);
    }

    #[cfg(unix)]
    fn fake(script: &str) -> Dialog {
        Dialog {
            tool: dialog::Tool::NotifySend,
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env: Vec::new(),
            close_watch: None,
        }
    }

    /// A fake `gdbus monitor` that prints the close of notice 7 with `reason`.
    #[cfg(unix)]
    fn closes_notice_7(reason: u32) -> dialog::CloseWatch {
        let line = format!(
            "/org/freedesktop/Notifications: org.freedesktop.Notifications.NotificationClosed \
             (uint32 7, uint32 {reason})"
        );
        dialog::CloseWatch {
            program: "sh".into(),
            args: vec!["-c".into(), format!("echo '{line}'; sleep 30")],
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_click_on_approve_in_the_dialog_answers_the_request() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let answer = approvals.watch(&id, &fake("echo 7; echo approve"));
        assert_eq!(answer, Watched::Answered(Verdict::Approve));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Approve));
    }

    #[cfg(unix)]
    #[test]
    fn a_click_on_deny_in_the_dialog_denies() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let answer = approvals.watch(&id, &fake("echo 7; echo deny"));
        assert_eq!(answer, Watched::Answered(Verdict::Deny));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[cfg(unix)]
    #[test]
    fn a_dismissed_dialog_leaves_the_request_waiting() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;

        let watched = approvals.watch(&id, &fake("echo 7"));

        assert_eq!(watched, Watched::NoAnswer(None));
        assert_eq!(approvals.answer_of(&id), None);
        assert_eq!(approvals.list().len(), 1, "the request still waits");
    }

    #[cfg(unix)]
    #[test]
    fn a_closed_notice_gives_the_reason_from_the_notice_server() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let dismissed = Dialog {
            close_watch: Some(closes_notice_7(2)),
            ..fake("sleep 0.2; echo 7")
        };
        let expired = Dialog {
            close_watch: Some(closes_notice_7(1)),
            ..fake("sleep 0.2; echo 7")
        };

        let first = approvals.watch(&id, &dismissed);
        let second = approvals.watch(&id, &expired);

        assert_eq!(first, Watched::NoAnswer(Some(CloseReason::Dismissed)));
        assert_eq!(second, Watched::NoAnswer(Some(CloseReason::Expired)));
        assert_eq!(approvals.answer_of(&id), None);
    }

    #[test]
    fn the_log_line_of_a_closed_dialog_says_that_the_request_still_waits() {
        let line = watched_line(
            "a1b2c3d4e5f6",
            Watched::NoAnswer(Some(CloseReason::Dismissed)),
        );
        let unknown = watched_line("a1b2c3d4e5f6", Watched::NoAnswer(None));
        let approved = watched_line("a1b2c3d4e5f6", Watched::Answered(Verdict::Approve));

        assert_eq!(
            line,
            "desktop request a1b2c3d4e5f6: the dialog closed with no answer \
             (dismissed by the user). The request still waits"
        );
        assert!(unknown.contains("(reason unknown)"), "{unknown}");
        assert_eq!(
            approved,
            "desktop request a1b2c3d4e5f6: Approve in the dialog"
        );
    }

    #[cfg(unix)]
    #[test]
    fn after_a_closed_dialog_the_next_dialog_asks() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;

        let watched = approvals.ask_in_turn(&id, &[fake("echo 7"), fake("echo approve")]);

        assert_eq!(watched, Watched::Answered(Verdict::Approve));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Approve));
    }

    #[cfg(unix)]
    #[test]
    fn after_the_last_closed_dialog_no_dialog_asks_and_the_request_waits() {
        let (data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let shown = data.path().join("shown");
        let count = format!("echo x >> '{}'", shown.display());

        let watched = approvals.ask_in_turn(&id, &[fake(&count), fake(&count)]);

        assert_eq!(watched, Watched::NoAnswer(None));
        assert_eq!(fs::read_to_string(&shown).unwrap(), "x\nx\n");
        assert_eq!(approvals.answer_of(&id), None);
        assert_eq!(approvals.list().len(), 1, "the request still waits");
    }

    #[cfg(unix)]
    #[test]
    fn an_answer_from_the_command_line_shows_no_more_dialogs() {
        let (data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let shown = data.path().join("shown");
        approvals.answer(&id, Verdict::Approve).unwrap();

        let watched =
            approvals.ask_in_turn(&id, &[fake(&format!("echo x >> '{}'", shown.display()))]);

        assert_eq!(watched, Watched::Ended);
        assert!(!shown.exists(), "no dialog after an answer");
    }

    #[cfg(unix)]
    #[test]
    fn the_first_answer_wins_and_stops_the_dialog() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        approvals.answer(&id, Verdict::Deny).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            approvals.watch(&id, &fake("sleep 30; echo approve")),
            Watched::Ended
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[cfg(unix)]
    #[test]
    fn a_closed_request_stops_its_dialog() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", "", 1).unwrap().id;
        let closing = approvals.clone();
        let closed = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            closing.close(&closed);
        });
        assert_eq!(
            approvals.watch(&id, &fake("sleep 30; echo approve")),
            Watched::Ended
        );
        assert_eq!(approvals.answer_of(&id), None);
    }

    #[test]
    fn with_no_dialog_the_request_says_that_only_the_command_answers() {
        let (_data, approvals) = approvals();
        let opened = approvals.open("claude", "/w", "x", "", 1).unwrap();
        assert_eq!(opened.prompted, Prompted::CommandLine);
    }

    #[test]
    fn a_notice_line_holds_the_state_the_id_and_how_the_desktop_asks() {
        let notice = Notice {
            id: "a1b2c3d4e5f6".into(),
            prompted: Prompted::Dialog,
            waiting: Waiting::Open,
            topic: Topic::Action,
        };
        assert_eq!(notice.line(), "Desktop: wait a1b2c3d4e5f6 dialog");
        let folder = Notice {
            topic: Topic::Folder,
            ..notice.clone()
        };
        assert_eq!(folder.line(), "Desktop: wait a1b2c3d4e5f6 dialog folder");
        let raise = Notice {
            prompted: Prompted::CommandLine,
            topic: Topic::Raise(Permission::AutoEdit),
            ..notice.ended(Waiting::Denied)
        };
        assert_eq!(
            raise.line(),
            "Desktop: denied a1b2c3d4e5f6 command raise auto-edit"
        );
        assert_eq!(
            notice.ended(Waiting::NoAnswer).line(),
            "Desktop: none a1b2c3d4e5f6 dialog"
        );
        assert_eq!(
            notice.ended(Waiting::Approved).line(),
            "Desktop: approved a1b2c3d4e5f6 dialog"
        );
    }

    #[test]
    fn the_dialog_shows_the_popup_text_who_asks_and_how_long_it_waits() {
        let pending = Pending {
            id: "a1b2c3d4e5f6".into(),
            created: 1,
            agent: "claude".into(),
            folder: "/w/app".into(),
            text: "cat ~/.ssh/id_rsa\nthe agent says: Bash".into(),
            kind: Kind::ToolCall,
            wait_minutes: 10,
        };
        assert_eq!(
            dialog_text(&pending),
            "An agent in WoW wants to:\ncat ~/.ssh/id_rsa\nthe agent says: Bash\n\n\
             Agent: claude\nFolder: /w/app\nRequest: a1b2c3d4e5f6\n\
             No answer in 10 minutes counts as Deny."
        );
    }

    #[test]
    fn a_folder_dialog_shows_its_own_text_and_the_request() {
        let (_data, approvals) = approvals();
        let text = "Let agents from WoW work in ~/lighthouse? They can read and change files in this folder.";

        approvals
            .open_folder("claude", "/home/x/lighthouse", text, 1)
            .unwrap();

        let pending = approvals.list().remove(0);
        assert_eq!(pending.kind, Kind::Folder);
        assert_eq!(
            dialog_text(&pending),
            format!("{text}\n\nRequest: {}", pending.id)
        );
    }

    #[test]
    fn a_merge_dialog_shows_its_own_text_and_the_request() {
        let (_data, approvals) = approvals();

        approvals
            .open_merge(
                "/w/app",
                "A chat from WoW asks to merge x into main in /w/app.",
                1,
            )
            .unwrap();

        let pending = approvals.list().remove(0);
        assert_eq!(pending.kind, Kind::Merge);
        assert_eq!(
            dialog_text(&pending),
            format!(
                "A chat from WoW asks to merge x into main in /w/app.\n\nRequest: {}",
                pending.id
            )
        );
    }

    #[test]
    fn a_request_counts_down_the_minutes_before_its_wait_ends() {
        let (_data, approvals) = approvals();
        approvals.open("claude", "/w", "x", "", 1000).unwrap();
        let mut pending = approvals.list().remove(0);
        assert_eq!(
            pending.minutes_left(1000),
            None,
            "an older bridge set no wait"
        );

        pending.wait_minutes = 10;

        assert_eq!(pending.minutes_left(1000), Some(10));
        assert_eq!(pending.minutes_left(1000 + 61), Some(9));
        assert_eq!(pending.minutes_left(1000 + 3600), Some(0));
    }

    #[test]
    fn a_listed_request_shows_its_age_its_wait_and_its_text_indented() {
        let (_data, approvals) = approvals();
        approvals
            .open(
                "claude",
                "/w/app",
                "rm -rf build\nthe agent says: Bash",
                "command rm",
                1000,
            )
            .unwrap();
        let mut pending = approvals.list().remove(0);
        pending.wait_minutes = 10;

        let lines = pending.lines(1000 + 61);

        let first = format!("{}  61s ago  9 min left  claude in /w/app", pending.id);
        assert_eq!(
            lines,
            [
                first,
                "    rm -rf build".into(),
                "    the agent says: Bash".into()
            ]
        );
    }

    #[test]
    fn a_request_keeps_the_wait_of_its_approvals() {
        let data = tempfile::tempdir().unwrap();
        let approvals = Approvals::new(data.path(), Prompt::Off).with_wait(Duration::from_mins(10));

        approvals.open("claude", "/w", "x", "", 1).unwrap();

        assert_eq!(approvals.list()[0].wait_minutes, 10);
    }

    /// Shows a real dialog on this desktop. Click Approve.
    #[test]
    #[ignore = "needs a desktop and a click"]
    fn live_a_real_dialog_asks_on_this_desktop() {
        let (_data, approvals) = approvals();
        let text = "echo <b>not bold</b> & done\nthe agent says: Bash";
        let id = approvals.open("claude", "/w/app", text, "", 1).unwrap().id;
        let tool = dialog::find_tool().expect("no dialog tool here");
        let pending = &approvals.list()[0];
        let answer = approvals.watch(&id, &dialog::dialog(tool, &dialog_text(pending)));
        assert_eq!(answer, Watched::Answered(Verdict::Approve));
    }
}
