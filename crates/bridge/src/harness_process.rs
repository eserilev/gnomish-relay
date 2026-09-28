//! One run of the harness of a `command` agent (SPEC.md 9.2, 9.4): the message goes in,
//! the output comes back as progress lines and as the reply. Stop, the timeout, and the
//! output limit kill the whole process group at once: a harness has no cancel channel.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

use crate::agent::StopSignal;
use crate::harness_output::{Lines, Tail};
use crate::process::allowlisted;
use crate::turn::{STOPPED, TIMED_OUT};

const POLL: Duration = Duration::from_millis(100);
const CHUNK: usize = 64 * 1024;
const STDERR_TAIL: usize = 2048;
/// A program that the harness left in the background can keep the output open. After the
/// harness exits, an output that stays quiet this long ends the run.
const PIPE_GRACE: Duration = Duration::from_secs(1);
/// A progress line is cut far below this anyway.
const MAX_LINE: usize = 4096;
pub const TOO_MUCH: &str = "The agent wrote more output than the limit, so the run stopped.";

/// What starts, and how.
pub struct Start {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The names of the `env` list of the entry, besides the allowlist (SPEC.md 6.2).
    pub env: Vec<String>,
    /// The variables of the bridge, such as the proxy.
    pub vars: Vec<(String, OsString)>,
    pub cwd: PathBuf,
    pub stdin: Vec<u8>,
}

pub struct Limits {
    pub timeout: Duration,
    /// All of stdout, in bytes.
    pub max_output: u64,
    /// The bytes of stdout that the run keeps for the reply.
    pub keep: usize,
}

/// The end of a harness that exited by itself.
pub struct Finished {
    pub code: i32,
    /// At most `Limits::keep` bytes, the end of the output.
    pub stdout: Vec<u8>,
    pub stderr_tail: Vec<u8>,
}

impl Finished {
    /// The last line of stderr that is not blank, for an error text.
    pub fn last_error_line(&self) -> Option<String> {
        let text = crate::harness_output::clean(&self.stderr_tail);
        let last = text.lines().rev().find(|l| !l.trim().is_empty())?;
        Some(crate::process::cut(last.trim(), 300).to_owned())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Out,
    Err,
}

/// `None` at the end of the stream.
type Chunk = (Stream, Option<Vec<u8>>);

/// Runs the harness to its end. Each line of stdout and stderr goes to `progress`.
pub fn run(
    start: &Start,
    limits: &Limits,
    stop: &StopSignal,
    progress: &dyn Fn(&[u8]),
) -> Result<Finished, String> {
    let mut child = spawn(start)?;
    let chunks = pipes(&mut child, start.stdin.clone());
    let deadline = Instant::now() + limits.timeout;
    let mut reading = Reading::default();
    let mut exited = false;
    let mut last_chunk = Instant::now();
    loop {
        if stop.requested() {
            return Err(killed(&mut child, STOPPED));
        }
        if Instant::now() >= deadline {
            return Err(killed(&mut child, TIMED_OUT));
        }
        match chunks.recv_timeout(POLL) {
            Ok(chunk) => {
                reading.take(chunk, limits, progress);
                last_chunk = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if reading.out.total > limits.max_output {
            return Err(killed(&mut child, TOO_MUCH));
        }
        if reading.open == 0 {
            break;
        }
        exited = exited || matches!(child.try_wait(), Ok(Some(_)));
        // Only a quiet output ends the wait: chunks in the channel still count.
        if exited && last_chunk.elapsed() >= PIPE_GRACE {
            break;
        }
    }
    reading.finish(progress);
    let code = wait(&mut child, deadline, stop)?;
    Ok(Finished {
        code,
        stdout: reading.out.bytes().to_vec(),
        stderr_tail: reading.err.bytes().to_vec(),
    })
}

/// What came so far from both streams.
struct Reading {
    out: Tail,
    err: Tail,
    out_lines: Lines,
    err_lines: Lines,
    open: usize,
}

impl Default for Reading {
    fn default() -> Reading {
        Reading {
            out: Tail::default(),
            err: Tail::default(),
            out_lines: Lines::default(),
            err_lines: Lines::default(),
            open: 2,
        }
    }
}

impl Reading {
    fn take(&mut self, (stream, bytes): Chunk, limits: &Limits, progress: &dyn Fn(&[u8])) {
        let Some(bytes) = bytes else {
            self.open -= 1;
            return;
        };
        let lines = match stream {
            Stream::Out => {
                self.out.push(&bytes, limits.keep);
                self.out_lines.push(&bytes, MAX_LINE)
            }
            Stream::Err => {
                self.err.push(&bytes, STDERR_TAIL);
                self.err_lines.push(&bytes, MAX_LINE)
            }
        };
        for line in lines {
            progress(&line);
        }
    }

    fn finish(&mut self, progress: &dyn Fn(&[u8])) {
        for line in [self.out_lines.finish(), self.err_lines.finish()]
            .into_iter()
            .flatten()
        {
            progress(&line);
        }
    }
}

/// Never through a shell. The harness leads a process group of its own, so a kill ends
/// every program that it started.
fn spawn(start: &Start) -> Result<Child, String> {
    let mut command = allowlisted(&start.program, &start.env);
    command
        .args(&start.args)
        .envs(start.vars.iter().map(|(k, v)| (k, v)))
        .current_dir(&start.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .map_err(|e| format!("Cannot start {}: {e}", start.program.display()))
}

/// A thread writes stdin, so a harness that reads it late never blocks the bridge. One
/// thread reads each output.
fn pipes(child: &mut Child, input: Vec<u8>) -> Receiver<Chunk> {
    if let Some(mut stdin) = child.stdin.take() {
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let (tx, rx) = channel();
    for (stream, pipe) in [
        (Stream::Out, child.stdout.take().map(boxed)),
        (Stream::Err, child.stderr.take().map(boxed)),
    ] {
        let tx = tx.clone();
        let Some(mut pipe) = pipe else {
            let _ = tx.send((stream, None));
            continue;
        };
        thread::spawn(move || {
            let mut buf = vec![0u8; CHUNK];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 || tx.send((stream, Some(buf[..n].to_vec()))).is_err() {
                    break;
                }
            }
            let _ = tx.send((stream, None));
        });
    }
    rx
}

fn boxed(pipe: impl Read + Send + 'static) -> Box<dyn Read + Send> {
    Box::new(pipe)
}

fn wait(child: &mut Child, deadline: Instant, stop: &StopSignal) -> Result<i32, String> {
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            kill_group(child);
            return Ok(exit_code(status));
        }
        if stop.requested() {
            return Err(killed(child, STOPPED));
        }
        if Instant::now() >= deadline {
            return Err(killed(child, TIMED_OUT));
        }
        thread::sleep(POLL);
    }
}

fn killed(child: &mut Child, why: &str) -> String {
    kill_group(child);
    let _ = child.kill();
    let _ = child.wait();
    why.to_owned()
}

/// Ends the programs that the harness left behind, also after the harness exited.
#[cfg(unix)]
fn kill_group(child: &Child) {
    let group = format!("-{}", child.id());
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_group(_child: &Child) {}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}
