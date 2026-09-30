//! The `command` backend (SPEC.md 9.2): any LLM harness with only a command line. The
//! bridge cannot see its tool calls, so the whole harness runs inside the sandbox of the
//! run, and the level picks the walls.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use protocol::connect::Mode;

use crate::agent::{Agent, Control, Event, MAX_REPLY, MAX_STEP, Report, Run, SessionId};
use crate::agent_wall::{scan_sockets, with_notes};
use crate::allow_hosts::{Defaults, HostList};
use crate::command_sandbox::{
    self, ChatAccess, CommandSandbox, Guarded, HomeWrites, RunWalls, Shape, command_env,
};
use crate::config::{Kind, Permission};
use crate::gate::Gate;
use crate::harness_args::{self, Input};
use crate::harness_output::{progress_line, reply_text};
use crate::harness_process::{self, Finished, Limits, Start};
use crate::harness_sandbox::{self, NO_SANDBOX};
use crate::program::find_program;
use crate::proxy::ProxySettings;
use crate::relay::{Job, Work};

/// The chat of a `command` agent has no session id: this marks that it ran before.
pub const RAN_BEFORE: &str = "ran";
const PROMPT_FILE: &str = "prompt.txt";
/// A harness that prints more than this for one message is broken or hostile.
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
const CHECK_TIME: Duration = Duration::from_secs(30);
pub const NO_OUTPUT: &str = "(The agent didn't reply.)";

pub struct CommandAgent {
    pub name: String,
    /// The template of `harness_args`, with the program first.
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub resume: Vec<String>,
    pub ask_args: Vec<String>,
    pub agent_hosts: Vec<String>,
    pub timeout: Duration,
    pub gate: Gate,
    /// The note that the harness runs commands with no question shows once for each start.
    told: Arc<AtomicBool>,
}

impl CommandAgent {
    pub fn new(
        name: &str,
        spec: &crate::config::AgentSpec,
        timeout: Duration,
        gate: &Gate,
    ) -> CommandAgent {
        CommandAgent {
            name: name.to_owned(),
            command: spec.command.clone(),
            env: spec.env.clone(),
            resume: spec.resume.clone(),
            ask_args: spec.ask_args.clone(),
            agent_hosts: spec.agent_hosts.clone(),
            timeout,
            gate: gate.clone(),
            told: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Agent for CommandAgent {
    fn run(&self, job: &Job, control: &Control) -> Run {
        let reply = match &job.work {
            Work::Attach { .. } => Err("This agent has no sessions to resume.".into()),
            Work::Prompt | Work::ListSessions | Work::ListFolders | Work::ListSettings => {
                self.prompt(job, control)
            }
        };
        let session = reply.is_ok().then(|| SessionId::from(RAN_BEFORE));
        Run {
            reply,
            session: session.or_else(|| job.resume.clone()),
        }
    }
}

/// The note for the first reply at a level that writes.
pub fn free_commands_note(name: &str) -> String {
    format!("({name} runs its own commands without asking, inside the sandbox.)")
}

impl CommandAgent {
    fn prompt(&self, job: &Job, control: &Control) -> Result<String, String> {
        let access = chat_access(job.permission);
        let walls = self.walls(&job.cwd, access, &format!("chat {}", job.chat))?;
        let input = harness_args::input_of(&self.command);
        let prompt_file = walls.walls.temp.join(PROMPT_FILE);
        if input == Input::File {
            crate::fs_safe::write_private(&walls.walls.temp, PROMPT_FILE, &job.text)
                .map_err(|e| format!("No file for the message: {e:#}"))?;
        }
        let mut extra = Vec::new();
        if job.resume.is_some() {
            extra.extend(self.resume.iter().cloned());
        }
        if access == ChatAccess::Read {
            extra.extend(self.ask_args.iter().cloned());
        }
        let args = harness_args::expand(&self.command, &extra, &job.text, &prompt_file);
        let start = self.start(
            &walls,
            Path::new(&job.cwd),
            &args,
            harness_args::stdin_bytes(input, &job.text),
        )?;
        let limits = Limits {
            timeout: self.timeout,
            max_output: MAX_OUTPUT,
            keep: MAX_REPLY,
        };
        let events = &control.events;
        let progress = |line: &[u8]| {
            if let Some(line) = progress_line(line, MAX_STEP) {
                events.send(Event::Progress(line));
            }
        };
        let finished = harness_process::run(&start, &limits, &control.stop, &progress)?;
        let git = walls.git.clone();
        drop(walls);
        let reply = reply_of(&finished)?;
        let notes = self.notes(access);
        let notes: Vec<&str> = notes.iter().map(String::as_str).collect();
        Ok(with_notes(reply, &notes, git.notice()))
    }

    fn notes(&self, access: ChatAccess) -> Vec<String> {
        if access == ChatAccess::Read || self.told.swap(true, Ordering::Relaxed) {
            return Vec::new();
        }
        vec![free_commands_note(&self.name)]
    }

    /// The walls of 6.6.4 around the whole harness, with the proxy of the agent.
    fn walls(&self, cwd: &str, access: ChatAccess, tag: &str) -> Result<RunWalls, String> {
        if !self.gate.sandbox.is_on() {
            return Err(NO_SANDBOX.into());
        }
        let guarded = Guarded {
            config_dir: &self.gate.config_dir,
            data_dir: &self.gate.data_dir,
        };
        let shape = Shape {
            chat: access,
            home: HomeWrites::Thrown,
            more_hidden: self
                .gate
                .sandbox
                .home
                .as_deref()
                .map(scan_sockets)
                .unwrap_or_default(),
        };
        let sandbox = self.sandbox();
        command_sandbox::prepare_shaped(&sandbox, &guarded, Path::new(cwd), tag, &shape)
    }

    /// The sandbox of the commands, with the rules of the agent for its proxy.
    fn sandbox(&self) -> CommandSandbox {
        let mut sandbox = self.gate.sandbox.clone();
        sandbox.proxy = Some(harness_proxy(&self.gate, &self.agent_hosts));
        sandbox
    }

    fn start(
        &self,
        walls: &RunWalls,
        cwd: &Path,
        args: &[String],
        stdin: Vec<u8>,
    ) -> Result<Start, String> {
        let program = self.program()?;
        let (launcher, launch_args) = harness_sandbox::launch(&walls.walls, cwd, &program, args)?;
        Ok(Start {
            program: launcher,
            args: launch_args,
            env: self.env.clone(),
            vars: harness_vars(&walls.walls),
            cwd: cwd.to_owned(),
            stdin,
        })
    }

    fn program(&self) -> Result<PathBuf, String> {
        let name = self.command.first().ok_or("The agent has no command.")?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        find_program(name, &path, cfg!(windows))
            .ok_or_else(|| format!("Cannot start {name}: not found on PATH"))
    }

    /// `<program> --version` inside the walls, with no model call. It shows that the
    /// harness starts there.
    pub fn check(&self, cwd: &str) -> Result<Report, String> {
        let walls = self.walls(cwd, ChatAccess::Read, "check")?;
        let start = self.start(
            &walls,
            Path::new(cwd),
            &["--version".to_owned()],
            Vec::new(),
        )?;
        let limits = Limits {
            timeout: CHECK_TIME,
            max_output: MAX_OUTPUT,
            keep: 4096,
        };
        let finished = harness_process::run(
            &start,
            &limits,
            &crate::agent::StopSignal::default(),
            &|_| {},
        )?;
        if cannot_start(finished.code) {
            return Err(format!(
                "{} does not start in the sandbox: {}",
                self.command[0],
                finished.last_error_line().unwrap_or_default()
            ));
        }
        let version = crate::harness_output::clean(&finished.stdout);
        Ok(Report {
            name: format!("{} (kind command)", self.command[0]),
            version: version.lines().next().unwrap_or("?").trim().to_owned(),
            load_session: !self.resume.is_empty(),
            modes: Vec::new(),
            details: self.details(&walls),
        })
    }

    fn details(&self, walls: &RunWalls) -> Vec<String> {
        let home = if walls.walls.home_view.is_some() {
            "the writes of the harness to the home folder go away at the end of each run"
        } else {
            "the home folder is read-only for the harness"
        };
        let network = match harness_proxy(&self.gate, &self.agent_hosts).mode {
            Mode::Public => "any public host, through the proxy of the run".to_owned(),
            Mode::Listed => {
                "only its model hosts, agent_hosts, and the hosts of the sandbox".to_owned()
            }
        };
        vec![
            format!("sandbox: {}; {home}", walls.walls.tool.name()),
            format!("input: {}", harness_args::input_of(&self.command).describe()),
            "levels: at ask it only reads. At auto-edit and full-auto it edits the chat folder and runs its commands with no question, inside the sandbox.".into(),
            format!("network: {network}"),
        ]
    }
}

/// The forwarder gives 126 when it cannot start the program, and a shell gives 127.
fn cannot_start(code: i32) -> bool {
    code == 126 || code == 127
}

/// A harness with no permission channel cannot ask, so at `ask` it changes nothing.
pub fn chat_access(level: Permission) -> ChatAccess {
    match level {
        Permission::Ask => ChatAccess::Read,
        Permission::AutoEdit | Permission::FullAuto => ChatAccess::Write,
    }
}

/// The harness and its commands are one process tree, so they share the rules of the
/// agent. In `strict` mode the hosts of the commands join the list.
pub fn harness_proxy(gate: &Gate, agent_hosts: &[String]) -> ProxySettings {
    let settings = gate.wall.for_agent(Kind::Command, agent_hosts).proxy;
    if settings.mode == Mode::Public {
        return settings;
    }
    let mut names: Vec<String> = agent_hosts.to_vec();
    if let Some(commands) = &gate.sandbox.proxy {
        names.extend(
            commands
                .hosts
                .names()
                .iter()
                .map(|n| String::from_utf8_lossy(n).into_owned()),
        );
    }
    let hosts = HostList::new(Defaults::Off, &names).unwrap_or_default();
    ProxySettings {
        hosts: Arc::new(hosts),
        ..settings
    }
}

/// The proxy, the temp folder, and the caches of the run, and no colors.
fn harness_vars(walls: &command_sandbox::Walls) -> Vec<(String, OsString)> {
    let mut vars = command_env(walls, |_| None);
    vars.push(("XDG_CACHE_HOME".into(), walls.temp.join("cache").into()));
    vars.push(("NO_COLOR".into(), "1".into()));
    vars.push(("TERM".into(), "dumb".into()));
    vars.push(("GNOMISH_RELAY_JOB".into(), "1".into()));
    vars
}

/// A harness can print a partial answer and fail, so a failure shows its last error line.
fn reply_of(finished: &Finished) -> Result<String, String> {
    if finished.code != 0 {
        let why = finished
            .last_error_line()
            .unwrap_or_else(|| "no error text".into());
        return Err(format!(
            "The agent failed (exit status {}): {why}",
            finished.code
        ));
    }
    let reply = reply_text(&finished.stdout, MAX_REPLY);
    if reply.is_empty() {
        return Ok(NO_OUTPUT.into());
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_ask_the_harness_only_reads() {
        assert_eq!(chat_access(Permission::Ask), ChatAccess::Read);
        assert_eq!(chat_access(Permission::AutoEdit), ChatAccess::Write);
        assert_eq!(chat_access(Permission::FullAuto), ChatAccess::Write);
    }

    #[test]
    fn a_failed_harness_gives_its_exit_status_and_last_error_line() {
        let finished = Finished {
            code: 3,
            stdout: b"partial".to_vec(),
            stderr_tail: b"warning\nError: no key\n\n".to_vec(),
        };
        assert_eq!(
            reply_of(&finished).unwrap_err(),
            "The agent failed (exit status 3): Error: no key"
        );
    }

    #[test]
    fn a_harness_with_no_output_says_so() {
        let finished = Finished {
            code: 0,
            stdout: b"\n \x1b[0m\n".to_vec(),
            stderr_tail: Vec::new(),
        };
        assert_eq!(reply_of(&finished).unwrap(), NO_OUTPUT);
    }

    #[test]
    fn the_forwarder_and_the_shell_codes_mean_no_start() {
        assert!(cannot_start(126));
        assert!(cannot_start(127));
        assert!(!cannot_start(1));
    }
}
