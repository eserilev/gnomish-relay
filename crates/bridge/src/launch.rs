//! The requests of the wrapper to the holder of a run (SPEC.md 6.6.4, "One sandbox for
//! each run"). The wrapper sends one command. The holder starts it inside the sandbox,
//! and sends back its output and its exit status. The wrapper prints them as its own.
//!
//! A frame is one byte of kind, four bytes of length (big-endian), and the bytes.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const STDOUT: u8 = b'o';
const STDERR: u8 = b'e';
const EXIT: u8 = b'x';
/// A command of Claude Code is at most 1 MiB (SPEC.md 6.6.3), and its variables are few.
const MAX_REQUEST: u64 = 4 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

/// One command, as the wrapper sends it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Request {
    pub shell: PathBuf,
    pub cwd: PathBuf,
    pub command: String,
    pub env: Vec<(String, String)>,
}

impl Request {
    /// A variable that is not UTF-8 goes in with its bad bytes replaced.
    pub fn new(
        shell: &Path,
        cwd: &Path,
        command: &str,
        env: Vec<(String, std::ffi::OsString)>,
    ) -> Request {
        Request {
            shell: shell.to_owned(),
            cwd: cwd.to_owned(),
            command: command.to_owned(),
            env: env
                .into_iter()
                .map(|(k, v)| (k, v.to_string_lossy().into_owned()))
                .collect(),
        }
    }
}

fn write_frame(out: &mut impl Write, kind: u8, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    out.write_all(&[kind])?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

/// `None` at the end of the stream.
fn read_frame(input: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match input.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    // The holder never sends more than one chunk in a frame.
    if len > CHUNK {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0u8; len];
    input.read_exact(&mut bytes)?;
    Ok(Some((head[0], bytes)))
}

/// The wrapper side: sends `request` to the holder at `socket`, prints the output of the
/// command, and gives its exit status.
pub fn run_in_holder(socket: &Path, request: &Request) -> Result<i32, String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket)
        .map_err(|_| "The sandbox of the run is not running.".to_owned())?;
    let mut line = serde_json::to_vec(request).map_err(|e| format!("bad request: {e}"))?;
    line.push(b'\n');
    stream
        .write_all(&line)
        .map_err(|e| format!("The sandbox of the run did not take the command: {e}"))?;
    print_until_exit(&mut stream)
}

fn print_until_exit(input: &mut impl Read) -> Result<i32, String> {
    let ended = || "The sandbox of the run ended before the command.".to_owned();
    let (mut out, mut err) = (io::stdout(), io::stderr());
    loop {
        let frame = read_frame(input).map_err(|_| ended())?;
        let Some((kind, bytes)) = frame else {
            return Err(ended());
        };
        match kind {
            STDOUT => out.write_all(&bytes).and_then(|()| out.flush()),
            STDERR => err.write_all(&bytes).and_then(|()| err.flush()),
            EXIT => return exit_code(&bytes).ok_or_else(ended),
            _ => return Err(ended()),
        }
        .map_err(|e| format!("cannot print the output: {e}"))?;
    }
}

fn exit_code(bytes: &[u8]) -> Option<i32> {
    Some(i32::from_be_bytes(bytes.try_into().ok()?))
}

/// The holder side: serves each client of `listener` in its own thread.
pub fn serve(listener: &std::os::unix::net::UnixListener) {
    for client in listener.incoming().flatten() {
        std::thread::spawn(move || serve_one(client));
    }
}

fn serve_one(client: std::os::unix::net::UnixStream) {
    let Ok(reader) = client.try_clone() else {
        return;
    };
    let mut line = Vec::new();
    let read = BufReader::new(reader.take(MAX_REQUEST)).read_until(b'\n', &mut line);
    let writer = Arc::new(Mutex::new(client));
    let request = read
        .ok()
        .and_then(|_| serde_json::from_slice::<Request>(&line).ok());
    let code = match request {
        Some(request) => run_request(&request, &writer),
        None => fail(&writer, "bad request to the sandbox of the run"),
    };
    let mut out = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = write_frame(&mut *out, EXIT, &code.to_be_bytes());
}

type Writer = Arc<Mutex<std::os::unix::net::UnixStream>>;

fn fail(writer: &Writer, why: &str) -> i32 {
    let mut out = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = write_frame(
        &mut *out,
        STDERR,
        format!("gnomish-relay sandbox: {why}\n").as_bytes(),
    );
    126
}

/// Runs the command in a process group of its own. A background process of it keeps
/// running after the command ends, until the run ends. When the wrapper goes away first,
/// for example at the timeout of Claude Code, the whole group stops.
fn run_request(request: &Request, writer: &Writer) -> i32 {
    use std::os::unix::process::CommandExt;
    let spawned = Command::new(&request.shell)
        .args(["-c", &request.command])
        .current_dir(&request.cwd)
        .env_clear()
        .envs(request.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return fail(writer, &format!("cannot start the command: {e}")),
    };
    let done = Arc::new(AtomicBool::new(false));
    watch_wrapper(writer, child.id(), &done);
    let pumps = [
        pump(child.stdout.take(), STDOUT, writer),
        pump(child.stderr.take(), STDERR, writer),
    ];
    let code = wait(&mut child);
    done.store(true, Ordering::SeqCst);
    for pump in pumps.into_iter().flatten() {
        let _ = pump.join();
    }
    code
}

fn wait(child: &mut Child) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match child.wait() {
        Ok(status) => status
            .code()
            .or_else(|| status.signal().map(|signal| 128 + signal))
            .unwrap_or(1),
        Err(_) => 1,
    }
}

/// Copies one output of the command into frames of `kind`.
fn pump(
    source: Option<impl Read + Send + 'static>,
    kind: u8,
    writer: &Writer,
) -> Option<std::thread::JoinHandle<()>> {
    let mut source = source?;
    let writer = Arc::clone(writer);
    Some(std::thread::spawn(move || {
        let mut buf = vec![0u8; CHUNK];
        while let Ok(n) = source.read(&mut buf) {
            if n == 0 {
                return;
            }
            let mut out = writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if write_frame(&mut *out, kind, &buf[..n]).is_err() {
                return;
            }
        }
    }))
}

/// The wrapper sends nothing after its request, so a read that ends means that it went
/// away. Then the group of the command stops, unless the command ended first.
fn watch_wrapper(writer: &Writer, pid: u32, done: &Arc<AtomicBool>) {
    let Ok(mut reader) = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .try_clone()
    else {
        return;
    };
    let done = Arc::clone(done);
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        let _ = reader.read(&mut byte);
        if !done.load(Ordering::SeqCst) {
            let group = format!("-{pid}");
            let _ = Command::new("kill")
                .args(["-KILL", "--", &group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn request(command: &str, cwd: &Path) -> Request {
        Request {
            shell: PathBuf::from("/bin/sh"),
            cwd: cwd.to_owned(),
            command: command.to_owned(),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
        }
    }

    /// The frames that the holder sends for `request`.
    fn frames_of(request: &Request) -> Vec<(u8, Vec<u8>)> {
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        std::thread::spawn(move || serve_one(theirs));
        let mut line = serde_json::to_vec(request).unwrap();
        line.push(b'\n');
        ours.write_all(&line).unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = read_frame(&mut ours).unwrap() {
            let exit = frame.0 == EXIT;
            frames.push(frame);
            if exit {
                break;
            }
        }
        frames
    }

    fn joined(frames: &[(u8, Vec<u8>)], kind: u8) -> String {
        let bytes: Vec<u8> = frames
            .iter()
            .filter(|(k, _)| *k == kind)
            .flat_map(|(_, b)| b.clone())
            .collect();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn the_holder_sends_the_output_and_the_exit_status_of_the_command() {
        let dir = tempfile::tempdir().unwrap();

        let frames = frames_of(&request(
            "pwd; echo oops >&2; echo \"$PATH\"; exit 7",
            dir.path(),
        ));

        let out = joined(&frames, STDOUT);
        assert!(out.ends_with("/usr/bin:/bin\n"), "{out}");
        assert!(out.starts_with(&dir.path().canonicalize().unwrap().display().to_string()));
        assert_eq!(joined(&frames, STDERR), "oops\n");
        assert_eq!(frames.last().unwrap(), &(EXIT, 7i32.to_be_bytes().to_vec()));
    }

    #[test]
    fn a_command_that_does_not_start_or_a_bad_request_gives_126() {
        let mut missing = request("true", Path::new("/"));
        missing.shell = PathBuf::from("/no/such/shell");

        let frames = frames_of(&missing);
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        std::thread::spawn(move || serve_one(theirs));
        ours.write_all(b"not json\n").unwrap();
        let bad = std::iter::from_fn(|| read_frame(&mut ours).unwrap())
            .last()
            .unwrap();

        assert_eq!(
            frames.last().unwrap(),
            &(EXIT, 126i32.to_be_bytes().to_vec())
        );
        assert!(joined(&frames, STDERR).contains("cannot start"));
        assert_eq!(bad, (EXIT, 126i32.to_be_bytes().to_vec()));
    }

    #[test]
    fn the_wrapper_prints_the_frames_and_returns_the_exit_status() {
        let mut stream = Vec::new();
        write_frame(&mut stream, STDOUT, b"").unwrap();
        write_frame(&mut stream, EXIT, &3i32.to_be_bytes()).unwrap();

        assert_eq!(print_until_exit(&mut &stream[..]), Ok(3));
        assert!(print_until_exit(&mut &b""[..]).is_err());
        let mut odd = Vec::new();
        write_frame(&mut odd, b'?', b"x").unwrap();
        assert!(print_until_exit(&mut &odd[..]).is_err());
    }

    #[test]
    fn a_holder_that_is_not_there_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        let error = run_in_holder(&dir.path().join("none"), &request("true", dir.path()));

        assert_eq!(error, Err("The sandbox of the run is not running.".into()));
    }

    #[test]
    fn the_holder_runs_each_request_of_its_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launch");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || serve(&listener));

        let code = run_in_holder(&path, &request("exit 4", dir.path()));

        assert_eq!(code, Ok(4));
    }

    #[test]
    fn when_the_wrapper_goes_away_the_group_of_the_command_stops() {
        let dir = tempfile::tempdir().unwrap();
        let beat = dir.path().join("beat");
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        std::thread::spawn(move || serve_one(theirs));
        let command = format!(
            "while true; do echo x >> '{}'; sleep 0.1; done",
            beat.display()
        );
        let mut line = serde_json::to_vec(&request(&command, dir.path())).unwrap();
        line.push(b'\n');
        ours.write_all(&line).unwrap();
        while !beat.exists() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        drop(ours);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let before = std::fs::read_to_string(&beat).unwrap().len();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let after = std::fs::read_to_string(&beat).unwrap().len();

        assert_eq!(before, after, "the command still runs");
    }
}
