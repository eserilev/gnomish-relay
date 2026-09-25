//! The native Codex backend (SPEC.md 9.2): `codex app-server`, JSON-RPC 2.0 with no
//! `jsonrpc` field, one message per line. It needs no Node and no adapter.
//!
//! The process limits of `process.rs` and `turn.rs` apply to the agent.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};

use protocol::live::OptionKind;
use protocol::popup::popup_text;

use crate::agent::{
    Agent, Choice, Control, MAX_PROMPT, MAX_REPLY, MAX_STEP, NEW_SESSION, Report, Run, SessionInfo,
    exchange_text,
};
use crate::config::Permission;
use crate::process::{self, AgentProcess, cut};
use crate::relay::{Job, Work};
use crate::turn::{STOPPED, Turn};

const METHOD_NOT_FOUND: i64 = -32601;
const CHECK_TIME: Duration = Duration::from_secs(30);
/// The relay keeps 30 after its folder check.
const LIST_LIMIT: u64 = 60;

pub struct CodexAgent {
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub timeout: Duration,
    /// How long a question waits for the game. The run timeout stops meanwhile.
    pub permission_timeout: Duration,
}

/// The sandbox and the approval policy of Codex for each level (SPEC.md 9.3). Full-auto
/// still asks: the bridge accepts each question, so it sees every escalation.
pub fn policy(level: Permission) -> (&'static str, &'static str) {
    match level {
        Permission::Ask => ("read-only", "untrusted"),
        Permission::AutoEdit | Permission::FullAuto => ("workspace-write", "on-request"),
    }
}

impl Agent for CodexAgent {
    fn run(&self, job: &Job, control: &Control) -> Run {
        let mut session = None;
        let reply = match &job.work {
            Work::Attach { session: id, fork } => self.attach(job, id, *fork, &mut session),
            Work::Prompt | Work::ListSessions => self.prompt(job, control, &mut session),
        };
        Run { reply, session }
    }

    /// Threads of the terminal, the IDE, `codex exec`, and the bridge itself.
    fn sessions(&self, cwd: &str) -> Result<Vec<SessionInfo>, String> {
        let mut codex = Connection::start(self, cwd, Control::default())?;
        codex.initialize()?;
        let result = codex.request(
            "thread/list",
            &json!({ "limit": LIST_LIMIT, "sortKey": "updated_at", "sourceKinds": ["cli", "vscode", "exec", "appServer"] }),
        )?;
        Ok(read_threads(&result))
    }
}

impl CodexAgent {
    fn prompt(
        &self,
        job: &Job,
        control: &Control,
        session: &mut Option<String>,
    ) -> Result<String, String> {
        let mut codex = Connection::start(self, &job.cwd, control.clone())?;
        codex.permission = job.permission;
        codex.initialize()?;
        let (thread, note) = codex.open_thread(&job.cwd, job.resume.as_deref(), job.permission)?;
        *session = Some(thread.clone());
        let reply = codex.turn(&thread, &job.text)?;
        Ok(match note {
            Some(note) => format!("{note}\n\n{reply}"),
            None => reply,
        })
    }

    /// Reads the saved thread, and forks it first if the terminal has it open. Neither
    /// call reaches the model.
    fn attach(
        &self,
        job: &Job,
        thread: &str,
        fork: bool,
        session: &mut Option<String>,
    ) -> Result<String, String> {
        let mut codex = Connection::start(self, &job.cwd, Control::default())?;
        codex.initialize()?;
        let last = codex.request(
            "thread/turns/list",
            &json!({ "threadId": thread, "limit": 1, "itemsView": "full" }),
        )?;
        let id = if fork {
            let result = codex.request(
                "thread/fork",
                &json!({ "threadId": thread, "excludeTurns": true }),
            )?;
            text_at(&result, "/thread/id")
                .ok_or("Codex made no copy of the thread.")?
                .to_owned()
        } else {
            thread.to_owned()
        };
        *session = Some(id);
        Ok(last_exchange(&last))
    }

    /// The version and the login, with no model call.
    pub fn check(&self, cwd: &str) -> Result<Report, String> {
        let version = process::output(
            &self.command,
            &["--version".into()],
            &self.env,
            cwd,
            CHECK_TIME,
        )?;
        let login = process::output(
            &self.command,
            &["login".into(), "status".into()],
            &self.env,
            cwd,
            CHECK_TIME,
        )?;
        if !login.success {
            return Err("Codex needs a login.".into());
        }
        Ok(Report {
            name: "Codex".into(),
            version: version
                .stdout
                .split_whitespace()
                .last()
                .unwrap_or("?")
                .to_owned(),
            load_session: true,
            modes: Vec::new(),
        })
    }
}

/// The first page of `thread/list`. Threads with no id or no folder are left out.
pub fn read_threads(result: &Value) -> Vec<SessionInfo> {
    let listed = result.get("data").and_then(Value::as_array);
    listed
        .into_iter()
        .flatten()
        .filter_map(|thread| {
            let title = text_at(thread, "/name")
                .filter(|n| !n.trim().is_empty())
                .or_else(|| text_at(thread, "/preview"))
                .unwrap_or("");
            Some(SessionInfo {
                id: text_at(thread, "/id")?.to_owned(),
                cwd: text_at(thread, "/cwd")?.to_owned(),
                title: title.split_whitespace().collect::<Vec<_>>().join(" "),
                updated: thread
                    .get("updatedAt")
                    .and_then(Value::as_u64)
                    .and_then(|t| u32::try_from(t).ok())
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// The prompt and the answer of the newest turn of `thread/turns/list`.
pub fn last_exchange(result: &Value) -> String {
    let items = result.pointer("/data/0/items").and_then(Value::as_array);
    let mut prompt = String::new();
    let mut answer: Vec<&str> = Vec::new();
    for item in items.into_iter().flatten() {
        match text_at(item, "/type") {
            Some("userMessage") => {
                prompt = user_text(item);
                answer.clear();
            }
            Some("agentMessage") => answer.extend(text_at(item, "/text")),
            _ => {}
        }
    }
    let answer = answer.join("\n\n");
    exchange_text(cut(&prompt, MAX_PROMPT), cut(&answer, MAX_REPLY))
}

fn user_text(item: &Value) -> String {
    let content = item.get("content").and_then(Value::as_array);
    let texts: Vec<&str> = content
        .into_iter()
        .flatten()
        .filter(|c| text_at(c, "/type") == Some("text"))
        .filter_map(|c| text_at(c, "/text"))
        .collect();
    texts.join(" ")
}

/// A notification of the server, as the bridge uses it.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A command or a file change starts: one progress line, and the paths of a change
    /// for a later approval question.
    Started {
        item: String,
        step: String,
        paths: Vec<String>,
    },
    /// The final text of one agent message.
    Said(String),
    /// The end of the turn: the reply, or an error text.
    Ended(Result<(), String>),
    Other,
}

pub fn read_event(method: &str, params: &Value) -> Event {
    match method {
        "item/started" => read_started(params.get("item").unwrap_or(&Value::Null)),
        "item/completed" if text_at(params, "/item/type") == Some("agentMessage") => {
            Event::Said(text_at(params, "/item/text").unwrap_or("").to_owned())
        }
        "turn/completed" => Event::Ended(read_ending(params.get("turn").unwrap_or(&Value::Null))),
        _ => Event::Other,
    }
}

fn read_started(item: &Value) -> Event {
    let id = text_at(item, "/id").unwrap_or("").to_owned();
    match text_at(item, "/type") {
        Some("commandExecution") => Event::Started {
            item: id,
            step: step(&format!("$ {}", text_at(item, "/command").unwrap_or(""))),
            paths: Vec::new(),
        },
        Some("fileChange") => {
            let changes = item.get("changes").and_then(Value::as_array);
            let paths: Vec<String> = changes
                .into_iter()
                .flatten()
                .filter_map(|c| text_at(c, "/path").map(str::to_owned))
                .collect();
            Event::Started {
                item: id,
                step: step(&format!("edit {}", paths.join(", "))),
                paths,
            }
        }
        Some("mcpToolCall") => Event::Started {
            item: id,
            step: step(&format!(
                "{} {}",
                text_at(item, "/server").unwrap_or("mcp"),
                text_at(item, "/tool").unwrap_or("")
            )),
            paths: Vec::new(),
        },
        Some("webSearch") => Event::Started {
            item: id,
            step: step(&format!("search {}", text_at(item, "/query").unwrap_or(""))),
            paths: Vec::new(),
        },
        _ => Event::Other,
    }
}

fn step(line: &str) -> String {
    let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    cut(&line, MAX_STEP).to_owned()
}

fn read_ending(turn: &Value) -> Result<(), String> {
    match text_at(turn, "/status") {
        Some("completed") => Ok(()),
        Some("interrupted") => Err(STOPPED.into()),
        _ => {
            let message = text_at(turn, "/error/message").unwrap_or("the turn failed");
            Err(format!("The agent stopped: {}", cut(message, 300)))
        }
    }
}

/// An approval question as the game sees it.
#[derive(Debug, PartialEq)]
pub struct Request {
    /// The output of `popup_text` (S15).
    pub text: Vec<u8>,
    /// The words of the agent, for "Not allowed from the game:".
    pub title: String,
}

/// The popup shows the raw command, or the paths of the change, first (SPEC.md 6.4).
/// A change that asks for a whole folder shows that folder.
pub fn read_request(method: &str, params: &Value, paths: &[String]) -> Request {
    let reason = text_at(params, "/reason").unwrap_or("");
    let (raw, default_title) = match method {
        "item/commandExecution/requestApproval" => (
            text_at(params, "/command").unwrap_or("").to_owned(),
            "run a command",
        ),
        _ => match text_at(params, "/grantRoot") {
            Some(root) => (format!("write anything in {root}"), "change files"),
            None => (paths.join(", "), "change files"),
        },
    };
    let title = if reason.is_empty() {
        default_title
    } else {
        reason
    };
    Request {
        text: popup_text(raw.as_bytes(), title.as_bytes()),
        title: cut(title, MAX_STEP).to_owned(),
    }
}

fn is_approval(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
    )
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn choices() -> Vec<Choice> {
    vec![
        Choice {
            kind: OptionKind::AllowOnce,
            label: "Allow".into(),
        },
        Choice {
            kind: OptionKind::RejectOnce,
            label: "Deny".into(),
        },
    ]
}

struct Connection {
    process: AgentProcess,
    turn: Turn,
    next_id: u64,
    permission: Permission,
    /// The thread and the turn that `turn/interrupt` names.
    running: Option<(String, String)>,
    refused: Vec<String>,
    /// The paths of each file change, by item id, for its approval question.
    changes: HashMap<String, Vec<String>>,
    said: Vec<String>,
    ended: Option<Result<(), String>>,
}

impl Connection {
    fn start(agent: &CodexAgent, cwd: &str, control: Control) -> Result<Connection, String> {
        Ok(Connection {
            process: AgentProcess::start(&agent.command, &["app-server".into()], &agent.env, cwd)?,
            turn: Turn::new(agent.timeout, agent.permission_timeout, control),
            next_id: 1,
            permission: Permission::Ask,
            running: None,
            refused: Vec::new(),
            changes: HashMap::new(),
            said: Vec::new(),
            ended: None,
        })
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            &json!({ "clientInfo": { "name": "gnomish_relay", "title": "Gnomish Relay", "version": env!("CARGO_PKG_VERSION") }, "capabilities": null }),
        )?;
        self.process.send(&json!({ "method": "initialized" }))
    }

    /// Resumes the thread of the chat. A thread that Codex cannot resume gives a new one,
    /// and the reply says so.
    fn open_thread(
        &mut self,
        cwd: &str,
        resume: Option<&str>,
        level: Permission,
    ) -> Result<(String, Option<&'static str>), String> {
        let (sandbox, approval) = policy(level);
        let settings = json!({ "cwd": cwd, "sandbox": sandbox, "approvalPolicy": approval });
        if let Some(id) = resume {
            let mut params = settings.clone();
            params["threadId"] = json!(id);
            params["excludeTurns"] = json!(true);
            match self.request("thread/resume", &params) {
                Ok(_) => return Ok((id.to_owned(), None)),
                Err(e) if e == STOPPED => return Err(e),
                Err(_) => {}
            }
        }
        let result = self.request("thread/start", &settings)?;
        let id = text_at(&result, "/thread/id")
            .ok_or("Codex opened no thread.")?
            .to_owned();
        Ok((id, resume.map(|_| NEW_SESSION)))
    }

    fn turn(&mut self, thread: &str, text: &str) -> Result<String, String> {
        let result = self.request(
            "turn/start",
            &json!({ "threadId": thread, "input": [{ "type": "text", "text": text, "text_elements": [] }] }),
        )?;
        let turn = text_at(&result, "/turn/id").ok_or("Codex started no turn.")?;
        self.running = Some((thread.to_owned(), turn.to_owned()));
        while self.ended.is_none() {
            let message = self.receive()?;
            self.handle(&message)?;
        }
        if self.turn.stopping() {
            return Err(STOPPED.into());
        }
        self.ended.take().unwrap_or(Ok(()))?;
        Ok(self.reply())
    }

    fn reply(&self) -> String {
        let last = self.said.last().map_or("", String::as_str);
        let reply = cut(last, MAX_REPLY);
        if self.refused.is_empty() {
            return reply.to_owned();
        }
        format!(
            "{reply}\n\nNot allowed from the game: {}",
            self.refused.join("; ")
        )
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.process
            .send(&json!({ "method": method, "id": id, "params": params }))?;
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
                let text = text_at(error, "/message").unwrap_or("unknown error");
                return Err(format!("Codex failed at {method}: {text}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// At Stop, Codex gets `turn/interrupt`. With no turn yet, there is nothing to stop.
    fn receive(&mut self) -> Result<Value, String> {
        let running = self.running.clone();
        self.turn.receive(&mut self.process, |codex| {
            let Some((thread, turn)) = running else {
                return Err(STOPPED.into());
            };
            // No answer matches this id, so `request` never waits for it.
            codex.send(&json!({ "method": "turn/interrupt", "id": "stop", "params": { "threadId": thread, "turnId": turn } }))
        })
    }

    /// A notification or a request of the server.
    fn handle(&mut self, message: &Value) -> Result<(), String> {
        let method = text_at(message, "/method").unwrap_or("");
        let params = message.get("params").unwrap_or(&Value::Null);
        let Some(id) = message.get("id").cloned() else {
            self.notice(method, params);
            return Ok(());
        };
        let answer = if is_approval(method) {
            json!({ "id": id, "result": { "decision": self.decide(method, params) } })
        } else {
            json!({ "id": id, "error": { "code": METHOD_NOT_FOUND, "message": "not supported" } })
        };
        self.process.send(&answer)
    }

    fn notice(&mut self, method: &str, params: &Value) {
        match read_event(method, params) {
            Event::Started { item, step, paths } => {
                self.turn.progress(step);
                if !paths.is_empty() {
                    self.changes.insert(item, paths);
                }
            }
            Event::Said(text) => self.said.push(text),
            Event::Ended(ending) => self.ended = Some(ending),
            Event::Other => {}
        }
    }

    /// The same rules as ACP (SPEC.md 9.3). The bridge never sends `acceptForSession`
    /// or an amendment: the game adds no "always allow" rule (6.6.5).
    fn decide(&mut self, method: &str, params: &Value) -> &'static str {
        if self.turn.stopping() {
            return "cancel";
        }
        if self.permission == Permission::FullAuto {
            return "accept";
        }
        let item = text_at(params, "/itemId").unwrap_or("");
        let paths = self.changes.get(item).cloned().unwrap_or_default();
        let request = read_request(method, params, &paths);
        if !self.turn.listening() {
            self.refused.push(request.title);
            return "decline";
        }
        match self.turn.ask_game(request.text, choices()) {
            Some(0) => "accept",
            _ => "decline",
        }
    }
}
