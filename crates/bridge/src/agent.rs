//! The agents that answer messages (SPEC.md 9).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::acp::AcpAgent;
use crate::config::{Config, Kind};
use crate::relay::Job;

pub trait Agent: Send + Sync {
    /// The final reply, or an error text for the user.
    fn run(&self, job: &Job) -> Result<String, String>;
}

/// Answers with the message itself. It proves the whole path through the game.
pub struct Echo;

impl Agent for Echo {
    fn run(&self, job: &Job) -> Result<String, String> {
        Ok(format!("echo: {}", job.text))
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
