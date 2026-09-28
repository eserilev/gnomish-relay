//! A raise of the level of an agent in `config.toml` (SPEC.md 6.6.2, 9.3). A chat
//! that asks for more than the config allows gets one desktop dialog. Only a click
//! on the desktop changes the config, and no addon can make it (S6).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::agent::Control;
use crate::config::{self, Permission};
use crate::config_edit::with_permission;
use crate::desktop::Approvals;
use crate::fs_safe::write_private;
use crate::gate;
use crate::relay::Job;
use crate::run::{log, now};
use crate::turn::{Answer, Turn};

/// After an answer that is not Approve, no dialog for this long, so a hostile addon
/// cannot fill the desktop with dialogs.
pub const QUIET: Duration = Duration::from_mins(10);

/// How a raise ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raised {
    Approved,
    /// A Deny or a closed dialog on the desktop: no more dialogs for this agent until
    /// the bridge starts again.
    DeniedOnTheDesktop,
    /// No answer, Stop, or a config that the bridge cannot write.
    NotRaised,
}

/// The raises of the relay lane. Only its thread uses it, so it needs no lock.
#[derive(Default)]
pub struct RaiseGuard {
    pending: bool,
    quiet_until: Option<Instant>,
    refused: BTreeSet<String>,
}

impl RaiseGuard {
    /// One dialog at a time, none in the quiet time, and none for a refused agent.
    pub fn may_ask(&self, agent: &str, now: Instant) -> bool {
        !self.pending
            && !self.refused.contains(agent)
            && self.quiet_until.is_none_or(|until| now >= until)
    }

    pub fn asked(&mut self) {
        self.pending = true;
    }

    pub fn answered(&mut self, agent: &str, raised: Raised, now: Instant) {
        self.pending = false;
        if raised == Raised::Approved {
            return;
        }
        self.quiet_until = Some(now + QUIET);
        if raised == Raised::DeniedOnTheDesktop {
            self.refused.insert(agent.to_owned());
        }
    }
}

/// Whether the commands of an agent ask in the game at `auto-edit`. A `command` agent
/// has no way to ask (SPEC.md 9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commands {
    Ask,
    NoQuestion,
}

/// Fixed text and the name of the agent from the config, never text from the game.
pub fn raise_text(agent: &str, level: Permission, commands: Commands) -> String {
    let word = level.word();
    match (level, commands) {
        (Permission::AutoEdit, Commands::NoQuestion) => format!(
            "A chat from WoW asks for more access. Allow {agent} to edit files in the chat \
             folder AND run its own commands with no question, in every chat from WoW? It \
             runs them inside the sandbox. This writes permission = \"{word}\" to \
             config.toml. Approve only if you just sent a message from WoW."
        ),
        (Permission::FullAuto, _) => format!(
            "A chat from WoW asks for full access. Allow {agent} to edit files AND run \
             commands with no question, in every chat from WoW? Any addon that can send a \
             chat message can then run code on this computer, inside the sandbox. The \
             Gnomish Relay addon never asks for this by itself. This writes \
             permission = \"{word}\" to config.toml."
        ),
        (Permission::AutoEdit | Permission::Ask, _) => format!(
            "A chat from WoW asks for more access. Allow {agent} to edit files in the chat \
             folder with no question, in every chat from WoW? Commands still ask in the \
             game, unless you added an Always rule there. This writes \
             permission = \"{word}\" to config.toml. Approve only if you just sent a message \
             from WoW."
        ),
    }
}

/// What a raise needs: the desktop requests, and the config file.
#[derive(Clone, Debug)]
pub struct Raiser {
    pub approvals: Approvals,
    pub config_dir: PathBuf,
    pub home: PathBuf,
    pub permission_timeout: Duration,
    /// The agents of kind `command`, whose commands never ask.
    pub free_commands: Vec<String>,
}

impl Raiser {
    fn changed_config(&self, agent: &str, level: Permission) -> Result<String> {
        let text = config::read_text(&self.config_dir)?;
        with_permission(&text, agent, level, &self.home)
    }

    /// Checked before the dialog, so the user never approves a change that the bridge
    /// cannot write.
    pub fn can_raise(&self, agent: &str, level: Permission) -> Result<()> {
        self.changed_config(agent, level).map(|_| ())
    }

    /// The file is read again after the click, so a hand edit meanwhile stays.
    pub fn write(&self, agent: &str, level: Permission) -> Result<()> {
        let text = self.changed_config(agent, level)?;
        write_private(&self.config_dir, config::FILE, &text)
    }

    /// Runs in the thread of the run, before the agent starts. The game shows a
    /// notice with no buttons.
    pub fn ask(&self, job: &Job, level: Permission, control: &Control) -> Raised {
        let agent = &job.agent;
        let file = self.config_dir.join(config::FILE);
        let commands = if self.free_commands.contains(agent) {
            Commands::NoQuestion
        } else {
            Commands::Ask
        };
        let text = raise_text(agent, level, commands);
        let opened = self
            .approvals
            .open_raise(agent, &file.to_string_lossy(), &text, now());
        let Ok(opened) = opened else {
            log(&format!("raise {agent}: no desktop request"));
            return Raised::NotRaised;
        };
        log(&format!(
            "raise {agent} to {}: asked as {}",
            level.word(),
            opened.id
        ));
        let mut turn = Turn::new(
            self.permission_timeout,
            self.permission_timeout,
            control.clone(),
        );
        let answer = gate::wait_on_the_desktop(&self.approvals, &opened, Some(level), &mut turn);
        let raised = self.outcome(agent, level, &answer);
        log(&format!("raise {agent}: {raised:?}"));
        raised
    }

    fn outcome(&self, agent: &str, level: Permission, answer: &Answer) -> Raised {
        match answer {
            Answer::Desktop(true) => match self.write(agent, level) {
                Ok(()) => Raised::Approved,
                Err(e) => {
                    log(&format!("raise {agent}: config.toml not written: {e:#}"));
                    Raised::NotRaised
                }
            },
            Answer::Desktop(false) => Raised::DeniedOnTheDesktop,
            Answer::Game(_) | Answer::None | Answer::NewMessage | Answer::Covered => {
                Raised::NotRaised
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{Prompt, Verdict};
    use crate::relay::{ChatId, MessageId, Session, Work};

    const CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"claude\"\n\
        [wow]\npath = \"~/wow\"\n\
        [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"ask\"\n";

    struct Home {
        _tmp: tempfile::TempDir,
        raiser: Raiser,
    }

    fn home(config: &str) -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_owned();
        std::fs::create_dir_all(home.join("Code")).unwrap();
        let config_dir = home.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        write_private(&config_dir, config::FILE, config).unwrap();
        let raiser = Raiser {
            approvals: Approvals::new(&home.join("data"), Prompt::Off),
            config_dir,
            home,
            permission_timeout: Duration::from_secs(10),
            free_commands: Vec::new(),
        };
        Home { _tmp: tmp, raiser }
    }

    fn job() -> Job {
        Job {
            token: "tok".into(),
            chat: ChatId("c1".into()),
            id: MessageId(1),
            agent: "claude".into(),
            permission: Permission::Ask,
            asked: Permission::AutoEdit,
            cwd: "/w".into(),
            session: Session::New,
            resume: None,
            text: "hi".into(),
            work: Work::Prompt,
            new_folder: false,
        }
    }

    fn config_text(home: &Home) -> String {
        config::read_text(&home.raiser.config_dir).unwrap()
    }

    /// Answers the first desktop request as the command line does.
    fn answer_on_the_desktop(home: &Home, verdict: Verdict) -> Raised {
        let approvals = home.raiser.approvals.clone();
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
        let raised = home
            .raiser
            .ask(&job(), Permission::AutoEdit, &Control::default());
        assert!(
            answering
                .join()
                .unwrap()
                .contains("Allow claude to edit files")
        );
        raised
    }

    #[test]
    fn an_approve_on_the_desktop_writes_the_new_level_into_the_config() {
        let home = home(CONFIG);
        assert_eq!(
            answer_on_the_desktop(&home, Verdict::Approve),
            Raised::Approved
        );
        assert!(config_text(&home).contains("permission = \"auto-edit\""));
        assert!(home.raiser.approvals.list().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_written_config_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let home = home(CONFIG);
        answer_on_the_desktop(&home, Verdict::Approve);
        let file = home.raiser.config_dir.join(config::FILE);
        let mode = std::fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_deny_on_the_desktop_keeps_the_config() {
        let home = home(CONFIG);
        let raised = answer_on_the_desktop(&home, Verdict::Deny);
        assert_eq!(raised, Raised::DeniedOnTheDesktop);
        assert_eq!(config_text(&home), CONFIG);
    }

    #[test]
    fn no_answer_keeps_the_config() {
        let mut home = home(CONFIG);
        home.raiser.permission_timeout = Duration::from_millis(200);
        let raised = home
            .raiser
            .ask(&job(), Permission::AutoEdit, &Control::default());
        assert_eq!(raised, Raised::NotRaised);
        assert_eq!(config_text(&home), CONFIG);
    }

    #[test]
    fn a_raise_shows_a_notice_in_the_game_and_no_game_request() {
        use crate::agent::{Event, Events};
        let mut home = home(CONFIG);
        home.raiser.permission_timeout = Duration::from_millis(200);
        let (to, events) = std::sync::mpsc::channel();
        let control = Control {
            events: Events::to_bridge(to, &job()),
            ..Control::default()
        };

        home.raiser.ask(&job(), Permission::AutoEdit, &control);

        let lines: Vec<String> = events
            .try_iter()
            .map(|(_, _, event)| match event {
                Event::Desktop(notice) => notice.line(),
                Event::Question(_) => "a game request".into(),
                Event::Progress(_) | Event::Raised { .. } | Event::Withdrawn => String::new(),
            })
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("Desktop: wait "), "{lines:?}");
        assert!(lines[0].ends_with(" raise auto-edit"), "{lines:?}");
        assert!(lines[1].starts_with("Desktop: none "), "{lines:?}");
    }

    #[test]
    fn a_config_that_the_bridge_cannot_change_gets_no_dialog() {
        let dotted = CONFIG.replace("[agents.claude]", "[agents.\"claude\"]");
        let home = home(&dotted);
        assert!(
            home.raiser
                .can_raise("claude", Permission::AutoEdit)
                .is_err()
        );
        let fine = self::home(CONFIG);
        assert!(
            fine.raiser
                .can_raise("claude", Permission::AutoEdit)
                .is_ok()
        );
    }

    #[test]
    fn a_config_that_turned_malformed_after_the_check_is_never_written() {
        let home = home(CONFIG);
        let broken = CONFIG.replace("[wow]", "[wow");
        write_private(&home.raiser.config_dir, config::FILE, &broken).unwrap();
        assert!(home.raiser.write("claude", Permission::AutoEdit).is_err());
        assert_eq!(config_text(&home), broken);
    }

    #[test]
    fn one_dialog_at_a_time() {
        let mut guard = RaiseGuard::default();
        let now = Instant::now();
        assert!(guard.may_ask("claude", now));
        guard.asked();
        assert!(!guard.may_ask("claude", now));
        assert!(!guard.may_ask("codex", now));
        guard.answered("claude", Raised::Approved, now);
        assert!(guard.may_ask("claude", now));
    }

    #[test]
    fn every_answer_but_approve_starts_ten_quiet_minutes() {
        let mut guard = RaiseGuard::default();
        let now = Instant::now();
        guard.asked();
        guard.answered("claude", Raised::NotRaised, now);
        assert!(!guard.may_ask("claude", now + Duration::from_mins(9)));
        assert!(!guard.may_ask("codex", now + Duration::from_mins(9)));
        assert!(guard.may_ask("claude", now + QUIET));
    }

    #[test]
    fn a_deny_on_the_desktop_ends_the_dialogs_for_that_agent_until_a_restart() {
        let mut guard = RaiseGuard::default();
        let now = Instant::now();
        guard.asked();
        guard.answered("claude", Raised::DeniedOnTheDesktop, now);
        let later = now + QUIET + Duration::from_hours(1);
        assert!(!guard.may_ask("claude", later));
        assert!(guard.may_ask("codex", later));
    }

    #[test]
    fn full_auto_gets_a_stronger_warning() {
        let text = raise_text("claude", Permission::FullAuto, Commands::Ask);
        assert!(text.contains("run commands with no question"), "{text}");
        assert!(text.contains("never asks for this by itself"), "{text}");
        let text = raise_text("claude", Permission::AutoEdit, Commands::Ask);
        assert!(
            text.contains("Commands still ask in the game, unless you added an Always rule there."),
            "{text}"
        );
        let text = raise_text("aider", Permission::AutoEdit, Commands::NoQuestion);
        assert!(
            text.contains("AND run its own commands with no question"),
            "{text}"
        );
        assert!(!text.contains("still ask"), "{text}");
    }
}
