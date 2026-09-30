//! The limits of one run of an agent (SPEC.md 9.3, 9.4): the deadline, Stop from the
//! game, and the questions that wait for the game.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::agent::{Choice, Control, Event, Events, Question, StopReason, StopSignal};
use crate::desktop::Notice;
use crate::process::{AgentProcess, Next};

pub const STOPPED: &str = "Stopped.";
pub const TIMED_OUT: &str = "Timed out.";
/// How often a wait for the agent checks the stop signal.
const POLL: Duration = Duration::from_millis(100);
/// After Stop, the agent gets this long to end the turn. Then it is killed.
const STOP_GRACE: Duration = Duration::from_secs(10);
/// After the timeout, the agent gets this long to end the turn and report its cost.
/// The run already took the whole timeout, so this grace is shorter.
const TIMEOUT_GRACE: Duration = Duration::from_secs(5);

/// Why the agent was asked to end the turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    No,
    Stop,
    Timeout,
}

pub struct Turn {
    deadline: Instant,
    stop: StopSignal,
    events: Events,
    permission_timeout: Duration,
    ending: Ending,
}

impl Turn {
    pub fn new(timeout: Duration, permission_timeout: Duration, control: Control) -> Turn {
        Turn {
            deadline: Instant::now() + timeout,
            stop: control.stop,
            events: control.events,
            permission_timeout,
            ending: Ending::No,
        }
    }

    /// True once Stop or the timeout came and the agent was asked to end the turn.
    pub fn stopping(&self) -> bool {
        self.ending != Ending::No
    }

    pub fn listening(&self) -> bool {
        self.events.listening()
    }

    pub fn progress(&self, line: String) {
        self.events.send(Event::Progress(line));
    }

    /// The output of a command of the agent, for the test line (SPEC.md 9.11).
    pub fn output(&self, text: String) {
        self.events.send(Event::CommandOutput(text));
    }

    /// The next message of the agent. At the first Stop or at the timeout, `interrupt`
    /// asks the agent to end the turn. The agent then sends its last message, with the
    /// cost of the turn (SPEC.md 9.10). An error from `interrupt` ends the run at once.
    pub fn receive(
        &mut self,
        agent: &mut AgentProcess,
        interrupt: impl FnOnce(&mut AgentProcess) -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut interrupt = Some(interrupt);
        loop {
            if self.stop.requested() && self.ending == Ending::No {
                self.ending = Ending::Stop;
                if let Some(interrupt) = interrupt.take() {
                    interrupt(agent)?;
                }
                self.deadline = self.deadline.min(Instant::now() + STOP_GRACE);
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() && self.ending == Ending::No {
                self.ending = Ending::Timeout;
                if let Some(interrupt) = interrupt.take() {
                    interrupt(agent).map_err(|_| TIMED_OUT.to_owned())?;
                }
                self.deadline = Instant::now() + TIMEOUT_GRACE;
                continue;
            }
            if left.is_zero() {
                return Err(self.ended().into());
            }
            match agent.next(left.min(POLL)) {
                Next::Line(line) => return line,
                Next::Quiet => {}
                Next::Ended if self.stopping() => return Err(self.ended().into()),
                Next::Ended => return Err(agent.stopped()),
            }
        }
    }

    /// The error of a turn that ended after Stop or the timeout.
    pub fn ended(&self) -> &'static str {
        match self.ending {
            Ending::Timeout => TIMED_OUT,
            Ending::No | Ending::Stop => STOPPED,
        }
    }

    /// Tells the bridge where a desktop request of the run stands.
    pub fn desktop(&self, notice: Notice) {
        self.events.send(Event::Desktop(notice));
    }

    /// Shows a question in the game and waits for the answer. `Answer::None` means no
    /// answer: nobody listens, Stop came, or `permission_timeout` passed. The run
    /// timeout stops while the question waits (SPEC.md 9.3). Once `covered` is true, an
    /// "Always allow" of another popup answered this one, and the question goes.
    pub fn ask_game(
        &mut self,
        text: Vec<u8>,
        choices: Vec<Choice>,
        covered: &dyn Fn() -> bool,
    ) -> Answer {
        let (answer, answers) = channel();
        let shown = self.events.send(Event::Question(Question {
            text,
            choices,
            answer,
        }));
        if !shown {
            return Answer::None;
        }
        let answer = self.wait(|| match answers.recv_timeout(POLL) {
            Ok(Some(chosen)) => Some(Answer::Game(chosen)),
            Err(RecvTimeoutError::Timeout) => covered().then_some(Answer::Covered),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => Some(Answer::None),
        });
        if answer == Answer::Covered {
            self.events.send(Event::Withdrawn);
        }
        answer
    }

    /// Waits for `desktop`, which gives `Some` once the desktop answers. The game
    /// cannot answer, so the wait goes on when nobody in the game listens.
    pub fn wait_desktop(&mut self, desktop: &dyn Fn() -> Option<bool>) -> Answer {
        self.wait(|| {
            let answer = desktop().map(Answer::Desktop);
            if answer.is_none() {
                std::thread::sleep(POLL);
            }
            answer
        })
    }

    /// Calls `check` about every `POLL` until it answers, Stop comes, or the
    /// permission timeout passes. The run timeout stops meanwhile.
    fn wait(&mut self, mut check: impl FnMut() -> Option<Answer>) -> Answer {
        let asked = Instant::now();
        let answer = loop {
            if self.stop.reason() == Some(StopReason::NewMessage) {
                break Answer::NewMessage;
            }
            if self.stop.requested() || asked.elapsed() >= self.permission_timeout {
                break Answer::None;
            }
            if let Some(answer) = check() {
                break answer;
            }
        };
        self.deadline += asked.elapsed();
        answer
    }
}

/// What answered a question.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// The index of the choice in the game.
    Game(usize),
    /// True for an approval on the desktop.
    Desktop(bool),
    /// Nobody answered in time, Stop came, or the game cancelled.
    None,
    /// The player sent a new message, which ends the wait (SPEC.md 9.3).
    NewMessage,
    /// A rule that another popup added now covers the call (SPEC.md 6.6.5).
    Covered,
}
