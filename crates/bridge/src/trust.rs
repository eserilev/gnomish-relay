//! Trust on first use (SPEC.md 9.12): a chat in a folder under no root waits for one
//! click on the desktop, which adds the folder to `allowed_roots`. Only that click
//! changes the roots, and no addon can make it.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::agent::Control;
use crate::config::{self, with_tilde};
use crate::config_edit::{can_add_root, with_root};
use crate::desktop::{Approvals, Topic};
use crate::fs_safe::write_private;
use crate::gate;
use crate::new_folder::create_last;
use crate::relay::Job;
use crate::roots::Roots;
use crate::run::{log, now};
use crate::turn::{Answer, Turn};

/// After an answer that is not Approve, no folder dialog for this long, so a hostile
/// addon cannot fill the desktop with dialogs.
pub const QUIET: Duration = Duration::from_mins(10);

pub const NOT_WRITTEN: &str = "Couldn't add the folder: config.toml can't change. See bridge.log.";
const WAITS: &str =
    "Another folder waits for your answer on your desktop. Answer it, then send this again.";
const QUIET_NOW: &str = "No new folders for 10 minutes after one wasn't approved. Pick a folder that agents can already use.";
const DENIED: &str =
    "Denied on your desktop. Agents can't work in this folder. Pick another folder.";
const NO_ANSWER: &str =
    "No answer on your desktop. Send the message again in 10 minutes to ask again.";
const STOPPED: &str = "Stopped.";

/// How a folder request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trusted {
    /// The folder is a root now.
    Added,
    /// A Deny or a closed dialog: this folder gets no dialog until the bridge starts again.
    Denied,
    NoAnswer,
    /// Stop, or a new message of the chat.
    Stopped,
    /// An Approve, but `config.toml` did not change.
    NotWritten,
}

impl Trusted {
    /// The reply of a message that did not get its folder.
    pub fn text(self) -> &'static str {
        match self {
            Trusted::Added => "",
            Trusted::Denied => DENIED,
            Trusted::NoAnswer => NO_ANSWER,
            Trusted::Stopped => STOPPED,
            Trusted::NotWritten => NOT_WRITTEN,
        }
    }
}

/// The folder requests of the relay lane. Only its thread uses it, so it needs no lock.
#[derive(Default)]
pub struct TrustGuard {
    pending: bool,
    quiet_until: Option<Instant>,
    denied: BTreeSet<PathBuf>,
}

impl TrustGuard {
    /// One dialog at a time, none in the quiet time, and none for a denied folder.
    pub fn may_ask(&self, folder: &Path, now: Instant) -> Result<(), &'static str> {
        if self.denied.contains(folder) {
            return Err(DENIED);
        }
        if self.pending {
            return Err(WAITS);
        }
        if self.quiet_until.is_some_and(|until| now < until) {
            return Err(QUIET_NOW);
        }
        Ok(())
    }

    pub fn asked(&mut self) {
        self.pending = true;
    }

    pub fn answered(&mut self, folder: &Path, trusted: Trusted, now: Instant) {
        self.pending = false;
        if trusted == Trusted::Added {
            return;
        }
        self.quiet_until = Some(now + QUIET);
        if trusted == Trusted::Denied {
            self.denied.insert(folder.to_owned());
        }
    }
}

/// A character that is not a letter, a digit, or printable ASCII shows as `<U+XXXX>`,
/// so a bidi or zero-width character cannot hide a part of the path.
fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_graphic() || c == ' ' || c.is_alphanumeric() {
            out.push(c);
        } else {
            let _ = write!(out, "<U+{:04X}>", u32::from(c));
        }
    }
    out
}

/// Fixed text of the bridge and the real path, never text from the game.
pub fn folder_text(folder: &Path, home: &Path) -> String {
    format!(
        "Let agents from WoW work in {}? They can read and change files in this folder.",
        visible(&with_tilde(folder, home))
    )
}

/// What a folder request needs: the desktop requests, the config file, and the roots
/// that the running bridge shares.
#[derive(Clone, Debug)]
pub struct Truster {
    pub approvals: Approvals,
    pub config_dir: PathBuf,
    /// Resolved, so that it starts each resolved folder.
    pub home: PathBuf,
    pub permission_timeout: Duration,
    pub roots: Roots,
}

impl Truster {
    /// Checked before the dialog, so the user never approves a change that the bridge
    /// cannot write.
    pub fn can_trust(&self) -> Result<()> {
        can_add_root(&config::read_text(&self.config_dir)?, &self.home)
    }

    /// Runs in the thread of the run, before the agent starts. The game shows a notice
    /// with no buttons. `folder` is the real path.
    pub fn ask(&self, job: &Job, folder: &Path, control: &Control) -> Trusted {
        let text = folder_text(folder, &self.home);
        let shown = folder.to_string_lossy();
        let opened = self.approvals.open_folder(&job.agent, &shown, &text, now());
        let Ok(opened) = opened else {
            log(&format!("folder {shown}: no desktop request"));
            return Trusted::NoAnswer;
        };
        log(&format!("folder {shown}: asked as {}", opened.id));
        let mut turn = Turn::new(
            self.permission_timeout,
            self.permission_timeout,
            control.clone(),
        );
        let answer = gate::wait_on_the_desktop(&self.approvals, &opened, Topic::Folder, &mut turn);
        let trusted = match answer {
            Answer::Desktop(true) => self.grant(folder, job.new_folder),
            Answer::Desktop(false) => Trusted::Denied,
            _ if control.stop.requested() => Trusted::Stopped,
            _ => Trusted::NoAnswer,
        };
        log(&format!("folder {shown}: {trusted:?}"));
        trusted
    }

    /// Config load needs each root to exist, so a new folder comes first. The file is
    /// read again after the click, so a hand edit meanwhile stays.
    fn grant(&self, folder: &Path, new_folder: bool) -> Trusted {
        match self.write(folder, new_folder) {
            Ok(()) => {
                self.roots.add(folder.to_owned());
                Trusted::Added
            }
            Err(e) => {
                log(&format!("folder {}: not added: {e:#}", folder.display()));
                Trusted::NotWritten
            }
        }
    }

    fn write(&self, folder: &Path, new_folder: bool) -> Result<()> {
        if new_folder {
            create_last(folder).map_err(|e| anyhow::anyhow!(e.text()))?;
        }
        let text = config::read_text(&self.config_dir)?;
        let changed = with_root(&text, &with_tilde(folder, &self.home), &self.home)?;
        write_private(&self.config_dir, config::FILE, &changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Event, Events};
    use crate::config::Permission;
    use crate::desktop::{Prompt, Verdict};
    use crate::relay::{ChatId, MessageId, Session, Work};

    const CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"claude\"\n\
        [wow]\npath = \"~/wow\"\n\
        [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"ask\"\n";

    struct Home {
        _tmp: tempfile::TempDir,
        truster: Truster,
    }

    fn home(config: &str) -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(home.join("Code")).unwrap();
        std::fs::create_dir_all(home.join("lighthouse")).unwrap();
        let config_dir = home.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        write_private(&config_dir, config::FILE, config).unwrap();
        let truster = Truster {
            approvals: Approvals::new(&home.join("data"), Prompt::Off),
            config_dir,
            roots: Roots::new(vec![home.join("Code")]),
            home,
            permission_timeout: Duration::from_secs(10),
        };
        Home { _tmp: tmp, truster }
    }

    fn job() -> Job {
        Job {
            token: "tok".into(),
            chat: ChatId::new("c1"),
            id: MessageId(1),
            agent: "claude".into(),
            permission: Permission::Ask,
            asked: Permission::Ask,
            cwd: "/w".into(),
            session: Session::New,
            resume: None,
            text: "hi".into(),
            work: Work::Prompt,
            new_folder: false,
        }
    }

    fn config_text(home: &Home) -> String {
        config::read_text(&home.truster.config_dir).unwrap()
    }

    fn lighthouse(home: &Home) -> PathBuf {
        home.truster.home.join("lighthouse")
    }

    /// Answers the first desktop request as the command line does, and returns its text.
    fn answer_on_the_desktop(home: &Home, verdict: Verdict, folder: &Path) -> (Trusted, String) {
        let approvals = home.truster.approvals.clone();
        let answering = std::thread::spawn(move || {
            for _ in 0..500 {
                if let Some(open) = approvals.list().first() {
                    approvals.answer(&open.id, verdict).unwrap();
                    return open.text.clone();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            String::new()
        });
        let trusted = home.truster.ask(&job(), folder, &Control::default());
        (trusted, answering.join().unwrap())
    }

    #[test]
    fn the_dialog_names_the_folder_from_home_and_what_approve_gives() {
        let home = Path::new("/home/x");

        assert_eq!(
            folder_text(Path::new("/home/x/Documents/Code/lighthouse"), home),
            "Let agents from WoW work in ~/Documents/Code/lighthouse? They can read and change files in this folder."
        );
    }

    #[test]
    fn a_bidi_or_zero_width_character_in_the_folder_shows_as_an_escape() {
        let text = folder_text(
            Path::new("/home/x/app\u{202e}txt\u{200b}"),
            Path::new("/home/x"),
        );

        assert!(text.contains("~/app<U+202E>txt<U+200B>?"), "{text}");
        assert!(
            folder_text(Path::new("/home/x/Código"), Path::new("/home/x")).contains("~/Código?")
        );
    }

    #[test]
    fn an_approve_adds_exactly_that_folder_to_the_config_and_to_the_running_roots() {
        let home = home(CONFIG);
        let folder = lighthouse(&home);

        let (trusted, text) = answer_on_the_desktop(&home, Verdict::Approve, &folder);

        assert_eq!(trusted, Trusted::Added);
        assert!(
            text.starts_with("Let agents from WoW work in ~/lighthouse?"),
            "{text}"
        );
        assert!(
            config_text(&home).starts_with("allowed_roots = [\"~/Code\", \"~/lighthouse\"]\n"),
            "{}",
            config_text(&home)
        );
        assert!(home.truster.roots.hold(&folder));
        assert!(!home.truster.roots.hold(&home.truster.home));
        assert!(home.truster.approvals.list().is_empty());
    }

    #[test]
    fn an_approve_for_a_new_folder_makes_it_before_the_config_names_it() {
        let home = home(CONFIG);
        let folder = home.truster.home.join("fresh");
        let mut asking = job();
        asking.new_folder = true;
        let approvals = home.truster.approvals.clone();
        let answering = std::thread::spawn(move || {
            for _ in 0..500 {
                if let Some(open) = approvals.list().first() {
                    approvals.answer(&open.id, Verdict::Approve).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        let trusted = home.truster.ask(&asking, &folder, &Control::default());
        answering.join().unwrap();

        assert_eq!(trusted, Trusted::Added);
        assert!(folder.is_dir());
        assert!(config_text(&home).contains("\"~/fresh\""));
    }

    #[test]
    fn a_deny_keeps_the_config_and_the_roots() {
        let home = home(CONFIG);
        let folder = lighthouse(&home);

        let (trusted, _) = answer_on_the_desktop(&home, Verdict::Deny, &folder);

        assert_eq!(trusted, Trusted::Denied);
        assert_eq!(config_text(&home), CONFIG);
        assert!(!home.truster.roots.hold(&folder));
    }

    #[test]
    fn no_answer_keeps_the_config() {
        let mut home = home(CONFIG);
        home.truster.permission_timeout = Duration::from_millis(200);

        let trusted = home
            .truster
            .ask(&job(), &lighthouse(&home), &Control::default());

        assert_eq!(trusted, Trusted::NoAnswer);
        assert_eq!(config_text(&home), CONFIG);
    }

    #[test]
    fn an_approve_that_cannot_change_the_config_adds_no_root() {
        let multi = CONFIG.replace("[\"~/Code\"]\n", "[\n  \"~/Code\",\n]\n");
        let home = home(&multi);
        let folder = lighthouse(&home);

        assert!(home.truster.can_trust().is_err());
        let (trusted, _) = answer_on_the_desktop(&home, Verdict::Approve, &folder);

        assert_eq!(trusted, Trusted::NotWritten);
        assert_eq!(config_text(&home), multi);
        assert!(!home.truster.roots.hold(&folder));
    }

    #[test]
    fn a_folder_request_shows_a_notice_in_the_game_and_no_game_request() {
        let mut home = home(CONFIG);
        home.truster.permission_timeout = Duration::from_millis(200);
        let (to, events) = std::sync::mpsc::channel();
        let control = Control {
            events: Events::to_bridge(to, &job()),
            ..Control::default()
        };

        home.truster.ask(&job(), &lighthouse(&home), &control);

        let lines: Vec<String> = events
            .try_iter()
            .map(|(_, _, event)| match event {
                Event::Desktop(notice) => notice.line(),
                Event::Question(_) => "a game request".into(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("Desktop: wait "), "{lines:?}");
        assert!(lines[0].ends_with(" folder"), "{lines:?}");
        assert!(lines[1].starts_with("Desktop: none "), "{lines:?}");
    }

    #[test]
    fn one_folder_request_waits_at_a_time() {
        let mut guard = TrustGuard::default();
        let now = Instant::now();

        assert_eq!(guard.may_ask(Path::new("/h/a"), now), Ok(()));
        guard.asked();

        assert_eq!(guard.may_ask(Path::new("/h/b"), now), Err(WAITS));
        guard.answered(Path::new("/h/a"), Trusted::Added, now);
        assert_eq!(guard.may_ask(Path::new("/h/b"), now), Ok(()));
    }

    #[test]
    fn an_answer_that_is_not_approve_starts_ten_quiet_minutes() {
        let mut guard = TrustGuard::default();
        let now = Instant::now();
        guard.asked();

        guard.answered(Path::new("/h/a"), Trusted::NoAnswer, now);

        assert_eq!(guard.may_ask(Path::new("/h/b"), now), Err(QUIET_NOW));
        assert_eq!(guard.may_ask(Path::new("/h/b"), now + QUIET), Ok(()));
    }

    #[test]
    fn a_denied_folder_gets_no_dialog_again_after_the_quiet_time() {
        let mut guard = TrustGuard::default();
        let now = Instant::now();
        guard.asked();

        guard.answered(Path::new("/h/a"), Trusted::Denied, now);

        assert_eq!(guard.may_ask(Path::new("/h/a"), now + QUIET), Err(DENIED));
        assert_eq!(guard.may_ask(Path::new("/h/b"), now + QUIET), Ok(()));
    }
}
