//! The gate: one answer for each tool call of a run from the game (SPEC.md 6.6.3). The
//! classifier of `protocol` gives a verdict, the level of the job bounds it (S6, 6.6.2),
//! and the answer then comes from the game, the desktop, or nobody.

use std::path::{Path, PathBuf};

use protocol::action::{Policy, ToolCall, Verdict, classify};
use protocol::live::OptionKind;
use protocol::shell::split;

use crate::action_input::{self, resolve};
use crate::agent::Choice;
use crate::agent_wall::AgentWall;
use crate::allow::AllowTable;
use crate::always_offer::{Offer, Place, offer_for};
use crate::always_rules::{AlwaysRules, Rule, words_for};
use crate::command_sandbox::CommandSandbox;
use crate::config::{Permission, RelayConfig};
use crate::desktop::{self, Approvals, Notice, Opened, Prompt, Waiting};
use crate::dirs::Dirs;
use crate::roots::Roots;
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

/// Whether the popup of this backend can offer "Always allow" (SPEC.md 6.6.5). Only
/// where the command sandbox is the wall for what a rule allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Always {
    Offer,
    Never,
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
    pub always: Always,
}

impl Job<'_> {
    /// At `full-auto` a command runs anyway, and at `ask` every command asks.
    fn offers_always(&self) -> bool {
        self.always == Always::Offer
            && self.level == Permission::AutoEdit
            && self.coverage == Coverage::Every
            && self.sandboxing == Sandboxing::On
    }
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
    pub roots: Roots,
    /// The config folder of the bridge. Every path in it is `deny`.
    pub config_dir: PathBuf,
    /// The data folder of the bridge: its state, locks, log, and desktop requests. Every
    /// path in it is `deny`, and the bridge writes them itself, never through this gate.
    pub data_dir: PathBuf,
    pub allow: std::sync::Arc<AllowTable>,
    pub approvals: Approvals,
    pub sandbox: CommandSandbox,
    /// The wall of each agent process (SPEC.md 6.6.4).
    pub wall: AgentWall,
    /// The "Always allow" rules from the game, in the data folder.
    pub always: AlwaysRules,
    /// A rule of the home folder covers only that folder.
    pub home: PathBuf,
}

/// Where the gate of the bridge finds its files.
pub struct Places<'a> {
    /// Holds `config.toml`.
    pub config_dir: &'a Path,
    /// Holds the desktop requests and `rules.json`.
    pub data_dir: &'a Path,
    pub home: &'a Path,
}

impl<'a> Places<'a> {
    pub fn of(dirs: &'a Dirs) -> Places<'a> {
        Places {
            config_dir: &dirs.config,
            data_dir: &dirs.data,
            home: &dirs.home,
        }
    }
}

impl Gate {
    pub fn new(config: &RelayConfig, places: &Places, prompt: Prompt) -> Gate {
        let roots = config
            .policy
            .folders
            .roots
            .iter()
            .map(|r| PathBuf::from(String::from_utf8_lossy(r).into_owned()))
            .collect();
        Gate {
            roots: Roots::new(roots),
            config_dir: places.config_dir.to_owned(),
            data_dir: places.data_dir.to_owned(),
            allow: std::sync::Arc::new(config.allow.clone()),
            approvals: Approvals::new(places.data_dir, prompt).with_wait(config.permission_timeout),
            sandbox: CommandSandbox::detect(config.hosts.clone(), &config.local_ports),
            wall: AgentWall::detect(places.data_dir, config.agent_network, &config.local_ports),
            always: AlwaysRules::new(places.data_dir),
            home: places.home.to_owned(),
        }
    }

    /// No allow table, no rules, no sandbox, no wall, and no desktop prompt. Tests set
    /// the fields that they need on top.
    pub fn bare(roots: Vec<PathBuf>, config_dir: PathBuf, data_dir: PathBuf) -> Gate {
        Gate {
            roots: Roots::new(roots),
            config_dir,
            approvals: Approvals::new(&data_dir, Prompt::Off),
            data_dir,
            allow: std::sync::Arc::default(),
            sandbox: CommandSandbox::none(),
            wall: AgentWall::none(),
            always: AlwaysRules::none(),
            home: std::env::temp_dir(),
        }
    }

    fn policy(&self, chat: &Path) -> Policy {
        let bridge = [&self.config_dir, &self.data_dir];
        let deny: Vec<PathBuf> = bridge
            .into_iter()
            .chain(&self.sandbox.game)
            .map(|d| resolve(d).unwrap_or_else(|| d.clone()))
            .collect();
        let rules = self.allow.rules_for(chat);
        action_input::policy(&self.roots.list(), chat, &deny, &rules)
    }

    /// `Ok` when the call runs. A question waits in the game or on the desktop.
    pub fn check(&self, call: &Call, job: &Job, turn: &mut Turn) -> Result<(), Refusal> {
        let chat = resolve(Path::new(job.cwd)).unwrap_or_else(|| PathBuf::from(job.cwd));
        let now = crate::run::now();
        let rules = self.always.list(now);
        let policy = self.policy(&chat);
        let verdict = classify(&call.tool, &policy, &words_for(&rules, &chat));
        let step = decide(job.level, verdict, call.effect, job.coverage);
        match without_sandbox(step, call.effect, job.sandboxing) {
            Step::Run => {
                self.note_use(call, &policy, &chat, &rules, now);
                Ok(())
            }
            Step::Refuse => Err(Refusal::by_rule(
                "It touches the settings or data folder of Gnomish Relay, which agents can't reach.",
            )),
            Step::AskGame => {
                let asking = Asking {
                    call,
                    job,
                    policy: &policy,
                    chat: &chat,
                };
                self.ask_game(&asking, &rules, turn)
            }
            Step::AskDesktop => self.ask_desktop(call, job, turn),
        }
    }

    /// A rule that let a command run counts as used today, so it does not expire.
    fn note_use(&self, call: &Call, policy: &Policy, chat: &Path, rules: &[Rule], now: u32) {
        let ToolCall::Command { raw, .. } = &call.tool else {
            return;
        };
        if rules.is_empty() || classify(&call.tool, policy, &[]) == Verdict::Allow {
            return;
        }
        let Some(script) = split(raw) else {
            return;
        };
        let commands: Vec<Vec<Vec<u8>>> = script.simples.into_iter().map(|s| s.words).collect();
        self.always.mark_used(chat, &commands, now);
    }

    fn offer(&self, asking: &Asking, rules: &[Rule]) -> Option<Offer> {
        if !asking.job.offers_always() || !self.always.is_on() {
            return None;
        }
        let roots = self.roots.list();
        let place = Place {
            chat: asking.chat,
            roots: &roots,
            home: &self.home,
        };
        offer_for(&asking.call.tool, asking.policy, rules, &place)
    }

    /// True once a rule that another popup added covers the call (SPEC.md 6.6.5).
    fn now_covered(&self, asking: &Asking) -> bool {
        if !asking.job.offers_always() {
            return false;
        }
        let rules = self.always.list(crate::run::now());
        let game = words_for(&rules, asking.chat);
        !game.is_empty() && classify(&asking.call.tool, asking.policy, &game) == Verdict::Allow
    }

    fn grant(&self, offer: &Offer) {
        let now = crate::run::now();
        if let Err(e) = self
            .always
            .grant(&offer.folder, offer.scope, &offer.rules, now)
        {
            crate::run::log(&format!("rule not added: {e:#}"));
            return;
        }
        crate::run::log(&format!("rule added: {}", offer.line));
        self.approvals.notice(&format!(
            "Always allowed now: {}. To remove it, use Settings in the game or run gnomish-relay rules.",
            offer.line
        ));
    }

    fn ask_game(&self, asking: &Asking, rules: &[Rule], turn: &mut Turn) -> Result<(), Refusal> {
        if !turn.listening() {
            return Err(Refusal::by_rule(NOT_FROM_THE_GAME));
        }
        let offer = self.offer(asking, rules);
        let choices = game_choices(offer.as_ref().map(|o| o.line.clone()));
        let covered = || self.now_covered(asking);
        match turn.ask_game(asking.call.text.clone(), choices, &covered) {
            Answer::Game(0) | Answer::Covered => Ok(()),
            Answer::Game(1) if offer.is_some() => {
                if let Some(offer) = &offer {
                    self.grant(offer);
                }
                Ok(())
            }
            Answer::Game(_) => Err(Refusal::ByUser),
            Answer::NewMessage => Err(Refusal::by_rule(NEW_MESSAGE)),
            Answer::Desktop(_) | Answer::None => Err(Refusal::by_rule("No answer from the game.")),
        }
    }

    /// The game shows a notice with no buttons: only the desktop answers.
    fn ask_desktop(&self, call: &Call, job: &Job, turn: &mut Turn) -> Result<(), Refusal> {
        let text = String::from_utf8_lossy(&call.text).into_owned();
        let opened = self
            .approvals
            .open(job.agent, job.cwd, &text, crate::run::now())
            .map_err(|e| Refusal::by_rule(&format!("Couldn't ask on your desktop: {e:#}")))?;
        let answer = wait_on_the_desktop(&self.approvals, &opened, None, turn);
        match answer {
            Answer::Desktop(true) => Ok(()),
            Answer::Desktop(false) => Err(Refusal::by_rule("Denied on your desktop.")),
            Answer::NewMessage => Err(Refusal::by_rule(NEW_MESSAGE)),
            Answer::Game(_) | Answer::None | Answer::Covered => {
                Err(Refusal::by_rule("No answer on your desktop."))
            }
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
        Answer::Game(_) | Answer::None | Answer::NewMessage | Answer::Covered => Waiting::NoAnswer,
    };
    turn.desktop(notice.ended(waiting));
    answer
}

/// A game question and what the gate knows about it.
struct Asking<'a> {
    call: &'a Call,
    job: &'a Job<'a>,
    policy: &'a Policy,
    /// Resolved.
    chat: &'a Path,
}

/// Allow, "Always allow" with its rule line when the popup offers it, and Deny. The
/// bridge made the line, never the agent (SPEC.md 6.6.5).
pub fn game_choices(always: Option<String>) -> Vec<Choice> {
    let allow = Choice {
        kind: OptionKind::AllowOnce,
        label: "Allow".into(),
    };
    let always = always.map(|line| Choice {
        kind: OptionKind::AllowAlways,
        label: line,
    });
    let deny = Choice {
        kind: OptionKind::RejectOnce,
        label: "Deny".into(),
    };
    std::iter::once(allow).chain(always).chain([deny]).collect()
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
        let gate = Gate {
            always: AlwaysRules::new(&home.join("data")),
            home: home.clone(),
            ..Gate::bare(vec![root], config.clone(), home.join("data"))
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
            always: Always::Offer,
        };
        let mut turn = Turn::new(wait, wait, crate::agent::Control::default());
        s.gate.check(call, &job, &mut turn)
    }

    fn read(path: PathBuf) -> Call {
        Call::files(&[path], &[], b"read".to_vec(), "Read".into())
    }

    const SHORT: std::time::Duration = std::time::Duration::from_millis(200);

    #[test]
    fn a_read_of_the_key_file_of_the_addon_is_refused_at_every_level() {
        let mut s = setup();
        let key = s.home.join("WoW/Interface/AddOns/GnomishRelay/Key.lua");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        std::fs::write(&key, "key").unwrap();
        s.gate.sandbox = CommandSandbox::none().with_game(vec![key.clone()]);

        for level in LEVELS {
            let refusal = check(&s, &read(key.clone()), level, SHORT).unwrap_err();

            assert!(
                refusal.reason().contains("settings or data folder"),
                "{refusal:?}"
            );
        }
    }

    #[test]
    fn a_read_of_the_strip_key_is_refused_at_every_level() {
        let s = setup();
        for level in LEVELS {
            let refusal = check(&s, &read(s.config.join("strip.key")), level, SHORT).unwrap_err();
            assert!(
                refusal.reason().contains("settings or data folder"),
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
        assert_eq!(refusal.reason(), "No answer on your desktop.");
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
            chat: ChatId::new("c1"),
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
            always: Always::Offer,
        };
        let mut turn = Turn::new(SHORT, SHORT, control);
        let call = read(s.home.join(".ssh").join("id_rsa"));

        let refusal = s.gate.check(&call, &job, &mut turn).unwrap_err();

        assert_eq!(refusal.reason(), "No answer on your desktop.");
        let lines: Vec<String> = events
            .try_iter()
            .map(|(_, _, event)| match event {
                Event::Desktop(notice) => notice.line(),
                Event::Question(_) => "a game request".into(),
                Event::Progress(_)
                | Event::Raised { .. }
                | Event::Withdrawn
                | Event::CommandOutput(_) => String::new(),
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
            always: Always::Offer,
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

    /// What the game saw and answered for one check with the game listening.
    struct Asked {
        result: Result<(), Refusal>,
        /// The labels of the choices of each question.
        questions: Vec<Vec<String>>,
        withdrawn: bool,
    }

    fn job_for(cwd: &str, level: Permission, always: Always, sandboxing: Sandboxing) -> Job<'_> {
        Job {
            agent: "claude",
            cwd,
            level,
            coverage: Coverage::Every,
            sandboxing,
            always,
        }
    }

    /// Answers each question with `choice`, or never with `None`.
    fn ask_with_game(s: &Setup, call: &Call, job: &Job, choice: Option<usize>) -> Asked {
        use crate::agent::{Control, Event, Events};
        use crate::relay::{ChatId, MessageId, Session, Work};
        let run = crate::relay::Job {
            token: "tok".into(),
            chat: ChatId::new("c1"),
            id: MessageId(1),
            agent: "claude".into(),
            permission: job.level,
            asked: job.level,
            cwd: job.cwd.into(),
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
        let seen = std::thread::spawn(move || {
            let mut questions = Vec::new();
            let mut withdrawn = false;
            let mut open = Vec::new();
            while let Ok((_, _, event)) = events.recv() {
                match event {
                    Event::Question(q) => {
                        questions.push(q.choices.iter().map(|c| c.label.clone()).collect());
                        match choice {
                            Some(c) => q.answer.send(Some(c)).unwrap(),
                            None => open.push(q.answer),
                        }
                    }
                    Event::Withdrawn => withdrawn = true,
                    _ => {}
                }
            }
            (questions, withdrawn)
        });
        let wait = std::time::Duration::from_secs(10);
        let mut turn = Turn::new(wait, wait, control);
        let result = s.gate.check(call, job, &mut turn);
        drop(turn);
        let (questions, withdrawn) = seen.join().unwrap();
        Asked {
            result,
            questions,
            withdrawn,
        }
    }

    fn command(s: &Setup, raw: &str) -> Call {
        Call::command(raw, &s.chat, raw.as_bytes().to_vec(), "Bash".into())
    }

    const ALWAYS: usize = 1;

    #[test]
    fn a_command_at_auto_edit_offers_always_with_the_rule_line() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        let asked = ask_with_game(&s, &command(&s, "make test"), &job, Some(0));
        assert_eq!(asked.result, Ok(()));
        assert_eq!(
            asked.questions,
            vec![vec![
                "Allow".to_owned(),
                "make * in Code/app".to_owned(),
                "Deny".to_owned()
            ]]
        );
        assert!(
            s.gate.always.list(crate::run::now()).is_empty(),
            "allow once adds no rule"
        );
    }

    #[test]
    fn always_adds_the_rule_and_the_next_same_command_runs_with_no_question() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        let first = ask_with_game(&s, &command(&s, "cargo test -p x"), &job, Some(ALWAYS));
        assert_eq!(first.result, Ok(()));
        let rules = s.gate.always.list(crate::run::now());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern(), "cargo test *");
        assert_eq!(rules[0].folder, s.chat);

        let next = ask_with_game(&s, &command(&s, "cargo test -q"), &job, Some(2));

        assert_eq!(next.result, Ok(()));
        assert!(next.questions.is_empty(), "{:?}", next.questions);
        let other = ask_with_game(&s, &command(&s, "cargo build"), &job, Some(2));
        assert_eq!(
            other.result,
            Err(Refusal::ByUser),
            "a rule covers only its words"
        );
    }

    #[test]
    fn with_no_always_choice_the_second_button_is_deny() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Never, Sandboxing::On);
        let asked = ask_with_game(&s, &command(&s, "make"), &job, Some(1));
        assert_eq!(asked.result, Err(Refusal::ByUser));
        assert_eq!(
            asked.questions,
            vec![vec!["Allow".to_owned(), "Deny".to_owned()]]
        );
        assert!(s.gate.always.list(crate::run::now()).is_empty());
    }

    #[test]
    fn at_ask_with_no_sandbox_or_for_codex_the_popup_has_no_always() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let cases = [
            job_for(&cwd, Permission::Ask, Always::Offer, Sandboxing::On),
            job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::Off),
            job_for(&cwd, Permission::AutoEdit, Always::Never, Sandboxing::On),
        ];
        for (i, job) in cases.iter().enumerate() {
            let asked = ask_with_game(&s, &command(&s, "make"), job, Some(0));
            assert_eq!(
                asked.questions,
                vec![vec!["Allow".to_owned(), "Deny".to_owned()]],
                "case {i}"
            );
        }
    }

    #[test]
    fn a_rule_does_not_apply_at_the_level_ask() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let auto = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        ask_with_game(&s, &command(&s, "make"), &auto, Some(ALWAYS));
        let ask = job_for(&cwd, Permission::Ask, Always::Offer, Sandboxing::On);
        let asked = ask_with_game(&s, &command(&s, "make"), &ask, Some(0));
        assert_eq!(asked.questions.len(), 1, "every command asks at ask");
    }

    #[test]
    fn a_push_or_a_recursive_rm_gets_no_always() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        for raw in ["git push", "rm -rf target", "npx x"] {
            let asked = ask_with_game(&s, &command(&s, raw), &job, Some(0));
            assert_eq!(
                asked.questions,
                vec![vec!["Allow".to_owned(), "Deny".to_owned()]],
                "{raw}"
            );
        }
    }

    #[test]
    fn an_open_question_ends_when_another_popup_adds_a_rule_that_covers_it() {
        let s = setup();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        let always = s.gate.always.clone();
        let chat = s.chat.clone();
        let granting = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let rule = [vec!["make".to_owned()]];
            always
                .grant(
                    &chat,
                    crate::always_rules::Scope::Tree,
                    &rule,
                    crate::run::now(),
                )
                .unwrap();
        });

        let asked = ask_with_game(&s, &command(&s, "make"), &job, None);

        granting.join().unwrap();
        assert_eq!(asked.result, Ok(()));
        assert!(asked.withdrawn, "the popup goes");
    }

    #[test]
    fn a_rule_that_lets_a_command_run_counts_as_used_today() {
        let s = setup();
        let now = crate::run::now();
        let mut rule = crate::always_rules::Rule {
            id: "a1b2".into(),
            folder: s.chat.clone(),
            scope: crate::always_rules::Scope::Tree,
            words: vec!["make".into()],
            added: now,
            used_day: crate::always_rules::day_of(now) - 5,
        };
        crate::always_rules::save(&s.home.join("data"), std::slice::from_ref(&rule)).unwrap();
        let cwd = s.chat.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);

        let asked = ask_with_game(&s, &command(&s, "make all"), &job, None);

        assert_eq!(asked.result, Ok(()));
        rule.used_day = crate::always_rules::day_of(now);
        assert_eq!(s.gate.always.list(now), vec![rule]);
    }

    #[test]
    fn a_rule_in_an_allowed_root_covers_only_that_folder() {
        let s = setup();
        let root = s.chat.parent().unwrap().to_owned();
        let cwd = root.to_string_lossy().into_owned();
        let job = job_for(&cwd, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        let call = Call::command("make", &root, b"make".to_vec(), "Bash".into());
        ask_with_game(&s, &call, &job, Some(ALWAYS));
        let rules = s.gate.always.list(crate::run::now());
        assert_eq!(rules[0].scope, crate::always_rules::Scope::Exact);
        let app = s.chat.to_string_lossy().into_owned();
        let job = job_for(&app, Permission::AutoEdit, Always::Offer, Sandboxing::On);
        let asked = ask_with_game(&s, &command(&s, "make"), &job, Some(0));
        assert_eq!(
            asked.questions.len(),
            1,
            "the chat inside the root still asks"
        );
    }

    #[test]
    fn deny_on_the_desktop_refuses_the_call() {
        let s = setup();
        let refusal = answer_on_the_desktop(&s, desktop::Verdict::Deny).unwrap_err();
        assert_eq!(refusal.reason(), "Denied on your desktop.");
    }
}
