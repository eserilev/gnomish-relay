//! The main loop: screenshots in, agent runs, slots out (SPEC.md 8.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tracing::Span;

use crate::active_folders::ActiveFolders;
use crate::agent::{
    Agent, Agents, Control, Event, Events, Run, SessionInfo, StopReason, StopSignal,
};
use crate::auto_update::{Activity, AutoUpdater, start_detached};
use crate::chat_branch;
use crate::ci_checks::CiChecks;
use crate::config::{Permission, Policy};
use crate::folder_path::real_path;
use crate::full_auto::{self, FullAutoAsker};
use crate::game_folders::GameFolders;
use crate::game_watch::GameWatch;
use crate::git_actions::{self, Context, Done, Effect, GitAction, MergeDesk};
use crate::git_blocks::RunBlocks;
use crate::git_host::GitHost;
use crate::raise::{RaiseGuard, Raised, Raiser};
use crate::receive::{KeySet, Rejected, frame_tag, receive, receive_for};
use crate::relay::BranchPlan;
use crate::roots::Roots;
use crate::run_git::{RunGit, WorktreeChange};
use crate::test_summary::{self, TestCounts};
use crate::trust::{NOT_WRITTEN, TrustGuard, Trusted, Truster};
use crate::turn::STOPPED;
use protocol::apps::App;
use protocol::record::Record;
use protocol::version::version_fit;

use crate::action_input::resolve;
use crate::daily_usage::{DailyUsage, cap_text};
use crate::folder_trust;
use crate::folder_walk::{Snapshot, Walk};
use crate::home_walk;
use crate::line_choice::{self, LineChoice, LineFile, with_line};
use crate::line_test;
use crate::logging;
use crate::new_folder::{make_folder, real_chat_folder, real_new_folder};
use crate::relay::{BAD_AGENT, ChatId, FrameTag, Job, MessageId, Outcome, Relay, Work};
use crate::saved;
use crate::screenshots::{Shot, Watcher, read_strip};
use crate::settings_list::BridgeSettings;
use crate::slots::{self, Files};
use crate::spool::{open_spool, spool_dir, take_files};
use crate::state;
use crate::status;
use crate::story::{Story, StorySpec};
use crate::subfolder_walk;
use crate::terminal_sessions::TerminalSessions;
use crate::timeways::{NO_STORY, Timeways};
use crate::usage::Usage;
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

/// What git adds to the end of a run (SPEC.md 9.11).
#[derive(Default)]
struct RunEnd {
    change: WorktreeChange,
    blocks: RunBlocks,
}

const NO_GIT: &str =
    "Git isn't available to the desktop app. Install git, then run gnomish-relay restart.";
type RunEvent = (ChatId, MessageId, Event);

pub struct Paths {
    /// Every game that the bridge serves, the configured one first (SPEC.md 7.9).
    pub games: Vec<GameFolders>,
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
/// log line (SPEC.md 6.2, rule 15). The fields of the spans around it follow (8.5).
pub fn log(line: &str) {
    if logging::is_on() {
        tracing::info!("{line}");
        return;
    }
    eprintln!("{} {}", now(), line.escape_debug());
}

/// The bridge between the Screenshots folder, the agents, and the slots. `run`
/// calls `step` four times a second. Tests call it directly.
pub struct Bridge {
    /// The `AddOns` folder of each game. Each publish writes the slots into all of them.
    addons: Vec<PathBuf>,
    /// The data folder, for the time of the last strip.
    data: PathBuf,
    /// The strip line that each publish sends to the addons (SPEC.md 7.1.3).
    line: LineFile,
    keys: KeySet,
    /// One for the `Screenshots` folder of each game.
    watchers: Vec<Watcher>,
    /// Only with the relay part in the config (SPEC.md 9.7, decision 15).
    relay: Option<RelayLane>,
    /// Only with a Timeways key. It holds no agents.
    timeways: Option<TimewaysLane>,
    /// `None` with `auto_update = false` (SPEC.md 11.3).
    auto_update: Option<AutoUpdater>,
    /// `None` in tests: a restart needs the real program (SPEC.md 7.9).
    game_watch: Option<GameWatch>,
}

/// What one app keeps on disk, and when it writes it (SPEC.md 9.7, decision 4).
struct LaneFiles {
    state: PathBuf,
    /// One for each game.
    saved: Vec<saved::Watcher>,
    changed: bool,
    stored: bool,
    last_publish: Instant,
}

impl LaneFiles {
    fn new(state: PathBuf, games: &[GameFolders], app: App) -> LaneFiles {
        LaneFiles {
            state,
            saved: games.iter().map(|g| saved::Watcher::new(g, app)).collect(),
            changed: true,
            stored: false,
            last_publish: Instant::now(),
        }
    }

    fn publish_due(&self) -> bool {
        self.changed || self.last_publish.elapsed() >= HEARTBEAT
    }

    /// The changed saved variables files of every game.
    fn changed_saved(&mut self) -> Vec<saved::SavedFile> {
        self.saved
            .iter_mut()
            .flat_map(saved::Watcher::changed)
            .collect()
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
    /// With none, no chat runs at full-auto (SPEC.md 9.3, `allow_full_auto`).
    full_auto: Option<FullAutoAsker>,
    /// The raises and the full-auto requests: one dialog at a time.
    raises: RaiseGuard,
    /// With no truster, a folder under no root never runs (SPEC.md 9.12).
    truster: Option<Truster>,
    trusts: TrustGuard,
    /// The answer to a settings list, with the levels of the relay.
    settings: BridgeSettings,
    /// Strips with a bad tag since the last good relay strip. The game shows a key
    /// mismatch only through this count.
    bad_tags: u32,
    /// The tokens and the cost of each day, and the cap of the config (SPEC.md 9.10).
    usage: DailyUsage,
    cost_cap: Option<f64>,
    /// The terminal sessions of the hooks, and the spool folder that brings their events.
    terminal: TerminalSessions,
    spool: PathBuf,
    notices_changed: bool,
    /// With no git on this computer, runs have no own branch and no summary (SPEC.md 9.11).
    git: Option<RunGit>,
    /// The test line of each run in progress, from the output of its commands.
    tests: BTreeMap<ChatId, TestCounts>,
    /// The log span of each run in progress, by chat (SPEC.md 8.5).
    spans: BTreeMap<ChatId, Span>,
    /// The replies that the next publish writes, with their spans.
    replies: Vec<(ChatId, MessageId, Span)>,
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
            watchers: paths
                .games
                .iter()
                .map(|g| Watcher::new(&g.screenshots))
                .collect(),
            timeways,
            relay,
            addons: paths.games.iter().map(|g| g.addons.clone()).collect(),
            line: LineFile::new(&paths.state),
            data: paths.state,
            keys,
            auto_update: None,
            game_watch: None,
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

    /// A chat at full-auto then runs with no question after one desktop Approve.
    #[must_use]
    pub fn with_full_auto(mut self, asker: FullAutoAsker) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.full_auto = Some(asker);
        }
        self
    }

    /// A chat in a folder of the home folder under no root then waits for a click on the
    /// desktop, which adds the folder to the roots (SPEC.md 9.12).
    #[must_use]
    pub fn with_trust(mut self, truster: Truster) -> Bridge {
        if let Some(lane) = &mut self.relay {
            lane.walk.roots = truster.roots.clone();
            if let Some(git) = &mut lane.git {
                git.walk.roots = truster.roots.clone();
            }
            lane.relay.take_new_folders(&truster.home);
            lane.truster = Some(truster);
        }
        self
    }

    /// At this cost in a UTC day, no new run starts (SPEC.md 9.10).
    #[must_use]
    pub fn with_cost_cap(mut self, cap: Option<f64>) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.cost_cap = cap;
        }
        self
    }

    /// Messages over the limit wait for their turn (SPEC.md 8.2).
    #[must_use]
    pub fn with_max_runs(mut self, max_runs: usize) -> Bridge {
        if let Some(lane) = &mut self.relay {
            lane.relay.set_max_runs(max_runs);
        }
        self
    }

    /// The values that the Settings and Diag tabs of the game show.
    #[must_use]
    pub fn with_settings(mut self, settings: BridgeSettings) -> Bridge {
        if let Some(relay) = &mut self.relay {
            if let Some(git) = &mut relay.git {
                git.ci = settings.ci_checks.clone();
            }
            relay.settings = settings;
        }
        self
    }

    /// Git in the chats with another host, for example one that skips the config of the
    /// user in a test.
    #[must_use]
    pub fn with_git(mut self, host: GitHost, ci: CiChecks) -> Bridge {
        if let Some(relay) = &mut self.relay {
            relay.git = Some(RunGit {
                host: Arc::new(host),
                ci,
                walk: relay.walk.clone(),
                active: ActiveFolders::default(),
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
        if let Some(updater) = &mut self.auto_update {
            updater.tick(activity(self.relay.as_ref()), Instant::now());
        }
        let busy = activity(self.relay.as_ref());
        let new_game = self
            .game_watch
            .as_mut()
            .and_then(|w| w.new_game(busy, Instant::now()));
        if let Some(game) = new_game {
            restart_for(&game, &self.data);
        }
    }

    fn take_screenshots(&mut self) {
        let ready: Vec<PathBuf> = self.watchers.iter_mut().flat_map(Watcher::ready).collect();
        for path in ready {
            let keys = &self.keys;
            let tag_checks =
                |bytes: &[u8]| receive(bytes, keys, now()).is_ok() || is_test_strip(bytes);
            let shot = match read_strip(&path, tag_checks) {
                Ok(Some(shot)) => shot,
                Ok(None) => continue,
                Err(e) => {
                    log(&format!("skipped {}: {e}", path.display()));
                    continue;
                }
            };
            if is_test_strip(&shot.bytes) {
                log(&format!(
                    "left {} in place: it's from the self-test",
                    path.display()
                ));
                continue;
            }
            let outcome = self.take_strip(&shot.bytes);
            if matches!(outcome, StripOutcome::Taken) {
                self.take_line_test(&shot);
            }
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

    /// Keeps the result of a line test and sends it with the next body (SPEC.md 7.1.4).
    fn take_line_test(&mut self, shot: &Shot) {
        let Some(choice) = line_test::result(&shot.image, &shot.bytes) else {
            return;
        };
        log(&format!(
            "line test: {}",
            line_choice::bar_text(Some(choice))
        ));
        if let Err(e) = line_choice::remember(&self.data, choice) {
            log(&format!("cannot write the line test: {e:#}"));
            return;
        }
        if let Some(relay) = &mut self.relay {
            relay.files.changed = true;
        }
        if let Some(timeways) = &mut self.timeways {
            timeways.files.changed = true;
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
        Rejected::Stale => "it's more than 5 minutes old",
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
        let (usage, problem) = DailyUsage::load(&paths.state);
        if let Some(problem) = problem {
            log(&problem);
        }
        let git = match GitHost::new() {
            Ok(host) => Some(RunGit {
                host: Arc::new(host),
                ci: CiChecks::Off,
                walk: walk.clone(),
                active: ActiveFolders::default(),
            }),
            Err(e) => {
                log(&format!("no git in chats: {e:#}"));
                None
            }
        };
        Ok(RelayLane {
            git,
            tests: BTreeMap::new(),
            spans: BTreeMap::new(),
            replies: Vec::new(),
            relay,
            files: LaneFiles::new(paths.state.clone(), &paths.games, App::Relay),
            agents,
            walk,
            stops: BTreeMap::new(),
            events,
            run_events,
            answers: BTreeMap::new(),
            finished,
            results,
            raiser: None,
            full_auto: None,
            raises: RaiseGuard::default(),
            truster: None,
            trusts: TrustGuard::default(),
            settings: BridgeSettings::default(),
            bad_tags: 0,
            usage,
            cost_cap: None,
            terminal,
            spool,
            notices_changed: false,
        })
    }

    fn step(&mut self, keys: &KeySet, addons: &[PathBuf], line: &mut LineFile) {
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
        for file in self.files.changed_saved() {
            let token = saved::saved_token(&file.text);
            self.relay.reset_window(token.as_deref());
            self.files.changed = true;
            if let Some(token) = &token {
                self.take_saved_token(&file.account, token);
            }
            for (tag, records) in outbox_records(App::Relay, &file.text, keys) {
                self.take_records(tag, &records, "outbox");
            }
        }
    }

    /// A new token in the file of an account is a wipe of that account (SPEC.md 7.6).
    fn take_saved_token(&mut self, account: &str, token: &str) {
        if let Some(old) = self.relay.saw_saved_token(account, token) {
            log(&format!(
                "{account}: new saved data, so the chats of the old one are gone ({old} is now {token})"
            ));
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
                Work::ListSubfolders => self.start_subfolder_list(job),
                Work::ListSettings => {
                    self.settings.usage_today = self.usage.today(now());
                    self.settings.strip = Some(line_choice::bar_text_of(&self.files.state));
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
        if self.relay.show_waiting() {
            self.files.changed = true;
        }
    }

    /// The bridge runs a git action itself, in a thread: a merge waits for the desktop.
    fn start_git(&mut self, job: Job, action: GitAction) {
        let span = logging::message_span(&job);
        self.spans.insert(job.chat.clone(), span.clone());
        let _in_message = span.clone().entered();
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
        let run = match action {
            GitAction::Commit(id) | GitAction::Revert(id) => {
                self.relay.changes_of(&job.chat, id).cloned()
            }
            GitAction::Merge | GitAction::Discard | GitAction::Checks => None,
        };
        let desk = self.raiser.as_ref().map(|r| MergeDesk {
            approvals: r.approvals.clone(),
            wait: r.permission_timeout,
        });
        thread::spawn(move || {
            let _in_message = span.entered();
            let context = Context {
                git: &git.host,
                worktree: worktree.as_ref(),
                run: run.as_ref(),
                folder: Path::new(&job.cwd),
                ci: &git.ci,
                desk: desk.as_ref(),
                control: &control,
            };
            let done = git_actions::perform(&action, &job.text, &context);
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
        // A message saved by an older bridge can hold full-auto from the config.
        let job = Job {
            permission: job.permission.min(Permission::AutoEdit),
            ..job
        };
        let span = logging::message_span(&job);
        self.spans.insert(job.chat.clone(), span.clone());
        let _in_message = span.clone().entered();
        log(&format!(
            "run {} #{} with {} at {:?}",
            job.chat, job.id.0, job.agent, job.permission
        ));
        if let Some(cap) = self.cap_reached(&job) {
            log(&format!(
                "{} #{}: the daily cost cap stops it",
                job.chat, job.id.0
            ));
            self.end_at_once(job, cap_text(cap));
            return;
        }
        // The policy refuses an agent that the config does not have, so this is a guard.
        let Some(agent) = self.agents.get(&job.agent).map(Arc::clone) else {
            self.end_at_once(job, BAD_AGENT.into());
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
        let trust = match self.trust_for(&job) {
            Ok(trust) => trust,
            Err(refused) => {
                self.end_at_once(job, refused);
                return;
            }
        };
        // A folder that waits for the desktop is made and checked after the click.
        let job = if trust.is_some() {
            job
        } else {
            match self.ready_folder(&job) {
                Ok(cwd) => {
                    logging::record_folder(&cwd);
                    Job { cwd, ..job }
                }
                Err(refused) => {
                    self.end_at_once(job, refused);
                    return;
                }
            }
        };
        // One message never shows two dialogs (SPEC.md 9.12).
        let full_auto = if trust.is_some() {
            FullAutoStep::Off
        } else {
            self.full_auto_for(&job)
        };
        let job = match full_auto {
            FullAutoStep::Approved => self.at_full_auto(job),
            FullAutoStep::Off | FullAutoStep::Ask(..) => job,
        };
        let raise = match full_auto {
            FullAutoStep::Off if trust.is_none() => self.raise_for(&job),
            _ => None,
        };
        let finished = self.finished.clone();
        let plan = self.relay.branch_plan(&job.chat);
        let git = self.git.clone();
        let walk = self.walk.clone();
        self.tests.remove(&job.chat);
        thread::spawn(move || {
            let _in_message = span.entered();
            let mut job = job;
            if let Some((desk, folder)) = trust
                && let Err(refused) = trust_folder(&desk, &walk, &mut job, &folder, &control)
            {
                let _ = finished.send(Finished::Run(job, never_ran(refused), Box::default()));
                return;
            }
            if let Some((raiser, level)) = raise {
                job.permission = raise_level(&raiser, &job, level, &control);
                logging::record_permission(job.permission.word());
            }
            if let FullAutoStep::Ask(asker, name) = full_auto {
                job.permission = ask_full_auto(&asker, &job, &name, &control);
            }
            let (run, end) = run_with_git(agent.as_ref(), &mut job, &control, git.as_ref(), plan);
            let _ = finished.send(Finished::Run(job, run, Box::new(end)));
        });
    }

    /// A run that never starts ends as an error, through the same path as a run.
    fn end_at_once(&self, job: Job, error: String) {
        let _ = self
            .finished
            .send(Finished::Run(job, never_ran(error), Box::default()));
    }

    /// The cap, when the cost of today reached it. A list and an attach call no model,
    /// so the cap never stops them (SPEC.md 9.10).
    fn cap_reached(&self, job: &Job) -> Option<f64> {
        let cap = self.cost_cap?;
        let reached = job.work == Work::Prompt && self.usage.cap_reached(now(), cap);
        reached.then_some(cap)
    }

    /// The real folder of the run, in a root (SPEC.md 6.2, rule 10).
    fn ready_folder(&self, job: &Job) -> Result<String, String> {
        self.make_new_folder(job)?;
        real_chat_folder(&self.walk, Path::new(&job.cwd))
    }

    /// The folder request that a job needs, if any. A folder that can never be a root
    /// ends the message with its reason, and no dialog shows (SPEC.md 9.12).
    fn trust_for(&mut self, job: &Job) -> Result<Option<(Truster, PathBuf)>, String> {
        let Some(truster) = &self.truster else {
            return Ok(None);
        };
        let folder = Path::new(&job.cwd);
        let real = if job.new_folder {
            real_new_folder(folder).map_err(|e| e.text())?
        } else {
            match real_path(folder) {
                Ok(real) => real,
                // The usual check of the folder gives the reply.
                Err(_) => return Ok(None),
            }
        };
        if truster.roots.hold(&real) {
            return Ok(None);
        }
        folder_trust::check_real(&self.walk, &truster.home, &real)
            .map_err(|why| why.text().to_owned())?;
        self.trusts.may_ask(&real, Instant::now())?;
        if let Err(e) = truster.can_trust() {
            log(&format!(
                "folder {}: config.toml cannot change: {e:#}",
                real.display()
            ));
            return Err(NOT_WRITTEN.into());
        }
        self.trusts.asked();
        Ok(Some((truster.clone(), real)))
    }

    /// The folder is made before the run, so the agent starts in it (SPEC.md 9.9).
    fn make_new_folder(&self, job: &Job) -> Result<(), String> {
        if !job.new_folder {
            return Ok(());
        }
        log(&format!("new folder for {} #{}", job.chat, job.id.0));
        make_folder(&self.walk, Path::new(&job.cwd)).map_err(|e| e.text())
    }

    /// Whether a chat at full-auto runs so at once, waits for the desktop, or does not
    /// get it (SPEC.md 9.3, "Full-auto for one chat").
    fn full_auto_for(&mut self, job: &Job) -> FullAutoStep {
        let Some(asker) = &self.full_auto else {
            return FullAutoStep::Off;
        };
        let wants = job.work == Work::Prompt && job.asked == Permission::FullAuto;
        // `ask` in the config keeps every chat at `ask`.
        let config_allows = job.permission == Permission::AutoEdit;
        if !wants || !config_allows || !asker.walls_hold(&job.agent) {
            return FullAutoStep::Off;
        }
        if self.relay.full_auto_holds(&job.chat, &job.cwd) {
            return FullAutoStep::Approved;
        }
        let key = full_auto::guard_key(&job.chat);
        if !self.raises.may_ask(&key, Instant::now()) {
            log(&format!("full-auto {}: no dialog now", job.chat));
            return FullAutoStep::Off;
        }
        self.raises.asked();
        FullAutoStep::Ask(asker.clone(), self.relay.chat_name(&job.chat))
    }

    fn at_full_auto(&mut self, job: Job) -> Job {
        log(&format!("{} #{}: full-auto", job.chat, job.id.0));
        let full = Permission::FullAuto;
        self.relay.show_level(&job.chat, job.id, full, full);
        Job {
            permission: full,
            ..job
        }
    }

    /// The raise that a job carries, if any. The config is checked first, so the
    /// user never approves a change that the bridge cannot write.
    fn raise_for(&mut self, job: &Job) -> Option<(Raiser, Permission)> {
        let raiser = self.raiser.as_ref()?;
        if job.work != Work::Prompt || !job.lowered_by_config() {
            return None;
        }
        if !self.raises.may_ask(&job.agent, Instant::now()) {
            log(&format!("raise {}: no dialog now", job.agent));
            return None;
        }
        if let Err(e) = raiser.can_raise(&job.agent, job.asked_of_config()) {
            log(&format!(
                "raise {}: config.toml cannot change: {e:#}",
                job.agent
            ));
            self.raises
                .answered(&job.agent, Raised::NotRaised, Instant::now());
            return None;
        }
        self.raises.asked();
        Some((raiser.clone(), job.asked_of_config()))
    }

    fn start_list(&self, job: Job) {
        log(&format!("list sessions #{}", job.id.0));
        let agents = self.agents.clone();
        let walk = self.walk.clone();
        let home = self.truster.as_ref().map(|t| t.home.clone());
        let finished = self.finished.clone();
        thread::spawn(move || {
            let found = list_sessions(&agents, &job.cwd)
                .map(|found| listable(found, &walk, home.as_deref()));
            let _ = finished.send(Finished::List(job, found));
        });
    }

    fn start_folder_list(&self, job: Job) {
        log(&format!("list folders #{}", job.id.0));
        let walk = self.walk.clone();
        let home = self.truster.as_ref().map(|t| t.home.clone());
        let finished = self.finished.clone();
        thread::spawn(move || {
            let found = home_walk::browse_folders(&walk, home.as_deref());
            let _ = finished.send(Finished::Folders(job, found));
        });
    }

    fn start_subfolder_list(&self, job: Job) {
        log(&format!("list subfolders #{}", job.id.0));
        let walk = self.walk.clone();
        let home = self.truster.as_ref().map(|t| t.home.clone());
        let finished = self.finished.clone();
        thread::spawn(move || {
            let folder = PathBuf::from(&job.cwd);
            let found = subfolder_walk::list_below(&walk, home.as_deref(), &folder);
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
            let _in_message = self.span_of(&chat).entered();
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
                // The last command with a test summary gives the test line of the run.
                Event::CommandOutput(output) => {
                    if let Some(counts) = test_summary::summary(&output) {
                        self.tests.insert(chat, counts);
                    }
                    continue;
                }
                Event::Question(question) => {
                    let request = self
                        .relay
                        .ask(&chat, id, question.text, question.choices, now());
                    log(&format!("ask {} #{} as {request}", chat, id.0));
                    self.answers.insert(request, question.answer);
                }
                Event::Trusted { folder, trusted } => {
                    self.trusts.answered(&folder, trusted, Instant::now());
                    if trusted == Trusted::Added {
                        log(&format!("new root: {}", folder.display()));
                        self.relay.add_root(&folder);
                        self.settings.add_root(&folder);
                    }
                }
                Event::FullAuto { folder, raised } => {
                    let key = full_auto::guard_key(&chat);
                    self.raises.answered(&key, raised, Instant::now());
                    if raised == Raised::Approved {
                        self.relay.approve_full_auto(&chat, &folder);
                        let full = Permission::FullAuto;
                        self.relay.show_level(&chat, id, full, full);
                    }
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
                    let span = self.end_span(&job);
                    let result = result_word(done.reply.is_ok());
                    span.in_scope(|| {
                        log_end(&format!("git done {} #{}", job.chat, job.id.0), result);
                    });
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
            let span = self.end_span(&job);
            let result = result_word(run.reply.is_ok());
            span.in_scope(|| log_end(&format!("done {} #{}", job.chat, job.id.0), result));
            self.stops.remove(&job.chat);
            // The thread of the run sent its events before its end, so they are all here.
            self.take_events();
            let mut blocks = end.blocks;
            blocks.tests = self.tests.remove(&job.chat);
            if let WorktreeChange::Set(worktree) = end.change {
                self.relay.set_worktree(&job.chat, worktree);
            }
            self.relay.keep_session(&job, run.session);
            self.count_usage(run.usage);
            self.relay
                .finish_run(&job, run.reply, blocks, run.usage.as_ref());
            // A request of a run that ended gets no answer: its run stopped waiting.
            let relay = &self.relay;
            self.answers.retain(|request, _| relay.is_asked(request));
            self.files.changed = true;
        }
    }

    /// Each report counts for its day, also the report of a run that failed.
    fn count_usage(&mut self, usage: Option<Usage>) {
        let Some(usage) = usage else {
            return;
        };
        if let Err(e) = self.usage.add(now(), usage) {
            log(&format!("cannot save the usage of today: {e:#}"));
        }
    }

    /// A failed publish waits for the next heartbeat, so it does not log every tick.
    fn publish(&mut self, addons: &[PathBuf], line: Option<LineChoice>) {
        let body = with_line(self.relay.body(now()), App::Relay, line);
        let files = Files {
            body: slots::with_bad_tags(body, App::Relay, self.bad_tags),
            restore: self.relay.restore_file(),
            live: self.relay.live_file(&self.terminal.notices()),
        };
        let windows = self.relay.next_slots();
        if !publish_in_each_game(addons, App::Relay, &files, &windows) {
            self.files.changed = false;
            self.notices_changed = false;
            return;
        }
        for (chat, id, span) in self.replies.drain(..) {
            span.in_scope(|| log(&format!("reply {chat} #{} written", id.0)));
        }
        self.files.changed = false;
        self.notices_changed = false;
    }

    /// The span of a run in progress, or no span.
    fn span_of(&self, chat: &ChatId) -> Span {
        self.spans.get(chat).cloned().unwrap_or_else(Span::none)
    }

    /// The span of a run that ended. It lives on until the publish of its reply.
    fn end_span(&mut self, job: &Job) -> Span {
        let span = self.spans.remove(&job.chat).unwrap_or_else(Span::none);
        self.replies.push((job.chat.clone(), job.id, span.clone()));
        span
    }
}

/// The last line of a run, with a `result` field (SPEC.md 8.5).
fn log_end(line: &str, result: &str) {
    if logging::is_on() {
        tracing::info!(result, "{line}");
        return;
    }
    log(line);
}

fn result_word(replied: bool) -> &'static str {
    if replied { "reply" } else { "error" }
}

/// Runs in the thread of the run: the own branch of the chat first, then the agent in
/// its folder, then the blocks of the bridge (SPEC.md 9.11).
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
                usage: None,
            };
            return (run, RunEnd::default());
        }
        None => None,
    };
    if let Some(started) = &started {
        job.cwd.clone_from(&started.folder);
        logging::record_folder(&job.cwd);
    }
    let run = if control.stop.requested() {
        Run {
            reply: Err(STOPPED.into()),
            session: job.resume.clone(),
            usage: None,
        }
    } else {
        agent.run(job, control)
    };
    let end = match (git, started) {
        (Some(git), Some(started)) => RunEnd {
            blocks: git.end(job, &started),
            change: started.change,
        },
        _ => RunEnd::default(),
    };
    (run, end)
}

/// The run of a message that never reached its agent.
fn never_ran(error: String) -> Run {
    Run {
        reply: Err(error),
        session: None,
        usage: None,
    }
}

/// Runs in the thread of the run: the dialog, and on Approve the real folder in its new
/// root. Else the reply of the message.
fn trust_folder(
    desk: &Truster,
    walk: &Walk,
    job: &mut Job,
    folder: &Path,
    control: &Control,
) -> Result<(), String> {
    let trusted = desk.ask(job, folder, control);
    control.events.send(Event::Trusted {
        folder: folder.to_owned(),
        trusted,
    });
    if trusted != Trusted::Added {
        return Err(trusted.text().into());
    }
    job.cwd = real_chat_folder(walk, folder)?;
    logging::record_folder(&job.cwd);
    Ok(())
}

/// A full-auto request of a run, in the thread of the run.
enum FullAutoStep {
    Off,
    /// The user approved this chat in this folder before.
    Approved,
    /// The first switch: a desktop request with the name of the chat.
    Ask(FullAutoAsker, String),
}

/// Runs in the thread of the run. Returns the level of the run after the answer.
fn ask_full_auto(asker: &FullAutoAsker, job: &Job, name: &str, control: &Control) -> Permission {
    let raised = asker.ask(job, name, control);
    control.events.send(Event::FullAuto {
        folder: job.cwd.clone(),
        raised,
    });
    if raised == Raised::Approved {
        Permission::FullAuto
    } else {
        job.permission
    }
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
    let mut skipped = Vec::new();
    for frame in saved::frames(text) {
        match (receive_for(app, &frame, keys, now()), frame_tag(&frame)) {
            (Ok(records), Some(tag)) => all.push((tag, records)),
            (Err(reason), _) => skipped.push(reason),
            (Ok(_), None) => log(&format!("{app:?} outbox frame with no tag")),
        }
    }
    for line in skipped_lines(app, &skipped) {
        log(&line);
    }
    all
}

/// The file keeps old frames across reloads, so old and early frames get one line
/// each in place of one line for each frame.
fn skipped_lines(app: App, reasons: &[Rejected]) -> Vec<String> {
    let mut lines = Vec::new();
    let stale = reasons.iter().filter(|r| **r == Rejected::Stale).count();
    if stale > 0 {
        lines.push(format!(
            "{app:?}: skipped {} older than 5 minutes",
            saved_messages(stale)
        ));
    }
    let future = reasons.iter().filter(|r| **r == Rejected::Future).count();
    if future > 0 {
        lines.push(format!(
            "{app:?}: skipped {} with a time in the future",
            saved_messages(future)
        ));
    }
    let others = reasons
        .iter()
        .filter(|r| !matches!(r, Rejected::Stale | Rejected::Future));
    for reason in others {
        lines.push(format!(
            "{app:?}: skipped a saved message: {}",
            rejected_text(reason)
        ));
    }
    lines
}

fn saved_messages(count: usize) -> String {
    if count == 1 {
        return "1 saved message".to_owned();
    }
    format!("{count} saved messages")
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
            files: LaneFiles::new(dir, &paths.games, App::Timeways),
            story: None,
        })
    }

    fn step(&mut self, keys: &KeySet, addons: &[PathBuf], line: &mut LineFile) {
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
        for file in self.files.changed_saved() {
            let token = saved::saved_token(&file.text);
            self.timeways.reset_window(token.as_deref());
            self.files.changed = true;
            for (_, records) in outbox_records(App::Timeways, &file.text, keys) {
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
    fn publish(&mut self, addons: &[PathBuf], line: Option<LineChoice>) {
        self.files.changed = false;
        let installed: Vec<PathBuf> = addons
            .iter()
            .filter(|dir| slots::is_installed(dir, App::Timeways))
            .cloned()
            .collect();
        let files = Files {
            body: with_line(self.timeways.body(now()), App::Timeways, line),
            ..Files::empty(App::Timeways, now())
        };
        let windows = self.timeways.next_slots();
        publish_in_each_game(&installed, App::Timeways, &files, &windows);
    }
}

/// Writes the slot windows into the `AddOns` folder of each game. Each game loads only
/// its own slots, and each token reads only its own records (SPEC.md 7.9). Returns
/// false when every write failed.
fn publish_in_each_game(addons: &[PathBuf], app: App, files: &Files, windows: &[usize]) -> bool {
    let mut any = addons.is_empty();
    for dir in addons {
        match slots::publish_windows(dir, app, files, windows) {
            Ok(()) => any = true,
            Err(e) => log(&format!(
                "{app:?} publish to {} failed: {e:#}",
                dir.display()
            )),
        }
    }
    any
}

/// A client that got the addon since the start needs a new bridge (SPEC.md 7.9).
fn restart_for(game: &Path, data: &Path) {
    log(&format!(
        "{}: the addon is there now, so the desktop app restarts to serve it",
        game.display()
    ));
    let started = std::env::current_exe()
        .map_err(anyhow::Error::from)
        .and_then(|exe| start_detached(&exe, data, &["restart"]));
    if let Err(e) = started {
        log(&format!("cannot restart the desktop app: {e:#}"));
    }
}

/// A desktop request always waits inside a run, so no run means no open request.
fn activity(relay: Option<&RelayLane>) -> Activity {
    match relay {
        Some(lane) if !lane.stops.is_empty() => Activity::Busy,
        _ => Activity::Idle,
    }
}

/// The folders of the bridge resolve as the classifier sees them (SPEC.md 6.6.3).
fn repo_walk(policy: &Policy, paths: &Paths) -> Walk {
    let roots = policy.folders.roots.iter();
    Walk {
        roots: Roots::new(
            roots
                .map(|r| PathBuf::from(String::from_utf8_lossy(r).into_owned()))
                .collect(),
        ),
        deny: [&paths.config, &paths.state]
            .map(|d| resolve(d).unwrap_or_else(|| d.clone()))
            .into(),
        home: home_folder().and_then(|h| resolve(&h)),
    }
}

pub fn home_folder() -> Option<PathBuf> {
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

/// The relay checks the text of each folder. This checks the disk: a gone folder, a link
/// out, and a private folder never show (SPEC.md 9.6).
fn listable(
    found: Vec<(String, SessionInfo)>,
    walk: &Walk,
    home: Option<&Path>,
) -> Vec<(String, SessionInfo)> {
    found
        .into_iter()
        .filter(|(_, info)| folder_trust::may_list(walk, home, Path::new(&info.cwd)))
        .collect()
}

/// What the relay lane takes from the config.
pub struct RelayParts {
    pub policy: Policy,
    pub agents: Agents,
    pub raiser: Raiser,
    /// `None` with `allow_full_auto = false`.
    pub full_auto: Option<FullAutoAsker>,
    pub truster: Truster,
    pub settings: BridgeSettings,
    pub max_parallel_runs: usize,
    pub daily_cost_cap_usd: Option<f64>,
}

/// With no relay part, `relay` is `None`, and the bridge serves Timeways alone.
pub fn run(
    paths: Paths,
    relay: Option<RelayParts>,
    keys: KeySet,
    story: Option<StorySpec>,
    auto_update: Option<AutoUpdater>,
    game_watch: Option<GameWatch>,
) -> Result<()> {
    for game in &paths.games {
        log(&format!("watching {}", game.screenshots.display()));
    }
    let mut bridge = match relay {
        Some(parts) => {
            let bridge = Bridge::new(paths, parts.policy, keys, parts.agents)?
                .with_raises(parts.raiser)
                .with_trust(parts.truster)
                .with_settings(parts.settings)
                .with_max_runs(parts.max_parallel_runs)
                .with_cost_cap(parts.daily_cost_cap_usd);
            match parts.full_auto {
                Some(asker) => bridge.with_full_auto(asker),
                None => bridge,
            }
        }
        None => Bridge::without_relay(paths, keys)?,
    };
    if let Some(spec) = story {
        bridge = bridge.with_story(spec);
    }
    bridge.auto_update = auto_update;
    bridge.game_watch = game_watch;
    loop {
        bridge.step();
        thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_stale_saved_messages_give_one_log_line() {
        let reasons: Vec<Rejected> = (0..30).map(|_| Rejected::Stale).collect();

        let lines = skipped_lines(App::Timeways, &reasons);

        assert_eq!(
            lines,
            vec!["Timeways: skipped 30 saved messages older than 5 minutes"]
        );
    }

    #[test]
    fn skipped_saved_messages_get_one_line_for_each_reason() {
        let reasons = [Rejected::Stale, Rejected::BadTag, Rejected::Stale];

        let lines = skipped_lines(App::Relay, &reasons);

        assert_eq!(
            lines,
            vec![
                "Relay: skipped 2 saved messages older than 5 minutes",
                "Relay: skipped a saved message: your game and the desktop app don't match",
            ]
        );
    }

    #[test]
    fn no_skipped_saved_message_gives_no_log_line() {
        assert!(skipped_lines(App::Relay, &[]).is_empty());
    }

    #[test]
    fn a_stale_message_is_named_as_more_than_5_minutes_old() {
        assert_eq!(
            rejected_text(&Rejected::Stale),
            "it's more than 5 minutes old"
        );
    }
}
