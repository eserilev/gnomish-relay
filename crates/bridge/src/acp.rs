//! The Agent Client Protocol backend (SPEC.md 9), protocol version 1. JSON-RPC 2.0,
//! one message per line, over the stdin and stdout of the agent process.
//!
//! The agent is untrusted: every line has a size limit, the whole run has a deadline,
//! and the process gets only the environment variables of the allowlist.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::Agent;
use crate::config::Permission;
use crate::relay::Job;

const PROTOCOL_VERSION: u64 = 1;
/// A tool call with a large diff fits in far less.
const MAX_LINE: usize = 8 * 1024 * 1024;
/// The slot body cuts a reply at 32 KiB anyway.
const MAX_REPLY: usize = 256 * 1024;
const STDERR_TAIL: usize = 2048;
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

pub struct AcpAgent {
    pub command: Vec<String>,
    pub env: Vec<String>,
    /// The session mode of the agent for each level, from the `modes` table of the config.
    pub modes: BTreeMap<Permission, String>,
    pub timeout: Duration,
}

impl Agent for AcpAgent {
    fn run(&self, job: &Job) -> Result<String, String> {
        let mut agent = Connection::start(self, &job.cwd)?;
        agent.initialize()?;
        let session = agent.new_session(&job.cwd)?;
        if let Some(mode) = self.modes.get(&job.permission) {
            agent.set_mode(&session.id, mode, &session.modes)?;
        }
        agent.prompt(&session.id, &job.text, job.permission)
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
        let mut agent = Connection::start(self, cwd)?;
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

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

struct Session {
    id: String,
    modes: Vec<String>,
}

type Line = Result<Value, String>;

struct Connection {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Line>,
    stderr: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
    deadline: Instant,
    reply: String,
    refused: Vec<String>,
    permission: Permission,
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Connection {
    fn start(agent: &AcpAgent, cwd: &str) -> Result<Connection, String> {
        let (program, args) = agent
            .command
            .split_first()
            .ok_or("The agent has no command.")?;
        let mut command = Command::new(program);
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
        keep_tail(stderr_pipe, Arc::clone(&stderr));
        Ok(Connection {
            child,
            stdin,
            lines: read_lines(stdout),
            stderr,
            next_id: 1,
            deadline: Instant::now() + agent.timeout,
            reply: String::new(),
            refused: Vec::new(),
            permission: Permission::Ask,
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
        let modes = result
            .pointer("/modes/availableModes")
            .and_then(Value::as_array)
            .map(|modes| {
                modes
                    .iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Session { id, modes })
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
        let result = self.request(
            "session/prompt",
            &json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] }),
        )?;
        let reply = std::mem::take(&mut self.reply);
        let reply = match text_at(&result, "/stopReason") {
            Some("end_turn") => reply,
            Some("cancelled") => return Err("Stopped.".into()),
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
        let left = self.deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(left) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => Err("Timed out.".into()),
            Err(RecvTimeoutError::Disconnected) => Err(self.stopped()),
        }
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
        let kind = params
            .pointer("/update/sessionUpdate")
            .and_then(Value::as_str);
        if kind != Some("agent_message_chunk") {
            return;
        }
        if let Some(text) = text_at(params, "/update/content/text") {
            let room = MAX_REPLY.saturating_sub(self.reply.len());
            self.reply.push_str(cut(text, room));
        }
    }

    /// The game cannot answer yet (SPEC.md 9.3), so the bridge answers under the
    /// ceiling: `full-auto` allows once, every other level refuses once.
    fn answer(&mut self, params: &Value) -> Value {
        let want = if self.permission == Permission::FullAuto {
            "allow_once"
        } else {
            "reject_once"
        };
        let options = params.get("options").and_then(Value::as_array);
        let chosen = options
            .into_iter()
            .flatten()
            .find(|o| o.get("kind").and_then(Value::as_str) == Some(want))
            .and_then(|o| o.get("optionId").cloned());
        if want == "reject_once" {
            let title = text_at(params, "/toolCall/title").unwrap_or("a tool call");
            self.refused.push(cut(title, 200).to_owned());
        }
        match chosen {
            Some(option) => json!({ "outcome": "selected", "optionId": option }),
            None => json!({ "outcome": "cancelled" }),
        }
    }

    fn stopped(&mut self) -> String {
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
fn keep_tail(stderr: impl Read + Send + 'static, tail: Arc<Mutex<Vec<u8>>>) {
    thread::spawn(move || {
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
