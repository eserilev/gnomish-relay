//! The agents that answer messages (SPEC.md 9). Real backends come with step 9.

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
