//! The process of an agent, which is untrusted (SPEC.md 9.4): it gets only the
//! environment variables of the allowlist, and each line from it has a size limit.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::program::find_program;

/// A tool call with a large diff fits in far less.
const MAX_LINE: usize = 8 * 1024 * 1024;
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

const OUTPUT_LIMIT: u64 = 64 * 1024;
const OUTPUT_POLL: Duration = Duration::from_millis(20);

type Line = Result<Value, String>;

/// What a wait for the next line found.
pub enum Next {
    Line(Line),
    /// Nothing came in the wait.
    Quiet,
    /// The agent closed its output.
    Ended,
}

pub struct AgentProcess {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Line>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: Receiver<()>,
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl AgentProcess {
    /// Starts `command` plus `args` in `cwd`.
    pub fn start(
        command: &[String],
        args: &[String],
        env: &[String],
        cwd: &str,
    ) -> Result<AgentProcess, String> {
        let mut child = spawn(command, args, env, cwd, Stdio::piped())?;
        let stdin = child.stdin.take().ok_or("The agent has no stdin.")?;
        let stdout = child.stdout.take().ok_or("The agent has no stdout.")?;
        let stderr_pipe = child.stderr.take().ok_or("The agent has no stderr.")?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (done, stderr_done) = channel();
        keep_tail(stderr_pipe, Arc::clone(&stderr), done);
        Ok(AgentProcess {
            child,
            stdin,
            lines: read_lines(stdout),
            stderr,
            stderr_done,
        })
    }

    /// Writes one message as one line. A closed pipe gives the error of `stopped`.
    pub fn send(&mut self, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| self.stopped())
    }

    pub fn next(&self, wait: Duration) -> Next {
        match self.lines.recv_timeout(wait) {
            Ok(line) => Next::Line(line),
            Err(RecvTimeoutError::Timeout) => Next::Quiet,
            Err(RecvTimeoutError::Disconnected) => Next::Ended,
        }
    }

    /// The error for an agent that went away, with the last line of its stderr.
    pub fn stopped(&mut self) -> String {
        let _ = self.stderr_done.recv_timeout(STDERR_WAIT);
        let tail = self.stderr.lock().map(|t| t.clone()).unwrap_or_default();
        let tail = String::from_utf8_lossy(&tail);
        match tail.lines().rev().find(|l| !l.trim().is_empty()) {
            Some(last) => format!("The agent stopped: {}", cut(last.trim(), 300)),
            None => "The agent stopped.".into(),
        }
    }
}

/// Never through a shell (SPEC.md 6.2, rule 11), and with only the variables of the
/// allowlist (rule 12).
fn spawn(
    command: &[String],
    args: &[String],
    env: &[String],
    cwd: &str,
    stdin: Stdio,
) -> Result<Child, String> {
    let (program, own_args) = command.split_first().ok_or("The agent has no command.")?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = find_program(program, &path, cfg!(windows))
        .ok_or_else(|| format!("Cannot start {program}: not found on PATH"))?;
    let mut child = Command::new(found);
    child
        .args(own_args)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in BASE_ENV
        .iter()
        .copied()
        .chain(env.iter().map(String::as_str))
    {
        if let Some(value) = std::env::var_os(name) {
            child.env(name, value);
        }
    }
    // Tells a hook of the agent that this run comes from the bridge (SPEC.md 10).
    child.env("GNOMISH_RELAY_JOB", "1");
    child
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))
}

/// The end of a short command, such as `claude --version`.
pub struct Output {
    pub success: bool,
    /// At most 64 KiB.
    pub stdout: String,
}

/// Runs a short command to its end, with the rules of `AgentProcess`. After `timeout`,
/// the command is killed and the result is an error.
pub fn output(
    command: &[String],
    args: &[String],
    env: &[String],
    cwd: &str,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = spawn(command, args, env, cwd, Stdio::null())?;
    let stdout = child.stdout.take().ok_or("The agent has no stdout.")?;
    if let Some(stderr) = child.stderr.take() {
        let (done, _) = channel();
        keep_tail(stderr, Arc::new(Mutex::new(Vec::new())), done);
    }
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(OUTPUT_LIMIT).read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(OUTPUT_POLL),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(crate::turn::TIMED_OUT.into());
            }
        }
    };
    let bytes = rx.recv_timeout(STDERR_WAIT).unwrap_or_default();
    Ok(Output {
        success: status.success(),
        stdout: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// The longest start of `text` with at most `max` bytes that ends on a character.
pub fn cut(text: &str, max: usize) -> &str {
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
