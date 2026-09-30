//! The main loop: screenshots in, agent runs, slots out (SPEC.md 8.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::agent::{
    Agent, Agents, Control, Event, Events, Run, SessionInfo, StopReason, StopSignal,
};
use crate::chat_branch;
use crate::config::{Permission, Policy};
use crate::git_actions::{self, Context, Done, Effect, GitAction, MergeDesk};
use crate::git_blocks::RunBlocks;
use crate::git_host::GitHost;
use crate::raise::{RaiseGuard, Raised, Raiser};
use crate::receive::{KeySet, Rejected, frame_tag, receive, receive_for};
use crate::relay::BranchPlan;
use crate::run_git::{RunGit, WorktreeChange};
use crate::turn::STOPPED;
use protocol::apps::App;
use protocol::record::Record;
use protocol::version::version_fit;

use crate::action_input::resolve;
use crate::folder_walk::{self, Snapshot, Walk};
use crate::line_choice::{LineChoice, LineFile, with_line};
use crate::new_folder::{make_folder, real_chat_folder};
use crate::relay::{BAD_AGENT, ChatId, FrameTag, Job, MessageId, Outcome, Relay, Work};
use crate::saved;
use crate::screenshots::{Watcher, read_strip};
use crate::settings_list::BridgeSettings;
use crate::slots::{self, Files};
use crate::spool::{open_spool, spool_dir, take_files};
use crate::state;
use crate::status;
use crate::story::{Story, StorySpec};
use crate::terminal_sessions::TerminalSessions;
use crate::timeways::{NO_STORY, Timeways};
use crate::vectors::is_test_strip;
use crate::versions::update_text;

const TICK: Duration = Duration::from_millis(250);
/// The addon calls the bridge offline when the body it loads is older than 150 s.
const HEARTBEAT: Duration = Duration::from_mins(1);
/// A flood of spool files then costs at most one live file for each 3 seconds (SPEC.md 10.2).
const NOTICE_GAP: Duration = Duration::from_secs(3);
/// The folder of the Timeways state, inside the data folder (SPEC.md 9.7, decision 4).
pub const TIMEWAYS_DIR: &str = "timeways";

type Found = Result<Vec<(String, SessionInfo)>, String>;

enum Finished {
    Run(Job, Run, Box<RunEnd>),
    List(Job, Found),
    Folders(Job, Snapshot),
    Git(Job, Done),
}

/// What git adds to the end of a run (SPEC.md 9.10).
#[derive(Default)]
struct RunEnd {
    change: WorktreeChange,
    blocks: RunBlocks,
}

const NO_GIT: &str =
    "Git isn't available to the desktop app. Install git, then run gnomish-relay restart.";
type RunEvent = (ChatId, MessageId, Event);

pub struct Paths {
    pub addons: PathBuf,
    pub screenshots: PathBuf,
    /// `WTF/Account`, which holds the saved variables of each account.
    pub accounts: PathBuf,
    /// The data folder of the bridge, for `state.json`.
    pub state: PathBuf,
    /// The config folder of the bridge, with the keys. The folder list never shows it.
    pub config: PathBuf,
}

#[allow(clippy::cast_possible_truncation)] // u32 seconds last until 2106
pub fn now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

/// Control characters in a log line come out escaped, so a prompt cannot fake a
/// log line (SPEC.md 6.2, rule 15).
pub fn log(line: &str) {
    eprintln!("{} {}", now(), line.escape_debug());
}

/// The bridge between the Screenshots folder, the agents, and the slots. `run`
/// calls `step` four times a second. Tests call it directly.
pub struct Bridge {
    addons: PathBuf,
    /// The data folder, for the time of the last strip.
    data: PathBuf,
    /// The strip line that each publish sends to the addons (SPEC.md 7.1.3).
    line: LineFile,
    keys: KeySet,
    watcher: Watcher,
    /// Only with the relay part in the config (SPEC.md 9.7, decision 15).
    relay: Option<RelayLane>,
    /// Only with a Timeways key. It holds no agents.
    timeways: Option<TimewaysLane>,
}

/// What one app keeps on disk, and when it writes it (SPEC.md 9.7, decision 4).
struct LaneFiles {
    state: PathBuf,
    saved: saved::Watcher,
    changed: bool,
    stored: bool,
    last_publish: Instant,
}

impl LaneFiles {
    fn new(state: PathBuf, accounts: &Path, app: App) -> LaneFiles {
        LaneFiles {
            state,
            saved: saved::Watcher::new(accounts, app),
            changed: true,
            stored: false,
            last_publish: Instant::now(),
        }
    }

    fn publish_due(&self) -> bool {
        self.changed || self.last_publish.elapsed() >= HEARTBEAT
    }
}

/// The relay app: its lane, its files, and the agents that its messages start. Only
/// this lane holds agents.
struct RelayLane {
    relay: Relay,
    files: LaneFiles,
    agents: Agents,
    /// Where the folder list looks, and where a new folder may go.
    walk: Walk,
    /// The stop signal of each run in progress, by chat.
    stops: BTreeMap<ChatId, StopSignal>,
    events: Sender<RunEvent>,
    run_events: Receiver<RunEvent>,
    /// Where the answer to each open permission request goes, by request id.
    answers: BTreeMap<String, Sender<Option<usize>>>,
    finished: Sender<Finished>,
    results: Receiver<Finished>,
    /// With no raiser, a chat never raises the level of the config (SPEC.md 9.3).
    raiser: Option<Raiser>,
    raises: RaiseGuard,
    /// The answer to a settings list, with the levels of the relay.
    settings: BridgeSettings,
    /// Strips with a bad tag since the last good relay strip. The game shows a key
    /// mismatch only through this count.
    bad_tags: u32,
    /// The terminal sessions of the hooks, and the spool folder that brings their events.
    terminal: TerminalSessions,
    spool: PathBuf,
    notices_changed: bool,
    /// With no git on this computer, runs have no own branch (SPEC.md 9.10).
    git: Option<RunGit>,
}

/// The Timeways app: its lane and its files. Its messages go to the story program, and
/// never to an agent.
struct TimewaysLane {
    timeways: Timeways,
    files: LaneFiles,
    /// Only with a `[story]` section in the config.
    story: Option<Story>,
}

impl Bridge {
    /// With no Timeways key there is no Timeways lane, and the bridge works as before.
    pub fn new(paths: Paths, policy: Policy, keys: KeySet, agents: Agents) -> Result<Bridge> {
        let relay = RelayLane::open(&paths, policy, agents)?;
        Bridge::with_lanes(paths, keys, Some(relay))
    }

    /// A player with only Timeways: the bridge serves the Timeways lane alone.
    pub fn without_relay(paths: Paths, keys: KeySet) -> Result<Bridge> {
        Bridge::with_lanes(paths, keys, None)
    }

    fn with_lanes(paths: Paths, keys: KeySet, relay: Option<RelayLane>) -> Result<Bridge> {
        let timeways = if keys.has_timeways() {
            Some(TimewaysLane::open(&paths)?)
        } else {
            None
        };
        Ok(Bridge {
            watcher: Watcher::new(&paths.screenshots),
            timeways,
            relay,
            addons: paths.addons,
            line: LineFile::new(&paths.state),
            data: paths.state,
            keys,
        })
    }

    /// A chat that asks for more than the config allows then gets one desktop dialog.
    #[must_use]
    pub fn with_raises(mut self, raiser: Raiser) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.raiser = Some(raiser);
        }
        self
    }

    /// The values that the Settings and Diag tabs of the game show.
    #[must_use]
    pub fn with_settings(mut self, settings: BridgeSettings) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.settings = settings;
        }
        self
    }

    /// Git in the chats with another host, for example one that skips the config of the
    /// user in a test.
    #[must_use]
    pub fn with_git(mut self, host: GitHost) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.git = Some(RunGit {
                host: Arc::new(host),
                walk: relay.walk.clone(),
            });
        }
        self
    }

    /// The story program runs only in the Timeways lane, so with no Timeways key it
    /// never starts.
    #[must_use]
    pub fn with_story(mut self, spec: StorySpec) -> Bridge {
        match &mut self.timeways {
            Some(timeways) => timeways.story = Some(Story::new(spec)),
            None => log("timeways: no timeways.key, so the story program does not start"),
        }
        self
    }

    pub fn step(&mut self) {
        self.take_screenshots();
        if let Some(relay) = &mut self.relay {
            relay.step(&self.keys, &self.addons, &mut self.line);
        }
        if let Some(timeways) = &mut self.timeways {
            timeways.step(&self.keys, &self.addons, &mut self.line);
        }
    }

    fn take_screenshots(&mut self) {
        for path in self.watcher.ready() {
            let keys = &self.keys;
            let tag_checks =
                |bytes: &[u8]| receive(bytes, keys, now()).is_ok() || is_test_strip(bytes);
            let bytes = match read_strip(&path, tag_checks) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    log(&format!("skipped {}: {e}", path.display()));
                    continue;
                }
            };
            if is_test_strip(&bytes) {
                log(&format!(
                    "left {} in place: it's from the self-test",
                    path.display()
                ));
                continue;
            }
            let outcome = self.take_strip(&bytes);
            if let Some(why) = kept_reason(&outcome) {
                log(&format!("left {} in place: {why}", path.display()));
                continue;
            }
            // A normal screenshot never decodes as a frame, so this is a strip, and its
            // pixels hold a prompt (SPEC.md 6.2, rule 8).
            if let Err(e) = std::fs::remove_file(&path) {
                log(&format!("cannot delete {}: {e}", path.display()));
                continue;
            }
            if let Some(line) = deleted_line(&outcome) {
                log(line);
            }
        }
    }

    fn take_strip(&mut self, bytes: &[u8]) -> StripOutcome {
        let (app, records) = match receive(bytes, &self.keys, now()) {
            Ok(routed) => routed,
            Err(reason) => {
                log(&format!(
                    "skipped a message from WoW: {}",
                    rejected_text(&reason)
                ));
                self.count_bad_tag(&reason);
                return StripOutcome::Rejected(reason);
            }
        };
        let Some(tag) = frame_tag(bytes) else {
            return StripOutcome::Rejected(Rejected::NotAFrame);
        };
        match (app, &mut self.relay, &mut self.timeways) {
            (App::Relay, Some(relay), _) => {
                relay.bad_tags = 0;
                relay.take_records(tag, &records, "strip");
            }
            (App::Timeways, _, Some(timeways)) => timeways.take_records(&records, "strip"),
            // `KeySet` routes to Timeways only with a Timeways key, and that key makes the lane.
            (App::Relay, None, _) | (App::Timeways, _, None) => {
                log(&format!(
                    "got a message for {app:?}, which is off in config.toml"
                ));
                return StripOutcome::AppOff;
            }
        }
        if let Err(e) = status::mark_strip(&self.data, now()) {
            log(&format!("cannot write the time of the last strip: {e:#}"));
        }
        StripOutcome::Taken
    }
}

enum StripOutcome {
    Taken,
    Rejected(Rejected),
    AppOff,
}

/// `None` for a strip that goes. A strip that is old, early, or signed with another key
/// never becomes valid, so it goes too.
fn kept_reason(outcome: &StripOutcome) -> Option<&'static str> {
    match outcome {
        StripOutcome::Taken
        | StripOutcome::Rejected(Rejected::Stale | Rejected::Future | Rejected::BadTag) => None,
        StripOutcome::Rejected(_) => Some("it holds a damaged message"),
        StripOutcome::AppOff => Some("its app is off in config.toml"),
    }
}

/// Why a message of the game did not count, in the words of the player.
fn rejected_text(reason: &Rejected) -> &'static str {
    match reason {
        Rejected::NotAFrame => "the screenshot holds no readable message",
        Rejected::BadTag => "your game and the desktop app don't match",
        Rejected::Ambiguous => "both keys match it",
        Rejected::OtherApp => "it came from the wrong addon",
        Rejected::Stale => "it's from before the desktop app started",
        Rejected::Future => "its time is in the future",
        Rejected::BadRecords => "its contents are damaged",
    }
}

fn deleted_line(outcome: &StripOutcome) -> Option<&'static str> {
    match outcome {
        StripOutcome::Rejected(Rejected::Stale) => {
            Some("deleted the screenshot of that old message")
        }
        StripOutcome::Rejected(Rejected::Future) => {
            Some("deleted its screenshot. Check the clock of this computer")
        }
        StripOutcome::Rejected(Rejected::BadTag) => {
            Some("deleted its screenshot. Run gnomish-relay setup, then type /reload in WoW")
        }
        _ => None,
    }
}

impl Bridge {
    /// No key checks a bad tag, so the app is unknown. Only the relay has a way to show it.
    fn count_bad_tag(&mut self, reason: &Rejected) {
        if let (Rejected::BadTag, Some(relay)) = (reason, &mut self.relay) {
            relay.bad_tags += 1;
            relay.files.changed = true;
        }
    }
}

impl RelayLane {
    fn open(paths: &Paths, policy: Policy, agents: Agents) -> Result<RelayLane> {
        let walk = repo_walk(&policy, paths);
        let relay = match state::load(&paths.state)? {
            Some(saved) => Relay::from_state(policy, saved),
            None => Relay::new(policy),
        };
        let (finished, results) = channel();
        let (events, run_events) = channel();
        let spool = spool_dir(&paths.state);
        if let Err(e) = open_spool(&spool) {
            log(&format!("no notifications from terminal sessions: {e:#}"));
        }
        let (terminal, problem) = TerminalSessions::load(&paths.state, now());
        if let Some(problem) = problem {
            log(&problem);
        }
        let git = match GitHost::new() {
            Ok(host) => Some(RunGit {
                host: Arc::new(host),
                walk: walk.clone(),
            }),
            Err(e) => {
                log(&format!("no git in chats: {e:#}"));
                None
            }
        };
        Ok(RelayLane {
            git,
            relay,
            files: LaneFiles::new(paths.state.clone(), &paths.accounts, App::Relay),
            agents,
            walk,
            stops: BTreeMap::new(),
            events,
            run_events,
            answers: BTreeMap::new(),
            finished,
            results,
            raiser: None,
            raises: RaiseGuard::default(),
            settings: BridgeSettings::default(),
            bad_tags: 0,
            terminal,
            spool,
            notices_changed: false,
        })
    }

    fn step(&mut self, keys: &KeySet, addons: &Path, line: &mut LineFile) {
        self.take_saved_variables(keys);
        self.take_notices();
        self.signal_stops();
        self.remove_rules();
        self.take_events();
        self.pass_answers();
        self.finish_runs();
        self.clean_up_worktrees();
        if self.files.publish_due() || self.notices_due() {
            self.store();
            self.publish(addons, line.choice());
            self.files.last_publish = Instant::now();
        }
        // A run starts only when its message is marked as seen on disk, so a crash
        // cannot run it twice.
        if self.files.stored {
            self.start_runs();
        }
    }

    /// The state after `start_runs` needs no write: a running job is a working record,
    /// and a restart ends it as an error.
    fn store(&mut self) {
        let result = state::save(&self.files.state, &self.relay.to_state());
        if let Err(e) = &result {
            log(&format!("cannot save the state: {e:#}"));
        }
        self.files.stored = result.is_ok();
    }

    fn notices_due(&self) -> bool {
        self.notices_changed && self.files.last_publish.elapsed() >= NOTICE_GAP
    }

    /// The events of the terminal sessions, with the time of this read (SPEC.md 10.2).
    fn take_notices(&mut self) {
        let taken = take_files(&self.spool, SystemTime::now());
        for refused in &taken.refused {
            log(&format!("spool file dropped: {refused}"));
        }
        let mut changed = self.terminal.expire(now());
        for file in &taken.files {
            self.terminal.apply(file, now());
            changed = true;
        }
        if !changed {
            return;
        }
        self.notices_changed = true;
        if let Err(e) = self.terminal.save() {
            log(&format!("cannot save the terminal sessions: {e:#}"));
        }
    }

    /// A changed file means a `/reload`: the outbox frames get the same checks as a strip.
    fn take_saved_variables(&mut self, keys: &KeySet) {
        for text in self.files.saved.changed() {
            self.relay.reset_window();
            self.files.changed = true;
            for (tag, records) in outbox_records(App::Relay, &text, keys) {
                self.take_records(tag, &records, "outbox");
            }
        }
    }

    fn take_records(&mut self, tag: FrameTag, records: &[Record], source: &str) {
        let build = self.relay.client_build().map(str::to_owned);
        let version = self.relay.addon_version();
        let outcomes = self.relay.on_tagged_frame(tag, records, now());
        if let Some(new) = self
            .relay
            .client_build()
            .filter(|b| Some(*b) != build.as_deref())
        {
            log(&format!(
                "game build {new}: screenshots and addon files work"
            ));
        }
        if let Some(new) = self.relay.addon_version().filter(|v| Some(*v) != version) {
            log_version(App::Relay, new);
        }
        let accepted = outcomes.iter().filter(|o| **o == Outcome::Accepted).count();
        log(&format!(
            "{source}: {} records, {accepted} new",
            records.len()
        ));
        self.files.changed = true;
    }

    fn start_runs(&mut self) {
        while let Some(job) = self.relay.next_job() {
            match job.work {
                Work::ListSessions => self.start_list(job),
                Work::ListFolders => self.start_folder_list(job),
                Work::ListSettings => {
                    let rules = self.settings.rules.lines(now());
                    let hooks = self.settings.hook_lines();
                    self.relay
                        .finish_settings(&job, &self.settings, &rules, &hooks);
                    self.files.changed = true;
                }
                Work::Prompt | Work::Attach { .. } => self.start_run(job),
                Work::Git(ref action) => {
                    let action = action.clone();
                    self.start_git(job, action);
                }
            }
        }
    }

    /// The bridge runs a git action itself, in a thread: a merge waits for the desktop.
    fn start_git(&mut self, job: Job, action: GitAction) {
        log(&format!("git {action:?} {} #{}", job.chat, job.id.0));
        let finished = self.finished.clone();
        let Some(git) = self.git.clone() else {
            let done = Done {
                reply: Err(NO_GIT.into()),
                effect: Effect::Nothing,
            };
            let _ = finished.send(Finished::Git(job, done));
            return;
        };
        let control = Control {
            stop: StopSignal::default(),
            events: Events::to_bridge(self.events.clone(), &job),
        };
        self.stops.insert(job.chat.clone(), control.stop.clone());
        let worktree = self.relay.worktree_of(&job.chat).cloned();
        let desk = self.raiser.as_ref().map(|r| MergeDesk {
            approvals: r.approvals.clone(),
            wait: r.permission_timeout,
        });
        thread::spawn(move || {
            let context = Context {
                git: &git.host,
                worktree: worktree.as_ref(),
                desk: desk.as_ref(),
                control: &control,
            };
            let done = git_actions::perform(&action, &context);
            let _ = finished.send(Finished::Git(job, done));
        });
    }

    /// A deleted chat takes its worktree along, but never work that exists nowhere else.
    fn clean_up_worktrees(&mut self) {
        let cleanups = self.relay.take_cleanups();
        let Some(git) = &self.git else {
            return;
        };
        if cleanups.is_empty() {
            return;
        }
        let host = Arc::clone(&git.host);
        thread::spawn(move || {
            for worktree in &cleanups {
                for line in chat_branch::remove_after_delete(&host, worktree) {
                    log(&format!("deleted chat {}: {line}", worktree.chat));
                }
            }
        });
    }

    fn start_run(&mut self, job: Job) {
        log(&format!(
            "run {} #{} with {} at {:?}",
            job.chat, job.id.0, job.agent, job.permission
        ));
        let finished = self.finished.clone();
        // The policy refuses an agent that the config does not have, so this is a guard.
        let Some(agent) = self.agents.get(&job.agent).map(Arc::clone) else {
            let run = Run {
                reply: Err(BAD_AGENT.into()),
                session: None,
            };
            let _ = finished.send(Finished::Run(job, run, Box::default()));
            return;
        };
        let control = Control {
            stop: StopSignal::default(),
            events: Events::to_bridge(self.events.clone(), &job),
        };
        self.stops.insert(job.chat.clone(), control.stop.clone());
        if job.work == Work::Prompt {
            self.relay.begin(&job);
            self.files.changed = true;
        }
        let real = self
            .make_new_folder(&job)
            .and_then(|()| real_chat_folder(&self.walk, Path::new(&job.cwd)));
        let job = match real {
            Ok(cwd) => Job { cwd, ..job },
            Err(refused) => {
                let run = Run {
                    reply: Err(refused),
                    session: None,
                };
                let _ = finished.send(Finished::Run(job, run, Box::default()));
                return;
            }
        };
        let raise = self.raise_for(&job);
        let plan = self.relay.branch_plan(&job.chat);
        let git = self.git.clone();
        thread::spawn(move || {
            let mut job = job;
            if let Some((raiser, level)) = raise {
                job.permission = raise_level(&raiser, &job, level, &control);
            }
            let (run, end) = run_with_git(agent.as_ref(), &mut job, &control, git.as_ref(), plan);
            let _ = finished.send(Finished::Run(job, run, Box::new(end)));
        });
    }

    /// The folder is made before the run, so the agent starts in it (SPEC.md 9.9).
    fn make_new_folder(&self, job: &Job) -> Result<(), String> {
        if !job.new_folder {
            return Ok(());
        }
        log(&format!("new folder for {} #{}", job.chat, job.id.0));
        make_folder(&self.walk, Path::new(&job.cwd)).map_err(|e| e.text())
    }

    /// The raise that a job carries, if any. The config is checked first, so the
    /// user never approves a change that the bridge cannot write.
    fn raise_for(&mut self, job: &Job) -> Option<(Raiser, Permission)> {
        let raiser = self.raiser.as_ref()?;
        if job.work != Work::Prompt || job.permission >= job.asked {
            return None;
        }
        if !self.raises.may_ask(&job.agent, Instant::now()) {
            log(&format!("raise {}: no dialog now", job.agent));
            return None;
        }
        if let Err(e) = raiser.can_raise(&job.agent, job.asked) {
            log(&format!(
                "raise {}: config.toml cannot change: {e:#}",
                job.agent
            ));
            self.raises
                .answered(&job.agent, Raised::NotRaised, Instant::now());
            return None;
        }
        self.raises.asked();
        Some((raiser.clone(), job.asked))
    }

    fn start_list(&self, job: Job) {
        log(&format!("list sessions #{}", job.id.0));
        let agents = self.agents.clone();
        let finished = self.finished.clone();
        thread::spawn(move || {
            let found = list_sessions(&agents, &job.cwd);
            let _ = finished.send(Finished::List(job, found));
        });
    }

    fn start_folder_list(&self, job: Job) {
        log(&format!("list folders #{}", job.id.0));
        let walk = self.walk.clone();
        let finished = self.finished.clone();
        thread::spawn(move || {
            let found = folder_walk::walk_folders(&walk, &folder_walk::LIMITS);
            let _ = finished.send(Finished::Folders(job, found));
        });
    }

    fn remove_rules(&mut self) {
        for id in self.relay.take_rule_removals() {
            match self.settings.rules.store.remove(&id, now()) {
                Ok(found) => log(&format!("rule {id} removed from the game: {found}")),
                Err(e) => log(&format!("rule {id} not removed: {e:#}")),
            }
        }
    }

    fn signal_stops(&mut self) {
        for chat in self.relay.take_cancels() {
            self.signal(&chat, StopReason::Stop);
        }
        for chat in self.relay.take_interrupts() {
            self.signal(&chat, StopReason::NewMessage);
        }
    }

    fn signal(&self, chat: &ChatId, reason: StopReason) {
        if let Some(stop) = self.stops.get(chat) {
            log(&format!("stop {chat}: {reason:?}"));
            stop.request_for(reason);
        }
    }

    fn take_events(&mut self) {
        while let Ok((chat, id, event)) = self.run_events.try_recv() {
            match event {
                Event::Progress(line) => self.relay.step(&chat, id, line),
                Event::Desktop(notice) => {
                    log(&format!("{} #{}: {}", chat, id.0, notice.line()));
                    self.relay.desktop(&chat, id, notice);
                }
                Event::Withdrawn => {
                    self.relay.withdraw(&chat, id);
                    let relay = &self.relay;
                    self.answers.retain(|request, _| relay.is_asked(request));
                }
                Event::Question(question) => {
                    let request = self
                        .relay
                        .ask(&chat, id, question.text, question.choices, now());
                    log(&format!("ask {} #{} as {request}", chat, id.0));
                    self.answers.insert(request, question.answer);
                }
                Event::Raised {
                    agent,
                    level,
                    raised,
                } => {
                    self.raises.answered(&agent, raised, Instant::now());
                    // An approved raise gives the run the level that it asked for.
                    if raised == Raised::Approved {
                        self.relay.set_level(&agent, level);
                        self.relay.show_level(&chat, id, level, level);
                    }
                }
            }
            self.files.changed = true;
        }
    }

    fn pass_answers(&mut self) {
        for (request, choice) in self.relay.take_answers() {
            log(&format!("answer {request}"));
            if let Some(answer) = self.answers.remove(&request) {
                let _ = answer.send(choice);
            }
            self.files.changed = true;
        }
    }

    fn finish_runs(&mut self) {
        while let Ok(done) = self.results.try_recv() {
            let (job, run, end) = match done {
                Finished::Run(job, run, end) => (job, run, end),
                Finished::Git(job, done) => {
                    log(&format!("git done {} #{}", job.chat, job.id.0));
                    self.stops.remove(&job.chat);
                    self.relay.finish_git(&job, done.reply, &done.effect);
                    self.files.changed = true;
                    continue;
                }
                Finished::List(job, found) => {
                    self.relay.finish_list(&job, found, now());
                    self.files.changed = true;
                    continue;
                }
                Finished::Folders(job, found) => {
                    self.relay.finish_folders(&job, &found);
                    self.files.changed = true;
                    continue;
                }
            };
            log(&format!("done {} #{}", job.chat, job.id.0));
            self.stops.remove(&job.chat);
            if let WorktreeChange::Set(worktree) = end.change {
                self.relay.set_worktree(&job.chat, worktree);
            }
            self.relay.keep_session(&job, run.session);
            self.relay.finish_run(&job, run.reply, &end.blocks);
            // A request of a run that ended gets no answer: its run stopped waiting.
            let relay = &self.relay;
            self.answers.retain(|request, _| relay.is_asked(request));
            self.files.changed = true;
        }
    }

    /// A failed publish waits for the next heartbeat, so it does not log every tick.
    fn publish(&mut self, addons: &Path, line: Option<LineChoice>) {
        let body = with_line(self.relay.body(now()), App::Relay, line);
        let files = Files {
            body: slots::with_bad_tags(body, App::Relay, self.bad_tags),
            restore: self.relay.restore_file(),
            live: self.relay.live_file(&self.terminal.notices()),
        };
        if let Err(e) = slots::publish(addons, App::Relay, &files, self.relay.next_slot()) {
            log(&format!("publish failed: {e:#}"));
        }
        self.files.changed = false;
        self.notices_changed = false;
    }
}

/// Runs in the thread of the run: the own branch of the chat first, then the agent in
/// its folder, then the blocks of the bridge (SPEC.md 9.10).
fn run_with_git(
    agent: &dyn Agent,
    job: &mut Job,
    control: &Control,
    git: Option<&RunGit>,
    plan: BranchPlan,
) -> (Run, RunEnd) {
    let started = match git.map(|g| g.start(job, plan)) {
        Some(Ok(started)) => Some(started),
        Some(Err(refused)) => {
            let run = Run {
                reply: Err(refused),
                session: None,
            };
            return (run, RunEnd::default());
        }
        None => None,
    };
    if let Some(started) = &started {
        job.cwd.clone_from(&started.folder);
    }
    let run = if control.stop.requested() {
        Run {
            reply: Err(STOPPED.into()),
            session: job.resume.clone(),
        }
    } else {
        agent.run(job, control)
    };
    let end = match (git, started) {
        (Some(git), Some(started)) => RunEnd {
            blocks: git.end(&started),
            change: started.change,
        },
        _ => RunEnd::default(),
    };
    (run, end)
}

/// Runs in the thread of the run. Returns the level of the run after the raise.
fn raise_level(asker: &Raiser, job: &Job, level: Permission, control: &Control) -> Permission {
    let raised = asker.ask(job, level, control);
    let now_level = if raised == Raised::Approved {
        level
    } else {
        job.permission
    };
    control.events.send(Event::Raised {
        agent: job.agent.clone(),
        level: now_level,
        raised,
    });
    now_level
}

fn log_version(app: App, reported: u32) {
    let fit = version_fit(app, reported);
    match update_text(app, fit) {
        None => log(&format!("{app:?} addon version {reported}")),
        Some(update) => log(&format!(
            "{app:?} addon version {reported} is not supported: {update}"
        )),
    }
}

/// The records of each outbox frame that the key of `app` signed. Any other frame is
/// refused (SPEC.md 9.7, decision 3).
fn outbox_records(app: App, text: &str, keys: &KeySet) -> Vec<(FrameTag, Vec<Record>)> {
    let mut all = Vec::new();
    for frame in saved::frames(text) {
        match (receive_for(app, &frame, keys, now()), frame_tag(&frame)) {
            (Ok(records), Some(tag)) => all.push((tag, records)),
            (Err(reason), _) => log(&format!(
                "{app:?}: skipped a saved message: {}",
                rejected_text(&reason)
            )),
            (Ok(_), None) => log(&format!("{app:?} outbox frame with no tag")),
        }
    }
    all
}

impl TimewaysLane {
    fn open(paths: &Paths) -> Result<TimewaysLane> {
        let dir = paths.state.join(TIMEWAYS_DIR);
        crate::fs_safe::make_private_dir(&dir)?;
        let timeways = match state::load(&dir)? {
            Some(saved) => Timeways::from_state(saved),
            None => Timeways::new(),
        };
        log("Timeways lane on");
        Ok(TimewaysLane {
            timeways,
            files: LaneFiles::new(dir, &paths.accounts, App::Timeways),
            story: None,
        })
    }

    fn step(&mut self, keys: &KeySet, addons: &Path, line: &mut LineFile) {
        self.take_saved_variables(keys);
        if self.files.publish_due() {
            self.store();
            self.publish(addons, line.choice());
            self.files.last_publish = Instant::now();
        }
        // A message reaches the story program only after `store` marked it as seen on
        // disk, as a run of the relay does.
        if self.files.stored {
            self.send_to_story();
        }
        self.take_story_replies();
    }

    fn take_saved_variables(&mut self, keys: &KeySet) {
        for text in self.files.saved.changed() {
            self.timeways.reset_window();
            self.files.changed = true;
            for (_, records) in outbox_records(App::Timeways, &text, keys) {
                self.take_records(&records, "outbox");
            }
        }
    }

    fn take_records(&mut self, records: &[Record], source: &str) {
        let version = self.timeways.addon_version();
        let outcomes = self.timeways.on_frame(records, now());
        if let Some(new) = self
            .timeways
            .addon_version()
            .filter(|v| Some(*v) != version)
        {
            log_version(App::Timeways, new);
        }
        let accepted = outcomes.iter().filter(|o| **o == Outcome::Accepted).count();
        log(&format!(
            "timeways {source}: {} records, {accepted} new",
            records.len()
        ));
        self.files.changed = true;
    }

    fn send_to_story(&mut self) {
        let messages = self.timeways.take_messages();
        let Some(story) = &mut self.story else {
            for message in &messages {
                self.timeways.answer(message, Err(NO_STORY.into()));
                self.files.changed = true;
            }
            return;
        };
        for message in messages {
            story.send(message);
        }
    }

    /// The bridge writes the slot body from each answer with the proved writers. The
    /// story program never writes a file that the game reads.
    fn take_story_replies(&mut self) {
        let Some(story) = &mut self.story else {
            return;
        };
        story.step();
        for (message, reply) in story.take_replies() {
            self.timeways.answer(&message, reply);
            self.files.changed = true;
        }
    }

    fn store(&mut self) {
        let result = state::save(&self.files.state, &self.timeways.to_state());
        if let Err(e) = &result {
            log(&format!("cannot save the Timeways state: {e:#}"));
        }
        self.files.stored = result.is_ok();
    }

    /// Setup makes the Timeways slots only for a player with the Timeways addon, so
    /// missing slots are normal.
    fn publish(&mut self, addons: &Path, line: Option<LineChoice>) {
        self.files.changed = false;
        if !slots::is_installed(addons, App::Timeways) {
            return;
        }
        let files = Files {
            body: with_line(self.timeways.body(now()), App::Timeways, line),
            ..Files::empty(App::Timeways, now())
        };
        if let Err(e) = slots::publish(addons, App::Timeways, &files, self.timeways.next_slot()) {
            log(&format!("Timeways publish failed: {e:#}"));
        }
    }
}

/// The folders of the bridge resolve as the classifier sees them (SPEC.md 6.6.3).
fn repo_walk(policy: &Policy, paths: &Paths) -> Walk {
    let roots = policy.folders.roots.iter();
    Walk {
        roots: roots
            .map(|r| PathBuf::from(String::from_utf8_lossy(r).into_owned()))
            .collect(),
        deny: [&paths.config, &paths.state]
            .map(|d| resolve(d).unwrap_or_else(|| d.clone()))
            .into(),
        home: home_folder().and_then(|h| resolve(&h)),
    }
}

fn home_folder() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// An agent that fails to list is left out. Only when every agent fails is the list
/// an error.
fn list_sessions(agents: &Agents, cwd: &str) -> Found {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for (name, agent) in agents {
        match agent.sessions(cwd) {
            Ok(sessions) => found.extend(sessions.into_iter().map(|s| (name.clone(), s))),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    if found.is_empty() && !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(found)
}

/// With no relay part, `relay` is `None`, and the bridge serves Timeways alone.
pub fn run(
    paths: Paths,
    relay: Option<(Policy, Agents, Raiser, BridgeSettings)>,
    keys: KeySet,
    story: Option<StorySpec>,
) -> Result<()> {
    log(&format!("watching {}", paths.screenshots.display()));
    let mut bridge = match relay {
        Some((policy, agents, raiser, settings)) => Bridge::new(paths, policy, keys, agents)?
            .with_raises(raiser)
            .with_settings(settings),
        None => Bridge::without_relay(paths, keys)?,
    };
    if let Some(spec) = story {
        bridge = bridge.with_story(spec);
    }
    loop {
        bridge.step();
        thread::sleep(TICK);
    }
}
