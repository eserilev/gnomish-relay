//! The Agent Client Protocol backend (SPEC.md 9), protocol version 1. JSON-RPC 2.0,
//! one message per line, over the stdin and stdout of the agent process.
//!
//! The agent is untrusted: every line has a size limit, the whole run has a deadline,
//! and the process gets only the environment variables of the allowlist.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use protocol::live::OptionKind;
use protocol::popup::popup_text;

use crate::agent::{Agent, Choice, Control, Event, Events, Question, Run, StopSignal};
use crate::config::Permission;
use crate::program::find_program;
use crate::relay::Job;

const PROTOCOL_VERSION: u64 = 1;
/// A tool call with a large diff fits in far less.
const MAX_LINE: usize = 8 * 1024 * 1024;
/// The slot body cuts a reply at 32 KiB anyway.
const MAX_REPLY: usize = 256 * 1024;
const STDERR_TAIL: usize = 2048;
/// After a crash, stdout can close before stderr is read to the end.
const STDERR_WAIT: Duration = Duration::from_millis(500);
/// Each agent process gets these, plus the ones in its `env` list (SPEC.md 6.2, rule 12).
const BASE_ENV: [&str; 11] = [
    "PATH",
    "HOME",
    "LANG",
    "TERM",
    "USER",
    "TMPDIR",
    "SYSTEMROOT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
];
const METHOD_NOT_FOUND: i64 = -32601;
/// How often a wait for the agent checks the stop signal.
const POLL: Duration = Duration::from_millis(100);
/// After `session/cancel`, the agent gets this long to end the turn. Then it is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(10);
const STOPPED: &str = "Stopped.";
const NEW_SESSION: &str = "(New session: the agent could not resume the old one.)";
/// A progress line or a refused tool call is at most this long.
const MAX_STEP: usize = 200;

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
        let reply = self.run_in_session(job, control, &mut session);
        Run { reply, session }
    }
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

/// What `check-agent` prints about a new agent entry.
pub struct Report {
    pub name: String,
    pub version: String,
    pub load_session: bool,
    pub modes: Vec<String>,
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

type Line = Result<Value, String>;

struct Connection {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Line>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: Receiver<()>,
    next_id: u64,
    deadline: Instant,
    reply: String,
    refused: Vec<String>,
    permission: Permission,
    stop: StopSignal,
    events: Events,
    permission_timeout: Duration,
    /// The session that `session/cancel` names.
    session: Option<String>,
    cancel_sent: bool,
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Connection {
    fn start(agent: &AcpAgent, cwd: &str, control: Control) -> Result<Connection, String> {
        let (program, args) = agent
            .command
            .split_first()
            .ok_or("The agent has no command.")?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let found = find_program(program, &path, cfg!(windows))
            .ok_or_else(|| format!("Cannot start {program}: not found on PATH"))?;
        let mut command = Command::new(found);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in BASE_ENV
            .iter()
            .copied()
            .chain(agent.env.iter().map(String::as_str))
        {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        // Tells a hook of the agent that this run comes from the bridge (SPEC.md 10).
        command.env("GNOMISH_RELAY_JOB", "1");
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start {program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("The agent has no stdin.")?;
        let stdout = child.stdout.take().ok_or("The agent has no stdout.")?;
        let stderr_pipe = child.stderr.take().ok_or("The agent has no stderr.")?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (done, stderr_done) = channel();
        keep_tail(stderr_pipe, Arc::clone(&stderr), done);
        Ok(Connection {
            child,
            stdin,
            lines: read_lines(stdout),
            stderr,
            stderr_done,
            next_id: 1,
            deadline: Instant::now() + agent.timeout,
            reply: String::new(),
            refused: Vec::new(),
            permission: Permission::Ask,
            stop: control.stop,
            events: control.events,
            permission_timeout: agent.permission_timeout,
            session: None,
            cancel_sent: false,
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
        if self.cancel_sent {
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
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| self.stopped())
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

    fn receive(&mut self) -> Result<Value, String> {
        loop {
            if self.stop.requested() && !self.cancel_sent {
                self.cancel()?;
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(if self.cancel_sent {
                    STOPPED
                } else {
                    "Timed out."
                }
                .into());
            }
            match self.lines.recv_timeout(left.min(POLL)) {
                Ok(line) => return line,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) if self.cancel_sent => {
                    return Err(STOPPED.into());
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.stopped()),
            }
        }
    }

    /// Asks the agent to end the turn. With no session yet, there is nothing to end.
    fn cancel(&mut self) -> Result<(), String> {
        self.cancel_sent = true;
        let Some(session) = self.session.clone() else {
            return Err(STOPPED.into());
        };
        self.deadline = self.deadline.min(Instant::now() + CANCEL_GRACE);
        self.send(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": session } }))
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
        match read_update(params) {
            Update::Chunk(text) => {
                let room = MAX_REPLY.saturating_sub(self.reply.len());
                self.reply.push_str(cut(&text, room));
            }
            Update::Step(line) => {
                self.events.send(Event::Progress(line));
            }
            Update::Other => {}
        }
    }

    fn answer(&mut self, params: &Value) -> Value {
        // ACP says: after `session/cancel`, every open request gets "cancelled".
        if self.cancel_sent {
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
        if !self.events.listening() {
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
        let text = request.text;
        let (answer, answers) = channel();
        if !self.events.send(Event::Question(Question {
            text,
            choices,
            answer,
        })) {
            return cancelled();
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
            .and_then(|i| request.options.get(i))
            .map(|(id, _, _)| id.clone())
            .map_or_else(
                cancelled,
                |id| json!({ "outcome": "selected", "optionId": id }),
            )
    }

    fn stopped(&mut self) -> String {
        let _ = self.stderr_done.recv_timeout(STDERR_WAIT);
        let tail = self.stderr.lock().map(|t| t.clone()).unwrap_or_default();
        let tail = String::from_utf8_lossy(&tail);
        match tail.lines().rev().find(|l| !l.trim().is_empty()) {
            Some(last) => format!("The agent stopped: {}", cut(last.trim(), 300)),
            None => "The agent stopped.".into(),
        }
    }
}

/// The longest start of `text` with at most `max` bytes that ends on a character.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Sends each line as JSON. A line over the limit or a line that is not JSON ends the stream.
fn read_lines(stdout: impl Read + Send + 'static) -> Receiver<Line> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut buf = Vec::new();
            let read = reader
                .by_ref()
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut buf);
            let line = match read {
                Ok(0) | Err(_) => return,
                Ok(_) if buf.len() > MAX_LINE => {
                    Err("The agent sent a message over the size limit.".to_owned())
                }
                Ok(_) if buf.iter().all(u8::is_ascii_whitespace) => continue,
                Ok(_) => serde_json::from_slice(&buf)
                    .map_err(|_| "The agent sent a line that is not JSON.".to_owned()),
            };
            let end = line.is_err();
            if tx.send(line).is_err() || end {
                return;
            }
        }
    });
    rx
}

/// Keeps the last bytes of stderr for an error message, and drains the rest, so a
/// chatty agent never blocks on a full pipe.
fn keep_tail(stderr: impl Read + Send + 'static, tail: Arc<Mutex<Vec<u8>>>, done: Sender<()>) {
    thread::spawn(move || {
        // The sender drops when the thread ends, and that wakes `stopped`.
        let _done = done;
        let mut stderr = stderr;
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                return;
            }
            let Ok(mut tail) = tail.lock() else { return };
            tail.extend_from_slice(&chunk[..n]);
            let extra = tail.len().saturating_sub(STDERR_TAIL);
            tail.drain(..extra);
        }
    });
}
