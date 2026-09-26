//! The main loop: screenshots in, agent runs, slots out (SPEC.md 8.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::agent::{Agents, Control, Event, Events, Run, SessionInfo, StopSignal};
use crate::config::Policy;
use crate::receive::{KeySet, receive, receive_for};
use protocol::apps::App;
use protocol::record::Record;
use protocol::version::version_fit;

use crate::relay::{ChatId, Job, MessageId, Outcome, Relay, Work};
use crate::saved;
use crate::screenshots::{Watcher, read_strip};
use crate::slots::{self, Files};
use crate::state;
use crate::story::{Story, StorySpec};
use crate::timeways::{NO_STORY, Timeways};
use crate::versions::update_text;

const TICK: Duration = Duration::from_millis(250);
/// The addon calls the bridge offline after 12 minutes without a new body.
const HEARTBEAT: Duration = Duration::from_mins(1);
/// The folder of the Timeways state, inside the data folder (SPEC.md 9.7, decision 4).
pub const TIMEWAYS_DIR: &str = "timeways";

type Found = Result<Vec<(String, SessionInfo)>, String>;

enum Finished {
    Run(Job, Run),
    List(Job, Found),
}
type RunEvent = (ChatId, MessageId, Event);

pub struct Paths {
    pub addons: PathBuf,
    pub screenshots: PathBuf,
    /// `WTF/Account`, which holds the saved variables of each account.
    pub accounts: PathBuf,
    /// The data folder of the bridge, for `state.json`.
    pub state: PathBuf,
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
    /// The stop signal of each run in progress, by chat.
    stops: BTreeMap<ChatId, StopSignal>,
    events: Sender<RunEvent>,
    run_events: Receiver<RunEvent>,
    /// Where the answer to each open permission request goes, by request id.
    answers: BTreeMap<String, Sender<Option<usize>>>,
    finished: Sender<Finished>,
    results: Receiver<Finished>,
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
            keys,
        })
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
            relay.step(&self.keys, &self.addons);
        }
        if let Some(timeways) = &mut self.timeways {
            timeways.step(&self.keys, &self.addons);
        }
    }

    fn take_screenshots(&mut self) {
        for path in self.watcher.ready() {
            let keys = &self.keys;
            let tag_checks = |bytes: &[u8]| receive(bytes, keys, now()).is_ok();
            let bytes = match read_strip(&path, tag_checks) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    log(&format!("skipped {}: {e}", path.display()));
                    continue;
                }
            };
            if !self.take_strip(&bytes) {
                log(&format!("rejected {}", path.display()));
                continue;
            }
            // Only a valid strip goes, never a screenshot of the user (SPEC.md 6.2, rule 8).
            if let Err(e) = std::fs::remove_file(&path) {
                log(&format!("cannot delete {}: {e}", path.display()));
            }
        }
    }

    /// Returns false for a frame that fails a check, or whose app has no lane.
    fn take_strip(&mut self, bytes: &[u8]) -> bool {
        let (app, records) = match receive(bytes, &self.keys, now()) {
            Ok(routed) => routed,
            Err(reason) => {
                log(&format!("strip rejected: {reason:?}"));
                return false;
            }
        };
        match (app, &mut self.relay, &mut self.timeways) {
            (App::Relay, Some(relay), _) => relay.take_records(&records, "strip"),
            (App::Timeways, _, Some(timeways)) => timeways.take_records(&records, "strip"),
            // `KeySet` routes to Timeways only with a Timeways key, and that key makes the lane.
            (App::Relay, None, _) | (App::Timeways, _, None) => {
                log(&format!("strip of {app:?}, which is off"));
                return false;
            }
        }
        true
    }
}

impl RelayLane {
    fn open(paths: &Paths, policy: Policy, agents: Agents) -> Result<RelayLane> {
        let relay = match state::load(&paths.state)? {
            Some(saved) => Relay::from_state(policy, saved),
            None => Relay::new(policy),
        };
        let (finished, results) = channel();
        let (events, run_events) = channel();
        Ok(RelayLane {
            relay,
            files: LaneFiles::new(paths.state.clone(), &paths.accounts, App::Relay),
            agents,
            stops: BTreeMap::new(),
            events,
            run_events,
            answers: BTreeMap::new(),
            finished,
            results,
        })
    }

    fn step(&mut self, keys: &KeySet, addons: &Path) {
        self.take_saved_variables(keys);
        self.signal_stops();
        self.take_events();
        self.pass_answers();
        self.finish_runs();
        if self.files.publish_due() {
            self.store();
            self.publish(addons);
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

    /// A changed file means a `/reload`: the outbox frames get the same checks as a strip.
    fn take_saved_variables(&mut self, keys: &KeySet) {
        for text in self.files.saved.changed() {
            self.relay.reset_window();
            self.files.changed = true;
            for records in outbox_records(App::Relay, &text, keys) {
                self.take_records(&records, "outbox");
            }
        }
    }

    fn take_records(&mut self, records: &[Record], source: &str) {
        let build = self.relay.client_build().map(str::to_owned);
        let version = self.relay.addon_version();
        let outcomes = self.relay.on_frame(records, now());
        if let Some(new) = self
            .relay
            .client_build()
            .filter(|b| Some(*b) != build.as_deref())
        {
            log(&format!("game build {new}: screenshots and slots work"));
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
            if job.work == Work::ListSessions {
                self.start_list(job);
                continue;
            }
            log(&format!(
                "run {} #{} with {} at {:?}",
                job.chat.0, job.id.0, job.agent, job.permission
            ));
            let finished = self.finished.clone();
            // The policy refuses an agent that the config does not have, so this is a guard.
            let Some(agent) = self.agents.get(&job.agent).map(Arc::clone) else {
                let run = Run {
                    reply: Err("Agent not set up.".into()),
                    session: None,
                };
                let _ = finished.send(Finished::Run(job, run));
                continue;
            };
            let control = Control {
                stop: StopSignal::default(),
                events: Events::to_bridge(self.events.clone(), &job),
            };
            self.stops.insert(job.chat.clone(), control.stop.clone());
            thread::spawn(move || {
                let run = agent.run(&job, &control);
                let _ = finished.send(Finished::Run(job, run));
            });
        }
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

    fn signal_stops(&mut self) {
        for chat in self.relay.take_cancels() {
            if let Some(stop) = self.stops.get(&chat) {
                log(&format!("stop {}", chat.0));
                stop.request();
            }
        }
    }

    fn take_events(&mut self) {
        while let Ok((chat, id, event)) = self.run_events.try_recv() {
            match event {
                Event::Progress(line) => self.relay.step(&chat, id, line),
                Event::Question(question) => {
                    let request = self
                        .relay
                        .ask(&chat, id, question.text, question.choices, now());
                    log(&format!("ask {} #{} as {request}", chat.0, id.0));
                    self.answers.insert(request, question.answer);
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
            let (job, run) = match done {
                Finished::Run(job, run) => (job, run),
                Finished::List(job, found) => {
                    self.relay.finish_list(&job, found, now());
                    self.files.changed = true;
                    continue;
                }
            };
            log(&format!("done {} #{}", job.chat.0, job.id.0));
            self.stops.remove(&job.chat);
            self.relay.keep_session(&job, run.session);
            self.relay.finish(&job, run.reply);
            // A request of a run that ended gets no answer: its run stopped waiting.
            let relay = &self.relay;
            self.answers.retain(|request, _| relay.is_asked(request));
            self.files.changed = true;
        }
    }

    /// A failed publish waits for the next heartbeat, so it does not log every tick.
    fn publish(&mut self, addons: &Path) {
        let files = Files {
            body: self.relay.body(now()),
            restore: self.relay.restore_file(),
            live: self.relay.live_file(),
        };
        if let Err(e) = slots::publish(addons, App::Relay, &files, self.relay.next_slot()) {
            log(&format!("publish failed: {e:#}"));
        }
        self.files.changed = false;
    }
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
fn outbox_records(app: App, text: &str, keys: &KeySet) -> Vec<Vec<Record>> {
    let mut all = Vec::new();
    for frame in saved::frames(text) {
        match receive_for(app, &frame, keys, now()) {
            Ok(records) => all.push(records),
            Err(reason) => log(&format!("{app:?} outbox rejected: {reason:?}")),
        }
    }
    all
}

impl TimewaysLane {
    fn open(paths: &Paths) -> Result<TimewaysLane> {
        let dir = paths.state.join(TIMEWAYS_DIR);
        std::fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
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

    fn step(&mut self, keys: &KeySet, addons: &Path) {
        self.take_saved_variables(keys);
        if self.files.publish_due() {
            self.store();
            self.publish(addons);
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
            for records in outbox_records(App::Timeways, &text, keys) {
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
    fn publish(&mut self, addons: &Path) {
        self.files.changed = false;
        if !slots::is_installed(addons, App::Timeways) {
            return;
        }
        let files = Files {
            body: self.timeways.body(now()),
            ..Files::empty(App::Timeways, now())
        };
        if let Err(e) = slots::publish(addons, App::Timeways, &files, self.timeways.next_slot()) {
            log(&format!("Timeways publish failed: {e:#}"));
        }
    }
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
    relay: Option<(Policy, Agents)>,
    keys: KeySet,
    story: Option<StorySpec>,
) -> Result<()> {
    log(&format!("watching {}", paths.screenshots.display()));
    let mut bridge = match relay {
        Some((policy, agents)) => Bridge::new(paths, policy, keys, agents)?,
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
