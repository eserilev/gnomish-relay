//! The agents that answer messages (SPEC.md 9).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use protocol::live::OptionKind;

use crate::acp::AcpAgent;
use crate::config::{Config, Kind};
use crate::relay::{ChatId, Job, MessageId};

/// The slot body cuts a reply at 32 KiB anyway.
pub const MAX_REPLY: usize = 256 * 1024;
/// A progress line or a refused tool call is at most this long.
pub const MAX_STEP: usize = 200;
/// The prompt of a replayed exchange, on one line.
pub const MAX_PROMPT: usize = 300;
pub const NEW_SESSION: &str = "(New session: the agent could not resume the old one.)";

/// The last exchange of a saved session for an attach (SPEC.md 9.6): the prompt on the
/// first line, the answer below. An empty session gives an empty text.
pub fn exchange_text(prompt: &str, answer: &str) -> String {
    if prompt.is_empty() && answer.is_empty() {
        return String::new();
    }
    let prompt: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{prompt}\n{answer}")
}

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

/// One answer that the game can give to a question.
pub struct Choice {
    pub kind: OptionKind,
    pub label: String,
}

/// A permission request for the game (SPEC.md 9.3). `text` is the output of
/// `popup_text` (S15). The answer is an index into `choices`, or `None` for "cancelled".
pub struct Question {
    pub text: Vec<u8>,
    pub choices: Vec<Choice>,
    pub answer: Sender<Option<usize>>,
}

pub enum Event {
    /// One step of the agent, for the activity panel.
    Progress(String),
    Question(Question),
}

/// The channel from the runs to the bridge. Each event names its chat and message.
pub type EventSender = Sender<(ChatId, MessageId, Event)>;

/// Where a run sends its events. With no bridge, nobody listens, and a backend
/// answers questions itself.
#[derive(Clone, Default)]
pub struct Events {
    to: Option<(EventSender, ChatId, MessageId)>,
}

impl Events {
    pub fn to_bridge(to: EventSender, job: &Job) -> Events {
        Events {
            to: Some((to, job.chat.clone(), job.id)),
        }
    }

    pub fn listening(&self) -> bool {
        self.to.is_some()
    }

    /// Returns false when nobody listens.
    pub fn send(&self, event: Event) -> bool {
        let Some((to, chat, id)) = &self.to else {
            return false;
        };
        to.send((chat.clone(), *id, event)).is_ok()
    }
}

/// What a run gets from the bridge besides the job.
#[derive(Clone, Default)]
pub struct Control {
    pub stop: StopSignal,
    pub events: Events,
}

/// The end of one run.
pub struct Run {
    /// The final reply, or an error text for the user.
    pub reply: Result<String, String>,
    /// The agent session, so the next message of the chat can resume it. A failed
    /// run can have one too.
    pub session: Option<String>,
}

/// One session of an agent, from `session/list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    /// The folder of the session, as the agent gives it.
    pub cwd: String,
    pub title: String,
    /// Unix seconds of the last change.
    pub updated: u32,
}

pub trait Agent: Send + Sync {
    fn run(&self, job: &Job, control: &Control) -> Run;

    /// The saved sessions of the agent. An agent that cannot list them has none.
    fn sessions(&self, _cwd: &str) -> Result<Vec<SessionInfo>, String> {
        Ok(Vec::new())
    }
}

/// Answers with the message itself. It proves the whole path through the game.
pub struct Echo;

impl Agent for Echo {
    fn run(&self, job: &Job, _control: &Control) -> Run {
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
                    permission_timeout: config.permission_timeout,
                }),
            };
            (name.clone(), agent)
        })
        .collect()
}
