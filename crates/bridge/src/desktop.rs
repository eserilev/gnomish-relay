//! Approvals on the desktop (SPEC.md 6.6.3): a tool call that only the user at the
//! computer can allow. Each open request is a file in the data folder. A dialog of the
//! OS, `gnomish-relay approve`, or `gnomish-relay deny` answers it, and the first
//! answer wins. No addon can write these files or click the dialog.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::dialog::{self, Dialog, Shown, show_notice};
use crate::fs_safe::check_real_dir;
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
}

/// What a desktop request asks for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    ToolCall,
    /// A higher level in `config.toml` (SPEC.md 9.3).
    Raise,
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
    pub fn new(data: &Path, prompt: Prompt) -> Approvals {
        Approvals {
            dir: data.join(FOLDER),
            prompt,
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

    /// Writes a new request, shows a dialog, and returns its id.
    pub fn open(&self, agent: &str, folder: &str, text: &str, now: u32) -> Result<String> {
        self.open_kind(agent, folder, text, now, Kind::ToolCall)
    }

    /// A request to write a higher level for `agent` into `config_file`.
    pub fn open_raise(
        &self,
        agent: &str,
        config_file: &str,
        text: &str,
        now: u32,
    ) -> Result<String> {
        self.open_kind(agent, config_file, text, now, Kind::Raise)
    }

    fn open_kind(
        &self,
        agent: &str,
        folder: &str,
        text: &str,
        now: u32,
        kind: Kind,
    ) -> Result<String> {
        self.ready_dir()?;
        let id = new_id()?;
        let pending = Pending {
            id: id.clone(),
            created: now,
            agent: agent.to_owned(),
            folder: folder.to_owned(),
            text: text.to_owned(),
            kind,
        };
        write_new(&self.file(&id, REQUEST), &serde_json::to_vec(&pending)?)?;
        eprintln!(
            "{now} approve on the desktop: gnomish-relay approve {id} ({})",
            text.escape_debug()
        );
        if self.prompt == Prompt::Dialog {
            let approvals = self.clone();
            std::thread::spawn(move || approvals.ask_the_desktop(&pending));
        }
        Ok(id)
    }

    /// With no dialog tool, a plain notice names the command that answers.
    fn ask_the_desktop(&self, pending: &Pending) {
        let text = dialog_text(pending);
        let Some(tool) = dialog::find_tool() else {
            log(&format!("desktop request {}: no dialog tool", pending.id));
            show_notice(&format!(
                "{text}\nRun: gnomish-relay approve {}",
                pending.id
            ));
            return;
        };
        log(&format!("desktop request {}: dialog {tool:?}", pending.id));
        let answer = self.watch(&pending.id, &dialog::dialog(tool, &text));
        log(&format!(
            "desktop request {}: {answer:?} in the dialog",
            pending.id
        ));
    }

    /// Shows `dialog` until it answers, or until the request no longer waits: an
    /// answer from the command line, a Deny in the game, or the timeout. Returns the
    /// answer of the dialog when it counted.
    pub fn watch(&self, id: &str, dialog: &Dialog) -> Option<Verdict> {
        let mut shown = Shown::start(dialog)?;
        loop {
            if let Some(approve) = shown.answer() {
                let verdict = if approve {
                    Verdict::Approve
                } else {
                    Verdict::Deny
                };
                return self.answer(id, verdict).ok().map(|()| verdict);
            }
            if !self.is_waiting(id) {
                shown.stop();
                return None;
            }
            std::thread::sleep(POLL);
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

/// The honest text of S15, then who asks, and the id for the command line.
pub fn dialog_text(pending: &Pending) -> String {
    match pending.kind {
        Kind::ToolCall => format!(
            "An agent from the game asks to:\n{}\n\nAgent: {}. Folder: {}. Request {}.",
            pending.text, pending.agent, pending.folder, pending.id
        ),
        Kind::Raise => format!(
            "{}\n\nConfig: {}. Request {}.",
            pending.text, pending.folder, pending.id
        ),
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

    #[cfg(unix)]
    fn fake(script: &str) -> Dialog {
        Dialog {
            tool: dialog::Tool::NotifySend,
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env: Vec::new(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_click_on_approve_in_the_dialog_answers_the_request() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        let answer = approvals.watch(&id, &fake("echo approve"));
        assert_eq!(answer, Some(Verdict::Approve));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Approve));
    }

    #[cfg(unix)]
    #[test]
    fn a_dismissed_dialog_denies() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        assert_eq!(approvals.watch(&id, &fake("exit 0")), Some(Verdict::Deny));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[cfg(unix)]
    #[test]
    fn the_first_answer_wins_and_stops_the_dialog() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        approvals.answer(&id, Verdict::Deny).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(approvals.watch(&id, &fake("sleep 30; echo approve")), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(approvals.answer_of(&id), Some(Verdict::Deny));
    }

    #[cfg(unix)]
    #[test]
    fn a_closed_request_stops_its_dialog() {
        let (_data, approvals) = approvals();
        let id = approvals.open("claude", "/w", "x", 1).unwrap();
        let closing = approvals.clone();
        let closed = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            closing.close(&closed);
        });
        assert_eq!(approvals.watch(&id, &fake("sleep 30; echo approve")), None);
        assert_eq!(approvals.answer_of(&id), None);
    }

    #[test]
    fn the_dialog_shows_the_popup_text_and_who_asks() {
        let pending = Pending {
            id: "a1b2c3d4e5f6".into(),
            created: 1,
            agent: "claude".into(),
            folder: "/w/app".into(),
            text: "cat ~/.ssh/id_rsa\nthe agent says: Bash".into(),
            kind: Kind::ToolCall,
        };
        assert_eq!(
            dialog_text(&pending),
            "An agent from the game asks to:\ncat ~/.ssh/id_rsa\nthe agent says: Bash\n\n\
             Agent: claude. Folder: /w/app. Request a1b2c3d4e5f6."
        );
    }

    /// Shows a real dialog on this desktop. Click Approve.
    #[test]
    #[ignore = "needs a desktop and a click"]
    fn live_a_real_dialog_asks_on_this_desktop() {
        let (_data, approvals) = approvals();
        let text = "echo <b>not bold</b> & done\nthe agent says: Bash";
        let id = approvals.open("claude", "/w/app", text, 1).unwrap();
        let tool = dialog::find_tool().expect("no dialog tool here");
        let pending = &approvals.list()[0];
        let answer = approvals.watch(&id, &dialog::dialog(tool, &dialog_text(pending)));
        assert_eq!(answer, Some(Verdict::Approve));
    }
}
