//! The agents that answer messages (SPEC.md 9).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::acp::AcpAgent;
use crate::config::{Config, Kind};
use crate::relay::Job;

/// Set by Stop in the game while a run is in progress.
#[derive(Clone, Default)]
pub struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    pub fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The end of one run.
pub struct Run {
    /// The final reply, or an error text for the user.
    pub reply: Result<String, String>,
    /// The agent session, so the next message of the chat can resume it. A failed
    /// run can have one too.
    pub session: Option<String>,
}

pub trait Agent: Send + Sync {
    fn run(&self, job: &Job, stop: &StopSignal) -> Run;
}

/// Answers with the message itself. It proves the whole path through the game.
pub struct Echo;

impl Agent for Echo {
    fn run(&self, job: &Job, _stop: &StopSignal) -> Run {
        Run {
            reply: Ok(format!("echo: {}", job.text)),
            session: None,
        }
    }
}

/// Each agent of the config, by name.
pub type Agents = BTreeMap<String, Arc<dyn Agent>>;

pub fn from_config(config: &Config) -> Agents {
    config
        .agents
        .iter()
        .map(|(name, spec)| {
            let agent: Arc<dyn Agent> = match spec.kind {
                Kind::Echo => Arc::new(Echo),
                Kind::Acp => Arc::new(AcpAgent {
                    command: spec.command.clone(),
                    env: spec.env.clone(),
                    modes: spec.modes.clone(),
                    timeout: config.timeout,
                }),
            };
            (name.clone(), agent)
        })
        .collect()
}
