//! The native Claude Code backend (SPEC.md 9.2): `claude -p` with stream-json on stdin
//! and stdout, and permission questions as control requests. It needs no Node.
//!
//! The process limits of `process.rs` and `turn.rs` apply to the agent.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use protocol::popup::popup_text;

use crate::agent::{
    Agent, Control, Events, MAX_REPLY, MAX_STEP, NEW_SESSION, Report, Run, SessionInfo, StopSignal,
};
use crate::claude_sessions;
use crate::config::Permission;
use crate::gate::{self, Call, Coverage, Gate, Refusal};
use crate::process::{self, AgentProcess, cut};
use crate::relay::{Job, Work};
use crate::turn::{STOPPED, Turn};

/// The modes of `claude --permission-mode` that the config can name.
pub const MODES: [&str; 5] = ["acceptEdits", "auto", "dontAsk", "manual", "plan"];
/// Claude Code asks nothing in this mode, so the game never sees a tool call (SPEC.md 9.3).
pub const REFUSED_MODE: &str = "bypassPermissions";
const INIT_ID: &str = "init1";
const STOP_ID: &str = "stop1";
const HOOK_ID: &str = "gate";
/// Tools that change nothing outside the session, such as a plan or a tool search.
const SESSION_TOOLS: [&str; 5] = [
    "ToolSearch",
    "TodoWrite",
    "EnterPlanMode",
    "ExitPlanMode",
    "AskUserQuestion",
];
pub const NO_TOOLS: &str = "The story program gets no tools.";
const UNCHECKED: &str = "A tool ran with no check by the bridge, so the run stopped.";
const CHECK_TIME: Duration = Duration::from_secs(30);
/// Claude Code runs a tool when the hook times out, so the bridge answers first.
const HOOK_MARGIN: Duration = Duration::from_mins(5);

pub struct ClaudeAgent {
    pub command: Vec<String>,
    pub env: Vec<String>,
    /// The permission mode for each level, from the `modes` table of the config.
    pub modes: BTreeMap<Permission, String>,
    pub timeout: Duration,
    /// How long a question waits for the game. The run timeout stops meanwhile.
    pub permission_timeout: Duration,
    /// Where Claude Code keeps its sessions (`claude_sessions::projects_dir`).
    pub projects: PathBuf,
    pub gate: Gate,
}

/// Full-auto still runs in `acceptEdits`: the bridge allows each question, so every
/// tool call passes through the bridge first.
fn default_mode(level: Permission) -> &'static str {
    match level {
        Permission::Ask => "plan",
        Permission::AutoEdit | Permission::FullAuto => "acceptEdits",
    }
}

impl Agent for ClaudeAgent {
    fn run(&self, job: &Job, control: &Control) -> Run {
        match &job.work {
            Work::Attach { session, fork } => self.attach(session, *fork),
            Work::Prompt | Work::ListSessions => self.prompt(job, control),
        }
    }

    fn sessions(&self, _cwd: &str) -> Result<Vec<SessionInfo>, String> {
        Ok(claude_sessions::list(&self.projects))
    }
}

impl ClaudeAgent {
    /// Reads the files of Claude Code, so an attach needs no process and no model call.
    fn attach(&self, id: &str, fork: bool) -> Run {
        let Some(path) = claude_sessions::find(&self.projects, id) else {
            return Run {
                reply: Err("The session is gone.".into()),
                session: None,
            };
        };
        let session = if fork {
            claude_sessions::fork(&path, id)
        } else {
            Ok(id.to_owned())
        };
        match session {
            Ok(session) => Run {
                reply: claude_sessions::read_last_exchange(&path),
                session: Some(session),
            },
            Err(e) => Run {
                reply: Err(e),
                session: None,
            },
        }
    }

    /// A session with no file cannot resume, so the run starts a new one and says so.
    fn prompt(&self, job: &Job, control: &Control) -> Run {
        let (resume, note) = match job.resume.as_deref() {
            Some(id) if claude_sessions::find(&self.projects, id).is_some() => (Some(id), None),
            Some(_) => (None, Some(NEW_SESSION)),
            None => (None, None),
        };
        let args = self.args(job.permission, resume);
        let mut stream = match AgentProcess::start(&self.command, &args, &self.env, &job.cwd) {
            Ok(process) => {
                let rules = Rules::Gate(Gated {
                    gate: self.gate.clone(),
                    permission: job.permission,
                    agent: job.agent.clone(),
                    cwd: job.cwd.clone(),
                });
                let turn = Turn::new(self.timeout, self.permission_timeout, control.clone());
                let mut stream = Stream::new(process, turn, rules, self.permission_timeout);
                stream.session = resume.map(str::to_owned);
                stream
            }
            Err(e) => {
                return Run {
                    reply: Err(e),
                    session: job.resume.clone(),
                };
            }
        };
        let reply = stream.talk(&job.text);
        Run {
            reply: reply.map(|reply| match note {
                Some(note) => format!("{note}\n\n{reply}"),
                None => reply,
            }),
            session: stream.session,
        }
    }

    fn args(&self, level: Permission, resume: Option<&str>) -> Vec<String> {
        let mode = self
            .modes
            .get(&level)
            .map_or(default_mode(level), String::as_str);
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-prompt-tool",
            "stdio",
            "--permission-mode",
            mode,
        ]
        .map(str::to_owned)
        .into();
        if let Some(id) = resume {
            args.extend(["--resume".to_owned(), id.to_owned()]);
        }
        args
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
        let status = process::output(
            &self.command,
            &["auth".into(), "status".into(), "--json".into()],
            &self.env,
            cwd,
            CHECK_TIME,
        )?;
        if !logged_in(&status.stdout) {
            return Err("Claude Code needs a login.".into());
        }
        Ok(Report {
            name: "Claude Code".into(),
            version: version
                .stdout
                .split_whitespace()
                .next()
                .unwrap_or("?")
                .to_owned(),
            load_session: true,
            modes: MODES.map(str::to_owned).into(),
        })
    }
}

/// `claude auth status --json` says `"loggedIn": true`.
pub fn logged_in(status: &str) -> bool {
    let status: Value = serde_json::from_str(status).unwrap_or(Value::Null);
    status.get("loggedIn").and_then(Value::as_bool) == Some(true)
}

/// One line of `--output-format stream-json`, as the bridge uses it.
#[derive(Debug, PartialEq)]
pub enum Message {
    /// The session id of the run.
    Started(String),
    /// A message of the model: its text, and a progress line for each tool call.
    Said {
        text: String,
        steps: Vec<String>,
    },
    /// A tool call that asks for permission (`can_use_tool`).
    Ask {
        id: Value,
        request: Request,
    },
    /// The `PreToolUse` hook of the bridge, before every tool call. `None` when the
    /// bridge cannot read the call.
    Hook {
        id: Value,
        request: Option<Request>,
    },
    /// The tool calls that ran and did not fail, by id.
    Ran(Vec<String>),
    /// A control request that the bridge does not serve.
    Unsupported {
        id: Value,
    },
    /// The answer to a control request of the bridge.
    Answered {
        id: String,
        error: Option<String>,
    },
    /// The end of the turn: the final text, or an error text.
    Ended {
        session: Option<String>,
        reply: Result<String, String>,
    },
    Other,
}

/// A tool call as the gate sees it.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub tool: String,
    pub tool_use_id: Option<String>,
    /// The output of `popup_text` (S15).
    pub text: Vec<u8>,
    /// The tool and its reason, for "Not allowed from the game:".
    pub title: String,
    /// The tool input, which an allow sends back unchanged.
    pub input: Value,
}

pub fn read_message(message: &Value) -> Message {
    match text_at(message, "/type") {
        Some("system") if text_at(message, "/subtype") == Some("init") => {
            match text_at(message, "/session_id") {
                Some(id) => Message::Started(id.to_owned()),
                None => Message::Other,
            }
        }
        Some("assistant") => read_said(message),
        Some("user") => read_ran(message),
        Some("control_request") => read_control(message),
        Some("control_response") => read_answered(message),
        Some("result") => read_ended(message),
        _ => Message::Other,
    }
}

fn read_said(message: &Value) -> Message {
    let blocks = message
        .pointer("/message/content")
        .and_then(Value::as_array);
    let mut text = String::new();
    let mut steps = Vec::new();
    for block in blocks.into_iter().flatten() {
        match text_at(block, "/type") {
            Some("text") => text.push_str(text_at(block, "/text").unwrap_or("")),
            Some("tool_use") => steps.push(step_of(block)),
            _ => {}
        }
    }
    Message::Said { text, steps }
}

/// One progress line for the activity panel: the command, else the tool and its file.
fn step_of(tool_use: &Value) -> String {
    let name = text_at(tool_use, "/name").unwrap_or("a tool call");
    let input = tool_use.get("input").unwrap_or(&Value::Null);
    let line = match (text_at(input, "/command"), path_of(input)) {
        (Some(command), _) => format!("$ {command}"),
        (None, Some(path)) => format!("{name} {path}"),
        (None, None) => name.to_owned(),
    };
    let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    cut(&line, MAX_STEP).to_owned()
}

fn path_of(input: &Value) -> Option<&str> {
    ["file_path", "notebook_path", "path", "url", "pattern"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
}

/// A call that failed also ran, but a call that the gate refused fails too, and so does
/// a call with bad input, which no hook sees. So only calls that did not fail count.
fn read_ran(message: &Value) -> Message {
    let blocks = message
        .pointer("/message/content")
        .and_then(Value::as_array);
    let ran = blocks
        .into_iter()
        .flatten()
        .filter(|b| text_at(b, "/type") == Some("tool_result"))
        .filter(|b| b.get("is_error").and_then(Value::as_bool) != Some(true))
        .filter_map(|b| text_at(b, "/tool_use_id").map(str::to_owned))
        .collect();
    Message::Ran(ran)
}

fn read_control(message: &Value) -> Message {
    let id = message.get("request_id").cloned().unwrap_or(Value::Null);
    let request = message.get("request").unwrap_or(&Value::Null);
    match text_at(request, "/subtype") {
        Some("can_use_tool") => Message::Ask {
            id,
            request: read_request(request),
        },
        Some("hook_callback") => Message::Hook {
            id,
            request: read_hook(request),
        },
        _ => Message::Unsupported { id },
    }
}

/// The input of a `PreToolUse` hook has the tool call as `can_use_tool` has it.
fn read_hook(request: &Value) -> Option<Request> {
    let input = request.get("input")?;
    if text_at(input, "/hook_event_name") != Some("PreToolUse") {
        return None;
    }
    let call = json!({
        "tool_name": text_at(input, "/tool_name")?,
        "input": input.get("tool_input")?,
        "tool_use_id": text_at(input, "/tool_use_id").or_else(|| text_at(request, "/tool_use_id")),
    });
    Some(read_request(&call))
}

/// The popup shows the raw command or path first, and the words of the agent below
/// it (SPEC.md 6.4).
pub fn read_request(request: &Value) -> Request {
    let tool = text_at(request, "/tool_name").unwrap_or("a tool");
    let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
    let reason = text_at(request, "/description")
        .or_else(|| text_at(&input, "/description"))
        .unwrap_or("");
    let title = if reason.is_empty() {
        tool.to_owned()
    } else {
        format!("{tool}: {reason}")
    };
    let raw = text_at(&input, "/command")
        .or_else(|| path_of(&input))
        .unwrap_or(tool);
    Request {
        tool: tool.to_owned(),
        tool_use_id: text_at(request, "/tool_use_id").map(str::to_owned),
        text: popup_text(raw.as_bytes(), title.as_bytes()),
        title: cut(&title, MAX_STEP).to_owned(),
        input,
    }
}

/// The classifier input of a tool of Claude Code (SPEC.md 6.6.3). A relative path is
/// relative to the folder of the chat. A tool that the bridge does not know is unknown.
pub fn tool_call(request: &Request, cwd: &Path) -> Call {
    let input = &request.input;
    let path = |key: &str| text_at(input, key).map(|p| cwd.join(p));
    let (text, title) = (request.text.clone(), request.title.clone());
    let files = |reads: Option<PathBuf>, writes: Option<PathBuf>| match (reads, writes) {
        (Some(read), None) => Call::files(&[read], &[], text.clone(), title.clone()),
        (None, Some(write)) => Call::files(&[], &[write], text.clone(), title.clone()),
        _ => Call::unknown(text.clone(), title.clone()),
    };
    match request.tool.as_str() {
        "Read" => files(path("/file_path"), None),
        "Write" | "Edit" | "MultiEdit" => files(None, path("/file_path")),
        "NotebookEdit" => files(None, path("/notebook_path")),
        "Glob" if !is_plain_glob(text_at(input, "/pattern").unwrap_or("")) => files(None, None),
        "Glob" | "Grep" | "LS" => files(path("/path").or(Some(cwd.to_owned())), None),
        "Bash" => match text_at(input, "/command") {
            Some(command) => Call::command(command, cwd, text, title),
            None => Call::unknown(text, title),
        },
        tool if SESSION_TOOLS.contains(&tool) => Call::files(&[], &[], text, title),
        _ => Call::unknown(text, title),
    }
}

/// A glob that stays under its folder: no absolute part, no `~`, and no `..`.
fn is_plain_glob(pattern: &str) -> bool {
    let absolute = pattern.starts_with(['/', '\\', '~']) || pattern.contains(':');
    !absolute && !pattern.contains("..")
}

fn read_answered(message: &Value) -> Message {
    let response = message.get("response").unwrap_or(&Value::Null);
    let error = (text_at(response, "/subtype") == Some("error")).then(|| {
        text_at(response, "/error")
            .unwrap_or("unknown error")
            .to_owned()
    });
    Message::Answered {
        id: text_at(response, "/request_id").unwrap_or("").to_owned(),
        error,
    }
}

fn read_ended(message: &Value) -> Message {
    let session = text_at(message, "/session_id").map(str::to_owned);
    let failed = message.get("is_error").and_then(Value::as_bool) == Some(true)
        || text_at(message, "/subtype") != Some("success");
    let text = text_at(message, "/result").unwrap_or("");
    let reply = if failed {
        let subtype = text_at(message, "/subtype").unwrap_or("error");
        Err(match text {
            "" => format!("The agent stopped: {subtype}"),
            text => format!("The agent stopped: {}", cut(text, 300)),
        })
    } else {
        Ok(cut(text, MAX_REPLY).to_owned())
    };
    Message::Ended { session, reply }
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn allow(input: &Value) -> Value {
    json!({ "behavior": "allow", "updatedInput": input })
}

fn deny(why: &str) -> Value {
    json!({ "behavior": "deny", "message": why })
}

/// The answer of the hook. It is only ever allow or deny: "ask" hands the call to the
/// permission rules of Claude Code, which the settings of the user can loosen.
fn hook_output(result: &Result<(), Refusal>) -> Value {
    let (decision, reason) = match result {
        Ok(()) => ("allow", "Allowed by Gnomish Relay."),
        Err(refusal) => ("deny", refusal.reason()),
    };
    json!({ "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": decision,
        "permissionDecisionReason": reason,
    }})
}

/// One answer of the model with no tools, for the story program of Timeways (SPEC.md
/// 9.7, decision 10). The hook and the check on tool results stay on, and every tool
/// call gets a deny. A stop ends the run at once: there is no session to keep.
pub fn answer_with_no_tools(
    command: &[String],
    args: &[String],
    folder: &str,
    prompt: &str,
    timeout: Duration,
    stop: StopSignal,
) -> Result<String, String> {
    let process = AgentProcess::start(command, args, &[], folder)?;
    let control = Control {
        stop,
        events: Events::default(),
    };
    let turn = Turn::new(timeout, Duration::ZERO, control);
    let mut stream = Stream::new(process, turn, Rules::NoTools, Duration::ZERO);
    stream.talk(prompt)
}

/// The gate of a run of the relay: its job, as the gate sees it.
struct Gated {
    gate: Gate,
    permission: Permission,
    agent: String,
    cwd: String,
}

/// Who answers the tool calls of a run.
enum Rules {
    Gate(Gated),
    /// The model route of the story program: every tool call gets a deny.
    NoTools,
}

/// One run of `claude -p`: the answers to its control requests and the reply.
struct Stream {
    process: AgentProcess,
    turn: Turn,
    rules: Rules,
    hook_timeout: Duration,
    /// The tool calls that the hook answered.
    checked: HashSet<String>,
    /// The tool calls that the hook allowed, so `can_use_tool` does not ask twice.
    allowed: HashSet<String>,
    session: Option<String>,
    /// Before the prompt, Stop has no turn to interrupt.
    prompted: bool,
    refused: Vec<String>,
    /// The text of the model, for a result with no text.
    said: String,
}

impl Stream {
    fn new(
        process: AgentProcess,
        turn: Turn,
        rules: Rules,
        permission_timeout: Duration,
    ) -> Stream {
        Stream {
            process,
            turn,
            rules,
            hook_timeout: permission_timeout + HOOK_MARGIN,
            checked: HashSet::new(),
            allowed: HashSet::new(),
            session: None,
            prompted: false,
            refused: Vec::new(),
            said: String::new(),
        }
    }

    fn talk(&mut self, prompt: &str) -> Result<String, String> {
        let hooks = json!({ "PreToolUse": [{
            "hookCallbackIds": [HOOK_ID],
            "timeout": self.hook_timeout.as_secs(),
        }]});
        self.send(&json!({ "type": "control_request", "request_id": INIT_ID, "request": { "subtype": "initialize", "hooks": hooks } }))?;
        self.wait_for_answer(INIT_ID)?;
        self.send(&json!({ "type": "user", "message": { "role": "user", "content": prompt } }))?;
        self.prompted = true;
        loop {
            let message = self.receive()?;
            if let Some(reply) = self.handle(read_message(&message))? {
                return Ok(reply);
            }
        }
    }

    fn wait_for_answer(&mut self, request: &str) -> Result<(), String> {
        loop {
            match read_message(&self.receive()?) {
                Message::Answered { id, error } if id == request => {
                    return match error {
                        Some(error) => Err(format!("The agent failed at {request}: {error}")),
                        None => Ok(()),
                    };
                }
                other => {
                    self.handle(other)?;
                }
            }
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        self.process.send(message)
    }

    fn receive(&mut self) -> Result<Value, String> {
        let can_interrupt = self.prompted && matches!(self.rules, Rules::Gate(_));
        self.turn.receive(&mut self.process, |agent| {
            if !can_interrupt {
                return Err(STOPPED.into());
            }
            agent.send(&json!({ "type": "control_request", "request_id": STOP_ID, "request": { "subtype": "interrupt" } }))
        })
    }

    /// The final reply at the end of the turn, else `None`.
    fn handle(&mut self, message: Message) -> Result<Option<String>, String> {
        match message {
            Message::Started(id) => self.session = Some(id),
            Message::Said { text, steps } => {
                let room = MAX_REPLY.saturating_sub(self.said.len());
                self.said.push_str(cut(&text, room));
                for step in steps {
                    self.turn.progress(step);
                }
            }
            Message::Ask { id, request } => {
                let decision = self.decide(&request);
                self.respond(
                    &json!({ "subtype": "success", "request_id": id, "response": decision }),
                )?;
            }
            Message::Hook { id, request } => {
                let output = hook_output(&self.hook(request.as_ref()));
                self.respond(
                    &json!({ "subtype": "success", "request_id": id, "response": output }),
                )?;
            }
            Message::Ran(ids) => {
                if ids.iter().any(|id| !self.checked.contains(id)) {
                    return Err(UNCHECKED.into());
                }
            }
            Message::Unsupported { id } => {
                self.respond(
                    &json!({ "subtype": "error", "request_id": id, "error": "not supported" }),
                )?;
            }
            Message::Ended { session, reply } => return self.end(session, reply).map(Some),
            Message::Answered { .. } | Message::Other => {}
        }
        Ok(None)
    }

    fn respond(&mut self, response: &Value) -> Result<(), String> {
        self.send(&json!({ "type": "control_response", "response": response }))
    }

    fn end(
        &mut self,
        session: Option<String>,
        reply: Result<String, String>,
    ) -> Result<String, String> {
        if session.is_some() {
            self.session = session;
        }
        if self.turn.stopping() {
            return Err(STOPPED.into());
        }
        let reply = match reply? {
            text if text.is_empty() => std::mem::take(&mut self.said),
            text => text,
        };
        if self.refused.is_empty() {
            return Ok(reply);
        }
        Ok(format!(
            "{reply}\n\nNot allowed from the game: {}",
            self.refused.join("; ")
        ))
    }

    /// The gate for one tool call (SPEC.md 6.6.3).
    fn check(&mut self, request: &Request) -> Result<(), Refusal> {
        if self.turn.stopping() {
            return Err(Refusal::ByRule("Stopped from the game.".into()));
        }
        let Rules::Gate(gated) = &self.rules else {
            return Err(Refusal::ByRule(NO_TOOLS.into()));
        };
        let call = tool_call(request, Path::new(&gated.cwd));
        let job = gate::Job {
            agent: &gated.agent,
            cwd: &gated.cwd,
            level: gated.permission,
            coverage: Coverage::Every,
        };
        let result = gated.gate.check(&call, &job, &mut self.turn);
        if let Err(Refusal::ByRule(_)) = &result {
            self.refused.push(request.title.clone());
        }
        result
    }

    /// A hook with no readable tool call gets a deny.
    fn hook(&mut self, request: Option<&Request>) -> Result<(), Refusal> {
        let Some(request) = request else {
            return Err(Refusal::ByRule(
                "The bridge cannot read this tool call.".into(),
            ));
        };
        let result = self.check(request);
        if let Some(id) = &request.tool_use_id {
            self.checked.insert(id.clone());
            if result.is_ok() {
                self.allowed.insert(id.clone());
            }
        }
        result
    }

    /// The second line after the hook. No answer ever holds the suggested rules of
    /// Claude Code: the game adds no "always allow" rule (6.6.5).
    fn decide(&mut self, request: &Request) -> Value {
        let passed = request
            .tool_use_id
            .as_ref()
            .is_some_and(|id| self.allowed.contains(id));
        if passed {
            return allow(&request.input);
        }
        match self.check(request) {
            Ok(()) => allow(&request.input),
            Err(refusal) => deny(refusal.reason()),
        }
    }
}
