//! The main loop: screenshots in, agent runs, slots out (SPEC.md 8.2).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::agent::Agent;
use crate::receive::{StripKey, receive};
use crate::relay::{Folders, Job, Outcome, Relay};
use crate::screenshots::{Watcher, read_strip};
use crate::slots;

const TICK: Duration = Duration::from_millis(250);
/// The addon calls the bridge offline after 12 minutes without a new body.
const HEARTBEAT: Duration = Duration::from_mins(1);

type Finished = (Job, Result<String, String>);

pub struct Paths {
    pub addons: PathBuf,
    pub screenshots: PathBuf,
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
    agent: Arc<dyn Agent>,
    relay: Relay,
    watcher: Watcher,
    finished: Sender<Finished>,
    results: Receiver<Finished>,
    changed: bool,
    last_publish: Instant,
}

impl Bridge {
    pub fn new(paths: Paths, folders: Folders, key: StripKey, agent: Arc<dyn Agent>) -> Bridge {
        let (finished, results) = channel();
        Bridge {
            watcher: Watcher::new(&paths.screenshots),
            paths,
            key,
            agent,
            relay: Relay::new(folders),
            finished,
            results,
            changed: true,
            last_publish: Instant::now(),
        }
    }

    pub fn step(&mut self) {
        self.take_screenshots();
        self.start_runs();
        self.finish_runs();
        if self.changed || self.last_publish.elapsed() >= HEARTBEAT {
            self.publish();
            self.last_publish = Instant::now();
        }
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
            match receive(&bytes, &self.key, now()) {
                Ok(records) => {
                    let outcomes = self.relay.on_frame(&records, now());
                    let accepted = outcomes.iter().filter(|o| **o == Outcome::Accepted).count();
                    log(&format!("strip: {} records, {accepted} new", records.len()));
                    // Only a valid strip goes, never a screenshot of the user (SPEC.md 6.2, rule 8).
                    if let Err(e) = std::fs::remove_file(&path) {
                        log(&format!("cannot delete {}: {e}", path.display()));
                    }
                    self.changed = true;
                }
                Err(reason) => log(&format!("rejected {}: {reason:?}", path.display())),
            }
        }
    }

    fn start_runs(&mut self) {
        while let Some(job) = self.relay.next_job() {
            log(&format!(
                "run {} #{} with {}",
                job.chat.0, job.id.0, job.agent
            ));
            let agent = Arc::clone(&self.agent);
            let finished = self.finished.clone();
            thread::spawn(move || {
                let result = agent.run(&job);
                let _ = finished.send((job, result));
            });
        }
    }

    fn finish_runs(&mut self) {
        while let Ok((job, result)) = self.results.try_recv() {
            log(&format!("done {} #{}", job.chat.0, job.id.0));
            self.relay.finish(&job, result);
            self.changed = true;
        }
    }

    /// A failed publish waits for the next heartbeat, so it does not log every tick.
    fn publish(&mut self) {
        let body = self.relay.body(now());
        if let Err(e) = slots::publish(&self.paths.addons, &body, self.relay.next_slot()) {
            log(&format!("publish failed: {e:#}"));
        }
        self.changed = false;
    }
}

pub fn run(paths: Paths, folders: Folders, key: StripKey, agent: Arc<dyn Agent>) -> Result<()> {
    log(&format!("watching {}", paths.screenshots.display()));
    let mut bridge = Bridge::new(paths, folders, key, agent);
    loop {
        bridge.step();
        thread::sleep(TICK);
    }
}
