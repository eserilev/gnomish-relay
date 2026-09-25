//! The limits of one run of an agent (SPEC.md 9.3, 9.4): the deadline, Stop from the
//! game, and the questions that wait for the game.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::agent::{Choice, Control, Event, Events, Question, StopSignal};
use crate::process::{AgentProcess, Next};

pub const STOPPED: &str = "Stopped.";
pub const TIMED_OUT: &str = "Timed out.";
/// How often a wait for the agent checks the stop signal.
const POLL: Duration = Duration::from_millis(100);
/// After Stop, the agent gets this long to end the turn. Then it is killed.
const STOP_GRACE: Duration = Duration::from_secs(10);

pub struct Turn {
    deadline: Instant,
    stop: StopSignal,
    events: Events,
    permission_timeout: Duration,
    stopping: bool,
}

impl Turn {
    pub fn new(timeout: Duration, permission_timeout: Duration, control: Control) -> Turn {
        Turn {
            deadline: Instant::now() + timeout,
            stop: control.stop,
            events: control.events,
            permission_timeout,
            stopping: false,
        }
    }

    /// True once Stop came and the agent was asked to end the turn.
    pub fn stopping(&self) -> bool {
        self.stopping
    }

    pub fn listening(&self) -> bool {
        self.events.listening()
    }

    pub fn progress(&self, line: String) {
        self.events.send(Event::Progress(line));
    }

    /// The next message of the agent. At the first Stop, `interrupt` asks the agent to
    /// end the turn. An error from `interrupt` ends the run at once.
    pub fn receive(
        &mut self,
        agent: &mut AgentProcess,
        interrupt: impl FnOnce(&mut AgentProcess) -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut interrupt = Some(interrupt);
        loop {
            if self.stop.requested() && !self.stopping {
                self.stopping = true;
                if let Some(interrupt) = interrupt.take() {
                    interrupt(agent)?;
                }
                self.deadline = self.deadline.min(Instant::now() + STOP_GRACE);
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(self.ended().into());
            }
            match agent.next(left.min(POLL)) {
                Next::Line(line) => return line,
                Next::Quiet => {}
                Next::Ended if self.stopping => return Err(STOPPED.into()),
                Next::Ended => return Err(agent.stopped()),
            }
        }
    }

    fn ended(&self) -> &'static str {
        if self.stopping { STOPPED } else { TIMED_OUT }
    }

    /// Shows a question in the game and waits for the index of the answer. `None` means
    /// no answer: nobody listens, Stop came, or `permission_timeout` passed. The run
    /// timeout stops while the question waits (SPEC.md 9.3).
    pub fn ask_game(&mut self, text: Vec<u8>, choices: Vec<Choice>) -> Option<usize> {
        let (answer, answers) = channel();
        if !self.events.send(Event::Question(Question {
            text,
            choices,
            answer,
        })) {
            return None;
        }
        let asked = Instant::now();
        let chosen = loop {
            if self.stop.requested() || asked.elapsed() >= self.permission_timeout {
                break None;
            }
            match answers.recv_timeout(POLL) {
                Ok(chosen) => break chosen,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break None,
            }
        };
        self.deadline += asked.elapsed();
        chosen
    }
}
