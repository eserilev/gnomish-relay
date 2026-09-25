//! The main loop: screenshots in, agent runs, slots out (SPEC.md 8.2).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::agent::{Agents, Control, Event, Events, Run, StopSignal};
use crate::config::Policy;
use crate::receive::{StripKey, receive};
use crate::relay::{ChatId, Job, MessageId, Outcome, Relay};
use crate::saved;
use crate::screenshots::{Watcher, read_strip};
use crate::slots::{self, Files};
use crate::state;

const TICK: Duration = Duration::from_millis(250);
/// The addon calls the bridge offline after 12 minutes without a new body.
const HEARTBEAT: Duration = Duration::from_mins(1);

type Finished = (Job, Run);
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
fn log(line: &str) {
    eprintln!("{} {}", now(), line.escape_debug());
}

/// The bridge between the Screenshots folder, the agents, and the slots. `run`
/// calls `step` four times a second. Tests call it directly.
pub struct Bridge {
    paths: Paths,
    key: StripKey,
    agents: Agents,
    /// The stop signal of each run in progress, by chat.
    stops: BTreeMap<ChatId, StopSignal>,
    events: Sender<RunEvent>,
    run_events: Receiver<RunEvent>,
    /// Where the answer to each open permission request goes, by request id.
    answers: BTreeMap<String, Sender<Option<usize>>>,
    relay: Relay,
    watcher: Watcher,
    saved: saved::Watcher,
    finished: Sender<Finished>,
    results: Receiver<Finished>,
    changed: bool,
    stored: bool,
    last_publish: Instant,
}

impl Bridge {
    pub fn new(paths: Paths, policy: Policy, key: StripKey, agents: Agents) -> Result<Bridge> {
        let relay = match state::load(&paths.state)? {
            Some(saved) => Relay::from_state(policy, saved),
            None => Relay::new(policy),
        };
        let (finished, results) = channel();
        let (events, run_events) = channel();
        Ok(Bridge {
            watcher: Watcher::new(&paths.screenshots),
            saved: saved::Watcher::new(&paths.accounts),
            paths,
            key,
            agents,
            stops: BTreeMap::new(),
            events,
            run_events,
            answers: BTreeMap::new(),
            relay,
            finished,
            results,
            changed: true,
            stored: false,
            last_publish: Instant::now(),
        })
    }

    pub fn step(&mut self) {
        self.take_screenshots();
        self.take_saved_variables();
        self.signal_stops();
        self.take_events();
        self.pass_answers();
        self.finish_runs();
        if self.changed || self.last_publish.elapsed() >= HEARTBEAT {
            self.store();
            self.publish();
            self.last_publish = Instant::now();
        }
        // A run starts only when its message is marked as seen on disk, so a crash
        // cannot run it twice.
        if self.stored {
            self.start_runs();
        }
    }

    /// The state after `start_runs` needs no write: a running job is a working record,
    /// and a restart ends it as an error.
    fn store(&mut self) {
        let result = state::save(&self.paths.state, &self.relay.to_state());
        if let Err(e) = &result {
            log(&format!("cannot save the state: {e:#}"));
        }
        self.stored = result.is_ok();
    }

    fn take_screenshots(&mut self) {
        for path in self.watcher.ready() {
            let bytes = match read_strip(&path) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    log(&format!("skipped {}: {e}", path.display()));
                    continue;
                }
            };
            if !self.take_frame(&bytes, "strip") {
                log(&format!("rejected {}", path.display()));
                continue;
            }
            // Only a valid strip goes, never a screenshot of the user (SPEC.md 6.2, rule 8).
            if let Err(e) = std::fs::remove_file(&path) {
                log(&format!("cannot delete {}: {e}", path.display()));
            }
        }
    }

    /// A changed file means a `/reload`: the outbox frames get the same checks as a strip.
    fn take_saved_variables(&mut self) {
        for text in self.saved.changed() {
            self.relay.reset_window();
            self.changed = true;
            for frame in saved::frames(&text) {
                self.take_frame(&frame, "outbox");
            }
        }
    }

    /// Returns false for a frame that fails the tag, the time, or the format check.
    fn take_frame(&mut self, bytes: &[u8], source: &str) -> bool {
        match receive(bytes, &self.key, now()) {
            Ok(records) => {
                let build = self.relay.client_build().map(str::to_owned);
                let outcomes = self.relay.on_frame(&records, now());
                if let Some(new) = self
                    .relay
                    .client_build()
                    .filter(|b| Some(*b) != build.as_deref())
                {
                    log(&format!("game build {new}: screenshots and slots work"));
                }
                let accepted = outcomes.iter().filter(|o| **o == Outcome::Accepted).count();
                log(&format!(
                    "{source}: {} records, {accepted} new",
                    records.len()
                ));
                self.changed = true;
                true
            }
            Err(reason) => {
                log(&format!("{source} rejected: {reason:?}"));
                false
            }
        }
    }

    fn start_runs(&mut self) {
        while let Some(job) = self.relay.next_job() {
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
                let _ = finished.send((job, run));
                continue;
            };
            let control = Control {
                stop: StopSignal::default(),
                events: Events::to_bridge(self.events.clone(), &job),
            };
            self.stops.insert(job.chat.clone(), control.stop.clone());
            thread::spawn(move || {
                let run = agent.run(&job, &control);
                let _ = finished.send((job, run));
            });
        }
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
            self.changed = true;
        }
    }

    fn pass_answers(&mut self) {
        for (request, choice) in self.relay.take_answers() {
            log(&format!("answer {request}"));
            if let Some(answer) = self.answers.remove(&request) {
                let _ = answer.send(choice);
            }
            self.changed = true;
        }
    }

    fn finish_runs(&mut self) {
        while let Ok((job, run)) = self.results.try_recv() {
            log(&format!("done {} #{}", job.chat.0, job.id.0));
            self.stops.remove(&job.chat);
            self.relay.keep_session(&job, run.session);
            self.relay.finish(&job, run.reply);
            // A request of a run that ended gets no answer: its run stopped waiting.
            let relay = &self.relay;
            self.answers.retain(|request, _| relay.is_asked(request));
            self.changed = true;
        }
    }

    /// A failed publish waits for the next heartbeat, so it does not log every tick.
    fn publish(&mut self) {
        let files = Files {
            body: self.relay.body(now()),
            restore: self.relay.restore_file(),
            live: self.relay.live_file(),
        };
        if let Err(e) = slots::publish(&self.paths.addons, &files, self.relay.next_slot()) {
            log(&format!("publish failed: {e:#}"));
        }
        self.changed = false;
    }
}

pub fn run(paths: Paths, policy: Policy, key: StripKey, agents: Agents) -> Result<()> {
    log(&format!("watching {}", paths.screenshots.display()));
    let mut bridge = Bridge::new(paths, policy, key, agents)?;
    loop {
        bridge.step();
        thread::sleep(TICK);
    }
}
