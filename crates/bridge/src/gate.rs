//! The gate: one answer for each tool call of a run from the game (SPEC.md 6.6.3). The
//! classifier of `protocol` gives a verdict, the level of the job bounds it (S6, 6.6.2),
//! and the answer then comes from the game, the desktop, or nobody.

use std::path::{Path, PathBuf};

use protocol::action::{ToolCall, Verdict, classify};
use protocol::live::OptionKind;

use crate::action_input::{self, resolve};
use crate::agent::Choice;
use crate::allow::AllowTable;
use crate::command_sandbox::CommandSandbox;
use crate::config::{Permission, RelayConfig};
use crate::desktop::{self, Approvals, Notice, Opened, Prompt, Waiting};
use crate::turn::{Answer, Turn};

pub const NOT_FROM_THE_GAME: &str = "Not allowed from the game.";
pub const NEW_MESSAGE: &str = "The player sent a new message.";

/// What a tool call does, for the level `ask`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// It only reads files.
    Read,
    /// It writes, or it is a tool that the classifier does not know.
    Change,
    /// It runs a shell command.
    Command,
}

/// Whether the commands of the run are inside a sandbox (SPEC.md 6.6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sandboxing {
    On,
    Off,
}

/// Which tool calls of the agent reach the bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Every tool call, before it runs: the hook of Claude, the approvals of Codex.
    Every,
    /// Only the calls that the agent asks about: other ACP agents.
    Asked,
}

/// What happens to a tool call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Run,
    AskGame,
    AskDesktop,
    Refuse,
}

/// The whole rule of SPEC.md 9.3. `full-auto` skips only the questions of the game
/// (6.6.2). An agent that picks what it asks never runs a call with no question, since
/// the calls it does not ask about already ran. At `ask`, only a read runs with no question.
pub fn decide(level: Permission, verdict: Verdict, effect: Effect, coverage: Coverage) -> Step {
    match verdict {
        Verdict::Deny => Step::Refuse,
        Verdict::Desktop => Step::AskDesktop,
        Verdict::Ask if level == Permission::FullAuto && coverage == Coverage::Every => Step::Run,
        Verdict::Ask => Step::AskGame,
        Verdict::Allow if coverage == Coverage::Asked => Step::AskGame,
        Verdict::Allow if level == Permission::Ask && effect != Effect::Read => Step::AskGame,
        Verdict::Allow => Step::Run,
    }
}

/// With no sandbox, a command that runs code can do anything that the user can, so it
/// never runs with no question (SPEC.md 6.6.4, the fallback).
pub fn without_sandbox(step: Step, effect: Effect, sandboxing: Sandboxing) -> Step {
    let unguarded = sandboxing == Sandboxing::Off && effect == Effect::Command;
    if unguarded && step == Step::Run {
        return Step::AskGame;
    }
    step
}

/// One tool call as a backend saw it.
pub struct Call {
    pub tool: ToolCall,
    pub effect: Effect,
    /// The popup text (S15).
    pub text: Vec<u8>,
    /// A short name of the call, for "Not allowed from the game:".
    pub title: String,
}

impl Call {
    pub fn files(reads: &[PathBuf], writes: &[PathBuf], text: Vec<u8>, title: String) -> Call {
        Call {
            tool: action_input::file_call(reads, writes),
            effect: if writes.is_empty() {
                Effect::Read
            } else {
                Effect::Change
            },
            text,
            title,
        }
    }

    pub fn command(command: &str, cwd: &Path, text: Vec<u8>, title: String) -> Call {
        Call {
            tool: action_input::command_call(command, cwd),
            effect: Effect::Command,
            text,
            title,
        }
    }

    pub fn unknown(text: Vec<u8>, title: String) -> Call {
        Call {
            tool: ToolCall::Unknown,
            effect: Effect::Change,
            text,
            title,
        }
    }
}

/// The run that a call belongs to.
pub struct Job<'a> {
    pub agent: &'a str,
    /// The folder of the chat.
    pub cwd: &'a str,
    pub level: Permission,
    pub coverage: Coverage,
    pub sandboxing: Sandboxing,
}

/// Why a call does not run.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The user said no in the game.
    ByUser,
    /// A rule, the desktop, or no answer said no.
    ByRule(String),
}

impl Refusal {
    fn by_rule(reason: &str) -> Refusal {
        Refusal::ByRule(reason.to_owned())
    }

    /// The text that the agent sees.
    pub fn reason(&self) -> &str {
        match self {
            Refusal::ByUser => "Denied in the game.",
            Refusal::ByRule(reason) => reason,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Gate {
    /// `allowed_roots`, resolved.
    pub roots: Vec<PathBuf>,
    /// The config folder of the bridge. Every path in it is `deny`.
    pub config_dir: PathBuf,
    /// The data folder of the bridge: its state, locks, log, and desktop requests. Every
    /// path in it is `deny`, and the bridge writes them itself, never through this gate.
    pub data_dir: PathBuf,
    pub allow: std::sync::Arc<AllowTable>,
    pub approvals: Approvals,
    pub sandbox: CommandSandbox,
}

impl Gate {
    /// The gate of the bridge: `config_dir` holds `config.toml`, and `data_dir` holds the
    /// desktop requests.
    pub fn new(config: &RelayConfig, config_dir: &Path, data_dir: &Path, prompt: Prompt) -> Gate {
        let roots = config
            .policy
            .folders
            .roots
            .iter()
            .map(|r| PathBuf::from(String::from_utf8_lossy(r).into_owned()))
            .collect();
        Gate {
            roots,
            config_dir: config_dir.to_owned(),
            data_dir: data_dir.to_owned(),
            allow: std::sync::Arc::new(config.allow.clone()),
            approvals: Approvals::new(data_dir, prompt),
            sandbox: CommandSandbox::detect(config.hosts.clone(), &config.local_ports),
        }
    }

    fn verdict(&self, call: &Call, chat: &Path) -> Verdict {
        let deny =
            [&self.config_dir, &self.data_dir].map(|d| resolve(d).unwrap_or_else(|| d.clone()));
        let rules = self.allow.rules_for(chat);
        let policy = action_input::policy(&self.roots, chat, &deny, &rules);
        classify(&call.tool, &policy, &[])
    }

    /// `Ok` when the call runs. A question waits in the game or on the desktop.
    pub fn check(&self, call: &Call, job: &Job, turn: &mut Turn) -> Result<(), Refusal> {
        let chat = resolve(Path::new(job.cwd)).unwrap_or_else(|| PathBuf::from(job.cwd));
        let verdict = self.verdict(call, &chat);
        let step = decide(job.level, verdict, call.effect, job.coverage);
        match without_sandbox(step, call.effect, job.sandboxing) {
            Step::Run => Ok(()),
            Step::Refuse => Err(Refusal::by_rule(
                "It touches the config or data folder of Gnomish Relay, which the agent never reaches.",
            )),
            Step::AskGame => ask_game(call, turn),
            Step::AskDesktop => self.ask_desktop(call, job, turn),
        }
    }

    /// The game shows a notice with no buttons: only the desktop answers.
    fn ask_desktop(&self, call: &Call, job: &Job, turn: &mut Turn) -> Result<(), Refusal> {
        let text = String::from_utf8_lossy(&call.text).into_owned();
        let opened = self
            .approvals
            .open(job.agent, job.cwd, &text, crate::run::now())
            .map_err(|e| Refusal::by_rule(&format!("No desktop approval: {e:#}")))?;
        let answer = wait_on_the_desktop(&self.approvals, &opened, None, turn);
        match answer {
            Answer::Desktop(true) => Ok(()),
            Answer::Desktop(false) => Err(Refusal::by_rule("Denied on the desktop.")),
            Answer::NewMessage => Err(Refusal::by_rule(NEW_MESSAGE)),
            Answer::Game(_) | Answer::None => Err(Refusal::by_rule("No answer on the desktop.")),
        }
    }
}

/// Shows the wait in the game, waits for the desktop, and closes the request. `raise`
/// is the level of a raise (SPEC.md 9.3).
pub fn wait_on_the_desktop(
    approvals: &Approvals,
    opened: &Opened,
    raise: Option<Permission>,
    turn: &mut Turn,
) -> Answer {
    let notice = Notice {
        id: opened.id.clone(),
        prompted: opened.prompted,
        waiting: Waiting::Open,
        raise,
    };
    turn.desktop(notice.clone());
    let answer_of = || {
        approvals
            .answer_of(&opened.id)
            .map(|v| v == desktop::Verdict::Approve)
    };
    let answer = turn.wait_desktop(&answer_of);
    approvals.close(&opened.id);
    let waiting = match answer {
        Answer::Desktop(true) => Waiting::Approved,
        Answer::Desktop(false) => Waiting::Denied,
        Answer::Game(_) | Answer::None | Answer::NewMessage => Waiting::NoAnswer,
    };
    turn.desktop(notice.ended(waiting));
    answer
}

/// Allow and Deny. The game never gets "allow always" (6.6.5).
pub fn game_choices() -> Vec<Choice> {
    vec![
        Choice {
            kind: OptionKind::AllowOnce,
            label: "Allow".into(),
        },
        Choice {
            kind: OptionKind::RejectOnce,
            label: "Deny".into(),
        },
    ]
}

fn ask_game(call: &Call, turn: &mut Turn) -> Result<(), Refusal> {
    if !turn.listening() {
        return Err(Refusal::by_rule(NOT_FROM_THE_GAME));
    }
    match turn.ask_game(call.text.clone(), game_choices()) {
        Answer::Game(0) => Ok(()),
        Answer::Game(_) => Err(Refusal::ByUser),
        Answer::NewMessage => Err(Refusal::by_rule(NEW_MESSAGE)),
        Answer::Desktop(_) | Answer::None => Err(Refusal::by_rule("No answer from the game.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: [Permission; 3] = [Permission::Ask, Permission::AutoEdit, Permission::FullAuto];

    #[test]
    fn deny_refuses_and_desktop_asks_the_desktop_at_every_level() {
        for level in LEVELS {
            for effect in [Effect::Read, Effect::Change] {
                for coverage in [Coverage::Every, Coverage::Asked] {
                    assert_eq!(decide(level, Verdict::Deny, effect, coverage), Step::Refuse);
                    assert_eq!(
                        decide(level, Verdict::Desktop, effect, coverage),
                        Step::AskDesktop
                    );
                }
            }
        }
    }

    /// Every pair of level and verdict for an agent whose every call reaches the bridge.
    #[test]
    fn the_table_of_levels_and_verdicts_for_a_change() {
        use Step::*;
        let table = [
            (Permission::Ask, [Refuse, AskDesktop, AskGame, AskGame]),
            (Permission::AutoEdit, [Refuse, AskDesktop, AskGame, Run]),
            (Permission::FullAuto, [Refuse, AskDesktop, Run, Run]),
        ];
        let verdicts = [
            Verdict::Deny,
            Verdict::Desktop,
            Verdict::Ask,
            Verdict::Allow,
        ];
        for (level, steps) in table {
            for (verdict, step) in verdicts.into_iter().zip(steps) {
                assert_eq!(
                    decide(level, verdict, Effect::Change, Coverage::Every),
                    step
                );
            }
        }
    }

    #[test]
    fn at_the_level_ask_an_allowed_read_runs_and_an_allowed_change_asks() {
        let read = decide(
            Permission::Ask,
            Verdict::Allow,
            Effect::Read,
            Coverage::Every,
        );
        let change = decide(
            Permission::Ask,
            Verdict::Allow,
            Effect::Change,
            Coverage::Every,
        );
        assert_eq!((read, change), (Step::Run, Step::AskGame));
    }

    #[test]
    fn an_agent_that_picks_its_questions_asks_the_game_even_at_full_auto() {
        for level in LEVELS {
            for verdict in [Verdict::Ask, Verdict::Allow] {
                let step = decide(level, verdict, Effect::Read, Coverage::Asked);
                assert_eq!(step, Step::AskGame);
            }
        }
    }

    #[test]
    fn with_no_sandbox_a_command_asks_the_game_at_every_level() {
        for level in LEVELS {
            for verdict in [Verdict::Ask, Verdict::Allow] {
                let step = decide(level, verdict, Effect::Command, Coverage::Every);
                let step = without_sandbox(step, Effect::Command, Sandboxing::Off);
                assert_eq!(step, Step::AskGame, "{level:?}");
            }
        }
    }

    #[test]
    fn with_no_sandbox_a_file_edit_and_a_refusal_stay_as_they_are() {
        let edit = without_sandbox(Step::Run, Effect::Change, Sandboxing::Off);
        let desktop = without_sandbox(Step::AskDesktop, Effect::Command, Sandboxing::Off);
        let guarded = without_sandbox(Step::Run, Effect::Command, Sandboxing::On);
        assert_eq!(
            (edit, desktop, guarded),
            (Step::Run, Step::AskDesktop, Step::Run)
        );
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        gate: Gate,
        chat: PathBuf,
        config: PathBuf,
        home: PathBuf,
    }

    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().canonicalize().unwrap();
        let root = home.join("Code");
        let chat = root.join("app");
        let config = home.join(".config").join("gnomish-relay");
        std::fs::create_dir_all(&chat).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        let allow = AllowTable::default();
        let gate = Gate {
            roots: vec![root],
            config_dir: config.clone(),
            data_dir: home.join("data"),
            allow: std::sync::Arc::new(allow),
            approvals: Approvals::new(&home.join("data"), desktop::Prompt::Off),
            sandbox: CommandSandbox::none(),
        };
        Setup {
            _tmp: tmp,
            gate,
            chat,
            config,
            home,
        }
    }

    fn check(
        s: &Setup,
        call: &Call,
        level: Permission,
        wait: std::time::Duration,
    ) -> Result<(), Refusal> {
        let cwd = s.chat.to_string_lossy();
        let job = Job {
            agent: "claude",
            cwd: &cwd,
            level,
            coverage: Coverage::Every,
            sandboxing: Sandboxing::On,
        };
        let mut turn = Turn::new(wait, wait, crate::agent::Control::default());
        s.gate.check(call, &job, &mut turn)
    }

    fn read(path: PathBuf) -> Call {
        Call::files(&[path], &[], b"read".to_vec(), "Read".into())
    }

    const SHORT: std::time::Duration = std::time::Duration::from_millis(200);

    #[test]
    fn a_read_of_the_strip_key_is_refused_at_every_level() {
        let s = setup();
        for level in LEVELS {
            let refusal = check(&s, &read(s.config.join("strip.key")), level, SHORT).unwrap_err();
            assert!(
                refusal.reason().contains("config or data folder"),
                "{refusal:?}"
            );
        }
    }

    #[test]
    fn a_read_in_the_chat_folder_runs_and_a_command_with_no_game_is_refused() {
        let s = setup();
        assert_eq!(
            check(&s, &read(s.chat.join("a.rs")), Permission::Ask, SHORT),
            Ok(())
        );
        let call = Call::command("make", &s.chat, b"make".to_vec(), "make".into());
        let refusal = check(&s, &call, Permission::AutoEdit, SHORT).unwrap_err();
        assert_eq!(refusal.reason(), NOT_FROM_THE_GAME);
        assert_eq!(check(&s, &call, Permission::FullAuto, SHORT), Ok(()));
    }

    #[test]
    fn a_desktop_request_times_out_as_refused_and_closes() {
        let s = setup();
        let call = read(s.home.join(".ssh").join("id_rsa"));
        let refusal = check(&s, &call, Permission::FullAuto, SHORT).unwrap_err();
        assert_eq!(refusal.reason(), "No answer on the desktop.");
        assert!(s.gate.approvals.list().is_empty());
    }

    fn answer_on_the_desktop(s: &Setup, verdict: desktop::Verdict) -> Result<(), Refusal> {
        let approvals = s.gate.approvals.clone();
        let answering = std::thread::spawn(move || {
            for _ in 0..200 {
                if let Some(open) = approvals.list().first() {
                    approvals.answer(&open.id, verdict).unwrap();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let call = read(s.home.join(".ssh").join("id_rsa"));
        let result = check(
            s,
            &call,
            Permission::AutoEdit,
            std::time::Duration::from_secs(10),
        );
        answering.join().unwrap();
        result
    }

    #[test]
    fn approve_on_the_desktop_runs_the_call() {
        let s = setup();
        assert_eq!(answer_on_the_desktop(&s, desktop::Verdict::Approve), Ok(()));
    }

    #[test]
    fn the_bridge_writes_desktop_requests_in_the_data_folder_that_no_agent_reaches() {
        let s = setup();
        assert_eq!(answer_on_the_desktop(&s, desktop::Verdict::Approve), Ok(()));
        let approval = s
            .gate
            .data_dir
            .join("approvals")
            .join("a1b2c3d4e5f6.answer");
        let refusal = check(&s, &read(approval), Permission::FullAuto, SHORT).unwrap_err();
        assert!(refusal.reason().contains("data folder"), "{refusal:?}");
    }

    #[test]
    fn a_desktop_request_sends_no_game_request_and_tells_the_game_how_it_ended() {
        use crate::agent::{Control, Event, Events};
        use crate::relay::{ChatId, MessageId, Session, Work};
        let s = setup();
        let run = crate::relay::Job {
            token: "tok".into(),
            chat: ChatId("c1".into()),
            id: MessageId(1),
            agent: "claude".into(),
            permission: Permission::AutoEdit,
            asked: Permission::AutoEdit,
            cwd: s.chat.to_string_lossy().into_owned(),
            session: Session::New,
            resume: None,
            text: "hi".into(),
            work: Work::Prompt,
            new_folder: false,
        };
        let (to, events) = std::sync::mpsc::channel();
        let control = Control {
            events: Events::to_bridge(to, &run),
            ..Control::default()
        };
        let job = Job {
            agent: "claude",
            cwd: &run.cwd,
            level: Permission::AutoEdit,
            coverage: Coverage::Every,
            sandboxing: Sandboxing::On,
        };
        let mut turn = Turn::new(SHORT, SHORT, control);
        let call = read(s.home.join(".ssh").join("id_rsa"));

        let refusal = s.gate.check(&call, &job, &mut turn).unwrap_err();

        assert_eq!(refusal.reason(), "No answer on the desktop.");
        let lines: Vec<String> = events
            .try_iter()
            .map(|(_, _, event)| match event {
                Event::Desktop(notice) => notice.line(),
                Event::Question(_) => "a game request".into(),
                Event::Progress(_) | Event::Raised { .. } => String::new(),
            })
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("Desktop: wait "), "{lines:?}");
        assert!(lines[0].ends_with(" command"), "{lines:?}");
        assert!(lines[1].starts_with("Desktop: none "), "{lines:?}");
    }

    #[test]
    fn a_new_message_ends_a_desktop_wait_and_tells_the_agent_why() {
        use crate::agent::{Control, StopReason};
        let s = setup();
        let control = Control::default();
        control.stop.request_for(StopReason::NewMessage);
        let cwd = s.chat.to_string_lossy();
        let job = Job {
            agent: "claude",
            cwd: &cwd,
            level: Permission::AutoEdit,
            coverage: Coverage::Every,
            sandboxing: Sandboxing::On,
        };
        let long = std::time::Duration::from_secs(30);
        let mut turn = Turn::new(long, long, control);
        let started = std::time::Instant::now();

        let refusal = s
            .gate
            .check(&read(s.home.join(".ssh").join("id_rsa")), &job, &mut turn)
            .unwrap_err();

        assert_eq!(refusal.reason(), "The player sent a new message.");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(s.gate.approvals.list().is_empty(), "the request closed");
    }

    #[test]
    fn deny_on_the_desktop_refuses_the_call() {
        let s = setup();
        let refusal = answer_on_the_desktop(&s, desktop::Verdict::Deny).unwrap_err();
        assert_eq!(refusal.reason(), "Denied on the desktop.");
    }
}
