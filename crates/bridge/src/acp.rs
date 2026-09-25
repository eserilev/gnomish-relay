//! The Agent Client Protocol backend (SPEC.md 9), protocol version 1. JSON-RPC 2.0,
//! one message per line, over the stdin and stdout of the agent process.
//!
//! The process limits of `process.rs` and `turn.rs` apply to the agent.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};

use protocol::live::OptionKind;
use protocol::popup::popup_text;

use crate::agent::{
    Agent, Choice, Control, MAX_PROMPT, MAX_REPLY, MAX_STEP, NEW_SESSION, Report, Run, SessionInfo,
    exchange_text,
};
use crate::config::Permission;
use crate::process::{AgentProcess, cut};
use crate::relay::{Job, Work};
use crate::turn::{STOPPED, Turn};

const PROTOCOL_VERSION: u64 = 1;
const METHOD_NOT_FOUND: i64 = -32601;

pub struct AcpAgent {
    pub command: Vec<String>,
    pub env: Vec<String>,
    /// The session mode of the agent for each level, from the `modes` table of the config.
    pub modes: BTreeMap<Permission, String>,
    pub timeout: Duration,
    /// How long a question waits for the game. The run timeout stops meanwhile.
    pub permission_timeout: Duration,
}

impl Agent for AcpAgent {
    fn run(&self, job: &Job, control: &Control) -> Run {
        let mut session = None;
        let reply = match &job.work {
            Work::Attach { session: id, fork } => {
                self.attach(job, control, id, *fork, &mut session)
            }
            Work::Prompt | Work::ListSessions => self.run_in_session(job, control, &mut session),
        };
        Run { reply, session }
    }

    fn sessions(&self, cwd: &str) -> Result<Vec<SessionInfo>, String> {
        let mut agent = Connection::start(self, cwd, Control::default())?;
        let init = agent.initialize()?;
        if !offers(&init, "/agentCapabilities/sessionCapabilities/list") {
            return Ok(Vec::new());
        }
        let result = agent.request("session/list", &json!({}))?;
        Ok(read_sessions(&result))
    }
}

fn offers(init: &Value, pointer: &str) -> bool {
    init.pointer(pointer).is_some_and(|v| !v.is_null())
}

/// The first page of `session/list`. Sessions with no id or no folder are left out.
pub fn read_sessions(result: &Value) -> Vec<SessionInfo> {
    let listed = result.get("sessions").and_then(Value::as_array);
    listed
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some(SessionInfo {
                id: text_at(s, "/sessionId")?.to_owned(),
                cwd: text_at(s, "/cwd")?.to_owned(),
                title: text_at(s, "/title").unwrap_or("").to_owned(),
                updated: text_at(s, "/updatedAt").and_then(unix_time).unwrap_or(0),
            })
        })
        .collect()
}

/// Seconds since 1970 of an ISO 8601 time in UTC, such as `2026-09-25T06:46:21.432Z`.
pub fn unix_time(iso: &str) -> Option<u32> {
    let number = |range: std::ops::Range<usize>| iso.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from civil, by Howard Hinnant: March starts the year, so February is last.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u32::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

impl AcpAgent {
    fn run_in_session(
        &self,
        job: &Job,
        control: &Control,
        session_id: &mut Option<String>,
    ) -> Result<String, String> {
        let mut agent = Connection::start(self, &job.cwd, control.clone())?;
        let init = agent.initialize()?;
        let (session, note) = agent.open_session(&init, &job.cwd, job.resume.as_deref())?;
        *session_id = Some(session.id.clone());
        if let Some(mode) = self.modes.get(&job.permission) {
            agent.set_mode(&session.id, mode, &session.modes)?;
        }
        let reply = agent.prompt(&session.id, &job.text, job.permission)?;
        Ok(match note {
            Some(note) => format!("{note}\n\n{reply}"),
            None => reply,
        })
    }
}

impl AcpAgent {
    /// Opens a saved session for a new chat, or a fork of it, and returns its last
    /// exchange: the prompt on the first line, the answer below.
    fn attach(
        &self,
        job: &Job,
        control: &Control,
        session: &str,
        fork: bool,
        session_id: &mut Option<String>,
    ) -> Result<String, String> {
        let mut agent = Connection::start(self, &job.cwd, control.clone())?;
        let init = agent.initialize()?;
        let params = |id: &str| json!({ "sessionId": id, "cwd": job.cwd, "mcpServers": [] });
        let id = if fork && offers(&init, "/agentCapabilities/sessionCapabilities/fork") {
            let result = agent.request("session/fork", &params(session))?;
            text_at(&result, "/sessionId")
                .ok_or("The agent made no copy of the session.")?
                .to_owned()
        } else {
            session.to_owned()
        };
        *session_id = Some(id.clone());
        if init
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Ok(String::new());
        }
        agent.session = Some(id.clone());
        agent.replay = Some(Replay::default());
        agent.request("session/load", &params(&id))?;
        Ok(agent.replay.take().map(|r| r.text()).unwrap_or_default())
    }
}

/// The last exchange of a replayed session.
#[derive(Default)]
struct Replay {
    prompt: String,
    answer: String,
    answering: bool,
}

impl Replay {
    fn add(&mut self, update: Update) {
        match update {
            Update::UserChunk(text) => {
                if self.answering {
                    self.prompt.clear();
                    self.answer.clear();
                    self.answering = false;
                }
                let room = MAX_PROMPT.saturating_sub(self.prompt.len());
                self.prompt.push_str(cut(&text, room));
            }
            Update::Chunk(text) => {
                self.answering = true;
                let room = MAX_REPLY.saturating_sub(self.answer.len());
                self.answer.push_str(cut(&text, room));
            }
            Update::Step(_) | Update::Other => {}
        }
    }

    fn text(&self) -> String {
        exchange_text(&self.prompt, &self.answer)
    }
}

impl AcpAgent {
    /// Starts the agent, opens one session in `cwd`, and stops it. No prompt is sent.
    pub fn check(&self, cwd: &str) -> Result<Report, String> {
        let mut agent = Connection::start(self, cwd, Control::default())?;
        let init = agent.initialize()?;
        let session = agent.new_session(cwd)?;
        Ok(Report {
            name: text_at(&init, "/agentInfo/name").unwrap_or("?").to_owned(),
            version: text_at(&init, "/agentInfo/version")
                .unwrap_or("?")
                .to_owned(),
            load_session: init
                .pointer("/agentCapabilities/loadSession")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            modes: session.modes,
        })
    }
}

/// One `session/update` from the agent, as the bridge uses it.
#[derive(Debug, PartialEq, Eq)]
pub enum Update {
    /// Text of the reply.
    Chunk(String),
    /// Text of the user, which comes only in the replay of `session/load`.
    UserChunk(String),
    /// A tool call, as one progress line of at most 200 bytes.
    Step(String),
    Other,
}

pub fn read_update(params: &Value) -> Update {
    let update = params.get("update").unwrap_or(&Value::Null);
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some("agent_message_chunk") => match text_at(update, "/content/text") {
            Some(text) => Update::Chunk(text.to_owned()),
            None => Update::Other,
        },
        Some("user_message_chunk") => match text_at(update, "/content/text") {
            Some(text) => Update::UserChunk(text.to_owned()),
            None => Update::Other,
        },
        Some("tool_call") => {
            let title = text_at(update, "/title").unwrap_or("a tool call");
            Update::Step(cut(title, MAX_STEP).to_owned())
        }
        _ => Update::Other,
    }
}

/// A permission request as the game sees it.
pub struct Request {
    /// The output of `popup_text` (S15).
    pub text: Vec<u8>,
    /// The option id of the agent, the kind, and the label of the agent.
    pub options: Vec<(Value, OptionKind, String)>,
}

/// Leaves out "allow always", which waits for the rules of SPEC.md 6.6.5, every
/// option with no id or no known kind, and every option after the fourth.
pub fn read_request(params: &Value) -> Request {
    let offered = params.get("options").and_then(Value::as_array);
    let options = offered
        .into_iter()
        .flatten()
        .filter_map(|o| Some((o.get("optionId")?.clone(), option_kind(o)?, o)))
        .filter(|(_, kind, _)| !matches!(kind, OptionKind::AllowAlways))
        .take(protocol::live::MAX_OPTIONS)
        .map(|(id, kind, o)| (id, kind, text_at(o, "/name").unwrap_or("?").to_owned()))
        .collect();
    let title = text_at(params, "/toolCall/title").unwrap_or("");
    Request {
        text: popup_text(command_of(params).as_bytes(), title.as_bytes()),
        options,
    }
}

fn cancelled() -> Value {
    json!({ "outcome": "cancelled" })
}

fn select(options: &[Value], kind: &str) -> Option<Value> {
    let option = options.iter().find(|o| text_at(o, "/kind") == Some(kind))?;
    let id = option.get("optionId")?.clone();
    Some(json!({ "outcome": "selected", "optionId": id }))
}

fn option_kind(option: &Value) -> Option<OptionKind> {
    match text_at(option, "/kind")? {
        "allow_once" => Some(OptionKind::AllowOnce),
        "allow_always" => Some(OptionKind::AllowAlways),
        "reject_once" => Some(OptionKind::RejectOnce),
        "reject_always" => Some(OptionKind::RejectAlways),
        _ => None,
    }
}

/// The raw text that the popup shows first (SPEC.md 6.4): the command line, else
/// the path or the address, else the title of the tool call.
fn command_of(params: &Value) -> String {
    let input = params.pointer("/toolCall/rawInput").unwrap_or(&Value::Null);
    let command = match input.get("command") {
        Some(Value::String(line)) => Some(line.clone()),
        Some(Value::Array(words)) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            Some(words.join(" "))
        }
        _ => None,
    };
    let path = ["file_path", "path", "url"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str).map(str::to_owned));
    command
        .or(path)
        .unwrap_or_else(|| text_at(params, "/toolCall/title").unwrap_or("").to_owned())
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

struct Session {
    id: String,
    modes: Vec<String>,
}

fn modes_of(result: &Value) -> Vec<String> {
    result
        .pointer("/modes/availableModes")
        .and_then(Value::as_array)
        .map(|modes| {
            modes
                .iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

struct Connection {
    process: AgentProcess,
    turn: Turn,
    next_id: u64,
    reply: String,
    refused: Vec<String>,
    permission: Permission,
    /// The session that `session/cancel` names.
    session: Option<String>,
    /// Set while `session/load` replays a session for an attach.
    replay: Option<Replay>,
}

impl Connection {
    fn start(agent: &AcpAgent, cwd: &str, control: Control) -> Result<Connection, String> {
        Ok(Connection {
            process: AgentProcess::start(&agent.command, &[], &agent.env, cwd)?,
            turn: Turn::new(agent.timeout, agent.permission_timeout, control),
            next_id: 1,
            reply: String::new(),
            refused: Vec::new(),
            permission: Permission::Ask,
            session: None,
            replay: None,
        })
    }

    fn initialize(&mut self) -> Result<Value, String> {
        let result = self.request(
            "initialize",
            &json!({
                "protocolVersion": PROTOCOL_VERSION,
                // The agent uses its own tools in v1 (SPEC.md 9.4).
                "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
                "clientInfo": { "name": "gnomish-relay", "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        let version = result.get("protocolVersion").and_then(Value::as_u64);
        if version != Some(PROTOCOL_VERSION) {
            return Err(format!(
                "The agent speaks ACP version {}, not {PROTOCOL_VERSION}.",
                version.map_or("?".to_owned(), |v| v.to_string())
            ));
        }
        Ok(result)
    }

    fn new_session(&mut self, cwd: &str) -> Result<Session, String> {
        let result = self.request("session/new", &json!({ "cwd": cwd, "mcpServers": [] }))?;
        let id = text_at(&result, "/sessionId")
            .ok_or("The agent opened no session.")?
            .to_owned();
        self.session = Some(id.clone());
        Ok(Session {
            id,
            modes: modes_of(&result),
        })
    }

    /// Resumes the session of the chat if the agent can, and says so if it cannot.
    /// `session/resume` needs no replay. `session/load` replays the chat as updates,
    /// and `prompt` drops them: they are history, not the reply.
    fn open_session(
        &mut self,
        init: &Value,
        cwd: &str,
        resume: Option<&str>,
    ) -> Result<(Session, Option<&'static str>), String> {
        let Some(id) = resume else {
            return Ok((self.new_session(cwd)?, None));
        };
        let caps = init.get("agentCapabilities").unwrap_or(&Value::Null);
        let method = if caps
            .pointer("/sessionCapabilities/resume")
            .is_some_and(|r| !r.is_null())
        {
            "session/resume"
        } else if caps.get("loadSession").and_then(Value::as_bool) == Some(true) {
            "session/load"
        } else {
            return Ok((self.new_session(cwd)?, Some(NEW_SESSION)));
        };
        match self.request(
            method,
            &json!({ "sessionId": id, "cwd": cwd, "mcpServers": [] }),
        ) {
            Ok(result) => {
                self.session = Some(id.to_owned());
                let session = Session {
                    id: id.to_owned(),
                    modes: modes_of(&result),
                };
                Ok((session, None))
            }
            Err(e) if e == STOPPED => Err(e),
            Err(_) => Ok((self.new_session(cwd)?, Some(NEW_SESSION))),
        }
    }

    /// A mode that the agent does not offer stops the run: with no mode, the agent
    /// runs at its own default, which can be more open than the config.
    fn set_mode(&mut self, session: &str, mode: &str, offered: &[String]) -> Result<(), String> {
        if !offered.iter().any(|m| m == mode) {
            return Err(format!(
                "The agent has no mode {mode}. Check `modes` in the config."
            ));
        }
        self.request(
            "session/set_mode",
            &json!({ "sessionId": session, "modeId": mode }),
        )?;
        Ok(())
    }

    fn prompt(
        &mut self,
        session: &str,
        text: &str,
        permission: Permission,
    ) -> Result<String, String> {
        self.permission = permission;
        self.reply.clear();
        self.refused.clear();
        let result = self.request(
            "session/prompt",
            &json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] }),
        )?;
        if self.turn.stopping() {
            return Err(STOPPED.into());
        }
        let reply = std::mem::take(&mut self.reply);
        let reply = match text_at(&result, "/stopReason") {
            Some("end_turn") => reply,
            Some("cancelled") => return Err(STOPPED.into()),
            Some("refusal") => return Err("The agent refused.".into()),
            Some(other) => format!("{reply}\n\n(The agent stopped: {other}.)"),
            None => return Err("The agent gave no stop reason.".into()),
        };
        Ok(self.with_refusals(reply))
    }

    fn with_refusals(&self, reply: String) -> String {
        if self.refused.is_empty() {
            return reply;
        }
        format!(
            "{reply}\n\nNot allowed from the game: {}",
            self.refused.join("; ")
        )
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        self.process.send(message)
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        loop {
            let message = self.receive()?;
            if message.get("method").is_some() {
                self.handle(&message)?;
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("The agent failed at {method}: {text}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// At Stop, the agent gets `session/cancel`. With no session yet, there is nothing
    /// to cancel.
    fn receive(&mut self) -> Result<Value, String> {
        let session = self.session.clone();
        self.turn.receive(&mut self.process, |agent| {
            let Some(session) = session else {
                return Err(STOPPED.into());
            };
            agent.send(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": session } }))
        })
    }

    /// A request or a notification from the agent.
    fn handle(&mut self, message: &Value) -> Result<(), String> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").unwrap_or(&Value::Null);
        if method == "session/update" {
            self.update(params);
        }
        let Some(id) = message.get("id").cloned() else {
            return Ok(());
        };
        let answer = if method == "session/request_permission" {
            json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": self.answer(params) } })
        } else {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": METHOD_NOT_FOUND, "message": "not supported" } })
        };
        self.send(&answer)
    }

    fn update(&mut self, params: &Value) {
        if let Some(replay) = &mut self.replay {
            replay.add(read_update(params));
            return;
        }
        match read_update(params) {
            Update::Chunk(text) => {
                let room = MAX_REPLY.saturating_sub(self.reply.len());
                self.reply.push_str(cut(&text, room));
            }
            Update::Step(line) => self.turn.progress(line),
            Update::UserChunk(_) | Update::Other => {}
        }
    }

    fn answer(&mut self, params: &Value) -> Value {
        // ACP says: after `session/cancel`, every open request gets "cancelled".
        if self.turn.stopping() {
            return cancelled();
        }
        let offered = params
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if self.permission == Permission::FullAuto {
            return select(&offered, "allow_once").unwrap_or_else(cancelled);
        }
        if !self.turn.listening() {
            let title = text_at(params, "/toolCall/title").unwrap_or("a tool call");
            self.refused.push(cut(title, MAX_STEP).to_owned());
            return select(&offered, "reject_once").unwrap_or_else(cancelled);
        }
        self.ask_game(params)
    }

    /// Shows the request in the game and waits for the answer. The run timeout
    /// stops while it waits, and `permission_timeout` applies (SPEC.md 9.3).
    fn ask_game(&mut self, params: &Value) -> Value {
        let request = read_request(params);
        let choices = request
            .options
            .iter()
            .map(|(_, kind, label)| Choice {
                kind: *kind,
                label: label.clone(),
            })
            .collect();
        self.turn
            .ask_game(request.text, choices)
            .and_then(|i| request.options.get(i))
            .map(|(id, _, _)| id.clone())
            .map_or_else(
                cancelled,
                |id| json!({ "outcome": "selected", "optionId": id }),
            )
    }
}
