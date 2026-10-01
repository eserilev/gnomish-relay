//! The native Codex backend (SPEC.md 9.2): `codex app-server`, JSON-RPC 2.0 with no
//! `jsonrpc` field, one message per line. It needs no Node and no adapter.
//!
//! The process limits of `process.rs` and `turn.rs` apply to the agent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use protocol::popup::popup_text;

use crate::agent::{
    Agent, Control, MAX_PROMPT, MAX_REPLY, MAX_STEP, NEW_SESSION, Report, Run, SUMMARY_FIRST,
    SessionId, SessionInfo, exchange_text,
};
use crate::agent_wall::{AgentWall, RunWall, Walled, agent_env, made_notice, with_notes};
use crate::config::{Kind, Permission};
use crate::gate::{self, Call, Coverage, Gate, Refusal, Sandboxing};
use crate::process::{self, AgentProcess, cut};
use crate::relay::{Job, Open, Work};
use crate::turn::{STOPPED, Turn};
use crate::usage::{Usage, tokens_at};

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
    pub gate: Gate,
    /// The wall of the agent process, with the hosts of Codex (SPEC.md 6.6.4).
    pub wall: AgentWall,
}

/// `untrusted` asks before every command and every change, so each one reaches the
/// gate (SPEC.md 9.3). The sandbox of the level applies after the answer.
pub const APPROVAL_POLICY: &str = "untrusted";

/// The sandbox of Codex for each level.
pub fn sandbox(level: Permission) -> &'static str {
    match level {
        Permission::Ask => "read-only",
        Permission::AutoEdit | Permission::FullAuto => "workspace-write",
    }
}

/// The settings of a thread. `approvalsReviewer` keeps a reviewer model of the user's
/// config out of the way, and web search runs with no approval, so it is off. The
/// sandbox writes the chat folder and `TMPDIR`, the private temp folder of the run, and
/// not the shared `/tmp` (SPEC.md 6.6.4).
pub fn thread_settings(cwd: &str, level: Permission) -> Value {
    json!({
        "cwd": cwd,
        "sandbox": sandbox(level),
        "approvalPolicy": APPROVAL_POLICY,
        "approvalsReviewer": "user",
        "developerInstructions": SUMMARY_FIRST,
        "config": {
            "web_search": "disabled",
            "sandbox_workspace_write": { "exclude_slash_tmp": true },
        },
    })
}

/// Codex runs each command as `<shell> -lc '<script>'`. The classifier gets the script,
/// which is what the shell runs. Any other form stays as it is.
pub fn unwrap_shell(command: &str) -> String {
    let Some(words) = shlex::split(command) else {
        return command.to_owned();
    };
    let [shell, flag, script] = words.as_slice() else {
        return command.to_owned();
    };
    let name = shell.rsplit(['/', '\\']).next().unwrap_or(shell);
    let is_shell = matches!(name, "bash" | "zsh" | "sh");
    if is_shell && matches!(flag.as_str(), "-lc" | "-c") {
        return script.clone();
    }
    command.to_owned()
}

impl Agent for CodexAgent {
    fn run(&self, job: &Job, control: &Control) -> Run {
        let mut session = None;
        let mut usage = None;
        let reply = match &job.work {
            Work::Attach { session: id, open } => {
                self.attach(job, id.as_str(), *open, &mut session)
            }
            Work::Prompt
            | Work::ListSessions
            | Work::ListFolders
            | Work::ListSubfolders
            | Work::ListSettings
            | Work::Git(_) => self.prompt(job, control, &mut session, &mut usage),
        };
        Run {
            reply,
            session: session.map(SessionId::from),
            usage,
        }
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
        usage: &mut Option<Usage>,
    ) -> Result<String, String> {
        let mut codex = Connection::start(self, &job.cwd, control.clone())?;
        codex.permission = job.permission;
        codex.agent.clone_from(&job.agent);
        codex.initialize()?;
        let (thread, note) = codex.open_thread(&job.cwd, job.resume_id(), job.permission)?;
        *session = Some(thread.clone());
        let reply = codex.turn(&thread, &job.text);
        *usage = codex.usage();
        let reply = reply?;
        let notes: Vec<&str> = note.into_iter().chain(self.wall.notice()).collect();
        let after = codex.made_notice(self.wall.home.as_deref());
        Ok(with_notes(reply, &notes, after))
    }

    /// Reads the saved thread, and forks it first if the terminal has it open. Neither
    /// call reaches the model.
    fn attach(
        &self,
        job: &Job,
        thread: &str,
        open: Open,
        session: &mut Option<String>,
    ) -> Result<String, String> {
        let mut codex = Connection::start(self, &job.cwd, Control::default())?;
        codex.initialize()?;
        let last = codex.request(
            "thread/turns/list",
            &json!({ "threadId": thread, "limit": 1, "itemsView": "full" }),
        )?;
        let id = if open == Open::Fork {
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
            return Err("Codex isn't logged in. Run codex login.".into());
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
            details: Vec::new(),
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
        paths: Vec<Change>,
    },
    /// The final text of one agent message.
    Said(String),
    /// The output of a command that ended, for the test line (SPEC.md 9.11).
    Output(String),
    /// The end of the turn: the reply, or an error text.
    Ended(Result<(), String>),
    /// The tokens of the thread so far, and of its last model call.
    Used {
        total: Usage,
        last: Usage,
    },
    Other,
}

pub fn read_event(method: &str, params: &Value) -> Event {
    match method {
        "item/started" => read_started(params.get("item").unwrap_or(&Value::Null)),
        "item/completed" if text_at(params, "/item/type") == Some("agentMessage") => {
            Event::Said(text_at(params, "/item/text").unwrap_or("").to_owned())
        }
        "item/completed" if text_at(params, "/item/type") == Some("commandExecution") => {
            Event::Output(
                text_at(params, "/item/aggregatedOutput")
                    .unwrap_or("")
                    .to_owned(),
            )
        }
        "turn/completed" => Event::Ended(read_ending(params.get("turn").unwrap_or(&Value::Null))),
        "thread/tokenUsage/updated" => Event::Used {
            total: read_tokens(params, "/tokenUsage/total"),
            last: read_tokens(params, "/tokenUsage/last"),
        },
        _ => Event::Other,
    }
}

/// `inputTokens` holds the cached ones. Codex gives no cost.
fn read_tokens(params: &Value, pointer: &str) -> Usage {
    let breakdown = params.pointer(pointer).unwrap_or(&Value::Null);
    Usage {
        input: tokens_at(breakdown, "/inputTokens"),
        cached: tokens_at(breakdown, "/cachedInputTokens"),
        output: tokens_at(breakdown, "/outputTokens"),
        cost_usd: None,
    }
}

fn read_started(item: &Value) -> Event {
    let id = text_at(item, "/id").unwrap_or("").to_owned();
    match text_at(item, "/type") {
        Some("commandExecution") => Event::Started {
            item: id,
            step: step(&format!(
                "$ {}",
                unwrap_shell(text_at(item, "/command").unwrap_or(""))
            )),
            paths: Vec::new(),
        },
        Some("fileChange") => {
            let changes = item.get("changes").and_then(Value::as_array);
            let paths: Vec<Change> = changes
                .into_iter()
                .flatten()
                .filter_map(read_change)
                .collect();
            Event::Started {
                item: id,
                step: step(&format!("edit {}", shown(&paths))),
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

/// One path of a patch. Codex writes the move target too, so the gate checks both.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub path: String,
    pub moved_to: Option<String>,
}

impl Change {
    pub fn at(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            moved_to: None,
        }
    }

    fn written(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.path).chain(&self.moved_to)
    }

    fn shown(&self) -> String {
        match &self.moved_to {
            Some(target) => format!("{} -> {target}", self.path),
            None => self.path.clone(),
        }
    }
}

fn read_change(change: &Value) -> Option<Change> {
    Some(Change {
        path: text_at(change, "/path")?.to_owned(),
        moved_to: text_at(change, "/kind/move_path").map(str::to_owned),
    })
}

fn shown(changes: &[Change]) -> String {
    let texts: Vec<String> = changes.iter().map(Change::shown).collect();
    texts.join(", ")
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

/// The classifier input of an approval request. A request with no command and no path
/// is unknown, for example a network approval.
pub fn approval_call(method: &str, params: &Value, paths: &[Change], cwd: &Path) -> Call {
    let request = read_request(method, params, paths);
    let (text, title) = (request.text, request.title);
    let cwd = text_at(params, "/cwd").map_or_else(|| cwd.to_owned(), PathBuf::from);
    if method == "item/commandExecution/requestApproval" {
        return match text_at(params, "/command") {
            Some(command) => Call::command(&unwrap_shell(command), &cwd, text, title),
            None => Call::unknown(text, title),
        };
    }
    // Codex applies every path of the patch, also one outside the root.
    let root = text_at(params, "/grantRoot").map(|root| cwd.join(root));
    let writes: Vec<PathBuf> = root
        .into_iter()
        .chain(paths.iter().flat_map(Change::written).map(|p| cwd.join(p)))
        .collect();
    if writes.is_empty() {
        return Call::unknown(text, title);
    }
    Call::files(&[], &writes, text, title)
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
pub fn read_request(method: &str, params: &Value, paths: &[Change]) -> Request {
    let reason = text_at(params, "/reason").unwrap_or("");
    let (raw, default_title) = match method {
        "item/commandExecution/requestApproval" => (
            unwrap_shell(text_at(params, "/command").unwrap_or("")),
            "run a command",
        ),
        _ => match text_at(params, "/grantRoot") {
            Some(root) => (format!("write anything in {root}"), "change files"),
            None => (shown(paths), "change files"),
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

struct Connection {
    process: AgentProcess,
    turn: Turn,
    next_id: u64,
    permission: Permission,
    gate: Gate,
    agent: String,
    cwd: String,
    /// The thread and the turn that `turn/interrupt` names.
    running: Option<(String, String)>,
    refused: Vec<String>,
    /// The paths of each file change, by item id, for its approval question.
    changes: HashMap<String, Vec<Change>>,
    said: Vec<String>,
    ended: Option<Result<(), String>>,
    /// The tokens of the thread before this turn, and the newest total (SPEC.md 9.10).
    used: Option<(Usage, Usage)>,
    /// The wall of the agent process and its proxy (SPEC.md 6.6.4), after the process.
    wall: Option<RunWall>,
    /// The private temp folder of the run. It goes away with the run.
    _temp: tempfile::TempDir,
}

impl Connection {
    fn start(agent: &CodexAgent, cwd: &str, control: Control) -> Result<Connection, String> {
        let (temp, temp_path) = crate::command_sandbox::make_temp()?;
        let wall = agent
            .wall
            .prepare(&[Path::new(cwd), &temp_path], "agent of codex")?;
        let mut vars = vec![("TMPDIR".to_owned(), temp_path.into())];
        vars.extend(agent_env(Kind::Codex, Walled::of(wall.as_ref())));
        let args = ["app-server".to_owned()];
        let process =
            AgentProcess::start_in(&agent.command, &args, &agent.env, &vars, cwd, wall.as_ref())?;
        Ok(Connection {
            process,
            turn: Turn::new(agent.timeout, agent.permission_timeout, control),
            next_id: 1,
            permission: Permission::Ask,
            gate: agent.gate.clone(),
            agent: String::new(),
            cwd: cwd.to_owned(),
            running: None,
            refused: Vec::new(),
            changes: HashMap::new(),
            said: Vec::new(),
            ended: None,
            used: None,
            wall,
            _temp: temp,
        })
    }

    /// The note of the startup files that the agent made during the run.
    fn made_notice(&self, home: Option<&Path>) -> Option<String> {
        let made = self
            .wall
            .as_ref()
            .map(RunWall::made_startup_files)
            .unwrap_or_default();
        made_notice(&made, home)
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
        let settings = thread_settings(cwd, level);
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
            return Err(self.turn.ended().into());
        }
        self.ended.take().unwrap_or(Ok(()))?;
        Ok(self.reply())
    }

    /// The total counts the whole thread, so the turn is the newest total less the
    /// total before its first model call.
    fn usage(&self) -> Option<Usage> {
        self.used.map(|(before, total)| total.since(before))
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
            Event::Output(text) => self.turn.output(text),
            Event::Ended(ending) => self.ended = Some(ending),
            Event::Used { total, last } => {
                let before = self
                    .used
                    .map_or_else(|| total.since(last), |(before, _)| before);
                self.used = Some((before, total));
            }
            Event::Other => {}
        }
    }

    /// The gate answers (SPEC.md 6.6.3). The bridge never sends `acceptForSession` or an
    /// amendment: the game adds no "always allow" rule (6.6.5).
    fn decide(&mut self, method: &str, params: &Value) -> &'static str {
        if self.turn.stopping() {
            return "cancel";
        }
        let item = text_at(params, "/itemId").unwrap_or("");
        let paths = self.changes.get(item).cloned().unwrap_or_default();
        let call = approval_call(method, params, &paths, Path::new(&self.cwd));
        let job = gate::Job {
            agent: &self.agent,
            cwd: &self.cwd,
            level: self.permission,
            coverage: Coverage::Every,
            sandboxing: Sandboxing::On,
            wall: gate::SandboxWall::Leaks,
        };
        match self.gate.check(&call, &job, &mut self.turn) {
            Ok(()) => "accept",
            Err(Refusal::ByUser) => "decline",
            Err(Refusal::ByRule(_)) => {
                self.refused.push(call.title);
                "decline"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_ended_gives_its_output_for_the_test_line() {
        let params = serde_json::json!({ "item": {
            "type": "commandExecution", "id": "c1",
            "aggregatedOutput": "test result: ok. 3 passed; 0 failed;",
        }});

        let event = read_event("item/completed", &params);

        assert_eq!(
            event,
            Event::Output("test result: ok. 3 passed; 0 failed;".into())
        );
    }

    #[test]
    fn a_shell_wrapper_gives_its_script_and_any_other_command_stays() {
        assert_eq!(
            unwrap_shell("/bin/bash -lc 'cargo test -q'"),
            "cargo test -q"
        );
        assert_eq!(unwrap_shell("/usr/bin/zsh -c 'echo \"hi\"'"), "echo \"hi\"");
        assert_eq!(unwrap_shell("bash -lc 'a' extra"), "bash -lc 'a' extra");
        assert_eq!(unwrap_shell("python -c 'x'"), "python -c 'x'");
        assert_eq!(unwrap_shell("bash -lc 'open"), "bash -lc 'open");
    }

    #[test]
    fn every_level_asks_before_each_command_and_never_opens_the_sandbox() {
        for level in [Permission::Ask, Permission::AutoEdit, Permission::FullAuto] {
            let settings = thread_settings("/w", level);
            assert_eq!(settings["approvalPolicy"], "untrusted");
            assert_eq!(settings["approvalsReviewer"], "user");
            assert_ne!(settings["sandbox"], "danger-full-access");
        }
        assert_eq!(sandbox(Permission::Ask), "read-only");
    }

    #[test]
    fn a_thread_asks_for_a_summary_before_a_long_reply() {
        let settings = thread_settings("/w", Permission::AutoEdit);
        assert_eq!(
            settings["developerInstructions"],
            crate::agent::SUMMARY_FIRST
        );
    }

    #[test]
    fn the_sandbox_of_codex_writes_no_shared_temp_folder() {
        let settings = thread_settings("/w", Permission::AutoEdit);
        let write = &settings["config"]["sandbox_workspace_write"];
        assert_eq!(write["exclude_slash_tmp"], true);
        assert!(write.get("exclude_tmpdir_env_var").is_none());
    }

    #[test]
    fn a_network_approval_with_no_command_is_unknown() {
        let params = json!({ "networkApprovalContext": { "host": "x", "protocol": "https" } });
        let call = approval_call(
            "item/commandExecution/requestApproval",
            &params,
            &[],
            Path::new("/w"),
        );
        assert!(matches!(call.tool, protocol::action::ToolCall::Unknown));
    }
}
