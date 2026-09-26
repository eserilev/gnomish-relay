//! The story program of Timeways and its life cycle (SPEC.md 9.8, and 9.7 decisions 8,
//! 9, and 16). It runs in its sandbox, speaks the app protocol, and starts again after
//! a crash. The addon and the story program are untrusted: each line from either one
//! gets the checks of `app_protocol`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::addon_lines::{AddonLine, Refused, forwarded_line, read_batch};
use crate::app_protocol::{
    self, Answer, BadLine, CallId, CompanionCheck, FromStory, RequestId, batch_end_line,
    hello_line, model_answered_line, model_failed_line, reply_text,
};
use crate::config::StoryConfig;
use crate::model::{ModelCalls, ModelSpec};
use crate::process::{self, RawLine, TooLong};
use crate::run::log;
use crate::story_sandbox::{self, Sandbox, Walls};
use crate::timeways::StoryMessage;

pub const UPDATE_BRIDGE: &str = "Update the desktop program: gnomish-relay update.";
pub const UPDATE_TIMEWAYS: &str = "Update Timeways.";
pub const STOPPED: &str = "The Timeways story program stopped.";
pub const NO_ANSWER: &str = "The Timeways story program did not answer in time.";
pub const TOO_LONG: &str = "The Timeways answer is too long for the game.";
pub const OUT_OF_ORDER: &str = "The lines of the batch are in the wrong order.";
pub const BAD_CHARACTER: &str = "The realm or the name of the character is too long.";
pub const NO_SANDBOX: &str = "The Timeways story program runs with no sandbox here.";
/// The folder of the story program, inside the Timeways folder of the data folder.
pub const STORY_DIR: &str = "story";

const HANDSHAKE: Duration = Duration::from_secs(10);
const MAX_BAD_LINES: usize = 10;
/// A flood of lines cannot hold up the main loop.
const MAX_LINES_PER_STEP: usize = 64;

/// How to start the story program.
#[derive(Clone, Debug)]
pub struct StorySpec {
    /// An absolute path. The bridge never looks it up on `PATH`.
    pub program: PathBuf,
    pub args: Vec<String>,
    pub walls: Walls,
    pub sandbox: Sandbox,
    pub timeout: Duration,
    pub model: ModelSpec,
}

impl StorySpec {
    /// Makes the folder of the story program and finds the sandbox of this computer. The
    /// command line is `<program> <lore pack> <story folder>`. The lore pack is the one
    /// file that the sandbox shows even inside a hidden folder. With no program in the
    /// config, there is nothing to start.
    pub fn from_config(
        config: &StoryConfig,
        config_dir: &Path,
        data: &Path,
        home: &Path,
    ) -> anyhow::Result<Option<StorySpec>> {
        let Some(story) = &config.program else {
            return Ok(None);
        };
        let folder = data.join(crate::run::TIMEWAYS_DIR).join(STORY_DIR);
        make_story_folder(&folder)?;
        let pack = &story.lore_pack;
        let walls = story_sandbox::walls(&folder, config_dir, data, home, &[pack]);
        let args = [pack, &walls.folder].map(|p| p.to_string_lossy().into_owned());
        Ok(Some(StorySpec {
            program: story.program.clone(),
            args: args.to_vec(),
            walls,
            sandbox: story_sandbox::detect(),
            timeout: config.timeout,
            model: config.model.clone(),
        }))
    }
}

/// A batch and its reply: the done text, or the error text.
pub type Reply = (StoryMessage, Result<String, String>);

/// The wait before the next start. It doubles after each stop, up to a minute, and
/// starts again at one second after a program that ran for a minute.
#[derive(Debug)]
pub struct Backoff {
    next: Duration,
}

const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_mins(1);

impl Backoff {
    pub fn new() -> Backoff {
        Backoff { next: FIRST_WAIT }
    }

    pub fn after_stop(&mut self, ran_for: Duration) -> Duration {
        if ran_for >= LONGEST_WAIT {
            self.next = FIRST_WAIT;
        }
        let wait = self.next;
        self.next = (wait * 2).min(LONGEST_WAIT);
        wait
    }
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff::new()
    }
}

/// How long a batch of game events waits for `events_seen`. It never ends in an error.
const EVENTS_WAIT: Duration = Duration::from_mins(1);

/// A batch of the addon and its checked lines.
struct Waiting {
    message: StoryMessage,
    lines: Vec<AddonLine>,
    deadline: Instant,
}

impl Waiting {
    /// A batch with a `lore_asked` or a `journal_asked` line. Any other batch holds only
    /// game events, and waits for `events_seen`.
    fn wants_reply(&self) -> bool {
        self.lines.iter().any(AddonLine::wants_reply)
    }
}

enum Life {
    Down {
        until: Instant,
    },
    Starting {
        process: StoryProcess,
        deadline: Instant,
    },
    Ready {
        process: StoryProcess,
    },
    /// The versions do not match. A restart cannot help, so the program stays stopped.
    Refused(&'static str),
}

/// The one-time warning of a computer with no sandbox (SPEC.md 6.6.4).
#[derive(PartialEq, Eq)]
enum Warning {
    Due,
    Given,
}

pub struct Story {
    spec: StorySpec,
    life: Life,
    backoff: Backoff,
    started: Instant,
    /// Batches that wait for a program that is ready.
    queue: Vec<Waiting>,
    /// Forwarded batches that wait for their answer. A waiting batch never holds up the
    /// next ones: answers match by id.
    sent: BTreeMap<RequestId, Waiting>,
    next_id: u64,
    bad_lines: usize,
    replies: Vec<Reply>,
    warning: Warning,
    /// They belong to no batch, and end when the program stops.
    models: ModelCalls,
}

impl Story {
    /// The program starts at the first `step`.
    pub fn new(spec: StorySpec) -> Story {
        let warning = if spec.sandbox == Sandbox::None {
            log(&format!("timeways: {NO_SANDBOX}"));
            Warning::Due
        } else {
            Warning::Given
        };
        Story {
            life: Life::Down {
                until: Instant::now(),
            },
            backoff: Backoff::new(),
            started: Instant::now(),
            queue: Vec::new(),
            sent: BTreeMap::new(),
            next_id: 1,
            bad_lines: 0,
            replies: Vec::new(),
            warning,
            models: ModelCalls::new(&spec.model),
            spec,
        }
    }

    /// A bad line of the addon is dropped. A batch out of order, or with a bad character
    /// line, gets an error, and none of its lines go on.
    pub fn send(&mut self, message: StoryMessage) {
        let batch = match read_batch(&message.text) {
            Ok(batch) => batch,
            Err(refused) => {
                log(&format!(
                    "timeways: refused batch #{}: {refused:?}",
                    message.id.0
                ));
                self.answer_error(message, refusal_text(refused));
                return;
            }
        };
        for bad in batch.dropped {
            log(&format!(
                "timeways: dropped a bad line of the addon: {bad:?}"
            ));
        }
        let waiting = Waiting {
            deadline: Instant::now() + self.wait_for(&batch.lines),
            message,
            lines: batch.lines,
        };
        if let Life::Refused(reason) = self.life {
            self.end_unanswered(waiting, reason);
            return;
        }
        self.queue.push(waiting);
    }

    fn wait_for(&self, lines: &[AddonLine]) -> Duration {
        if lines.iter().any(AddonLine::wants_reply) {
            self.spec.timeout
        } else {
            EVENTS_WAIT.min(self.spec.timeout)
        }
    }

    pub fn step(&mut self) {
        let life = std::mem::replace(
            &mut self.life,
            Life::Down {
                until: Instant::now(),
            },
        );
        self.life = self.advance(life);
        self.forward_queue();
        self.expire_queue();
    }

    pub fn take_replies(&mut self) -> Vec<Reply> {
        std::mem::take(&mut self.replies)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.life, Life::Ready { .. })
    }

    fn advance(&mut self, life: Life) -> Life {
        match life {
            Life::Down { until } if Instant::now() >= until => self.start(),
            Life::Starting { process, deadline } => self.handshake(process, deadline),
            Life::Ready { process } => self.serve(process),
            other => other,
        }
    }

    fn start(&mut self) -> Life {
        self.started = Instant::now();
        self.bad_lines = 0;
        let process = match StoryProcess::start(&self.spec) {
            Ok(process) => process,
            Err(e) => {
                log(&format!("timeways: {e}"));
                return self.down();
            }
        };
        process.write(hello_line());
        log(&format!(
            "timeways: story program started ({})",
            self.spec.sandbox.name()
        ));
        Life::Starting {
            process,
            deadline: Instant::now() + HANDSHAKE.min(self.spec.timeout),
        }
    }

    fn handshake(&mut self, process: StoryProcess, deadline: Instant) -> Life {
        for _ in 0..MAX_LINES_PER_STEP {
            let bytes = match process.next_line() {
                Incoming::Nothing => break,
                Incoming::Ended => return self.crashed(process),
                Incoming::Line(line) => line,
            };
            match read(bytes) {
                Ok(FromStory::Hello { protocol }) => return self.check_version(process, protocol),
                Ok(_) => self.bad_line("a line before the hello"),
                Err(bad) => self.bad_line(&format!("{bad:?}")),
            }
            if self.bad_lines > MAX_BAD_LINES {
                return self.stop(process, "too many bad lines");
            }
        }
        if Instant::now() >= deadline {
            return self.stop(process, "no hello in time");
        }
        Life::Starting { process, deadline }
    }

    fn check_version(&mut self, process: StoryProcess, protocol: u32) -> Life {
        if protocol == app_protocol::VERSION {
            log("timeways: story program ready");
            return Life::Ready { process };
        }
        let reason = if protocol > app_protocol::VERSION {
            UPDATE_BRIDGE
        } else {
            UPDATE_TIMEWAYS
        };
        log(&format!(
            "timeways: the story program speaks protocol {protocol}, the bridge speaks {}: {reason}",
            app_protocol::VERSION
        ));
        drop(process);
        for waiting in self.take_all_waiting() {
            self.end_unanswered(waiting, reason);
        }
        Life::Refused(reason)
    }

    /// A batch with a reply line that gets no answer in time means a hang. A batch of
    /// events that gets no answer in time just ends.
    fn serve(&mut self, process: StoryProcess) -> Life {
        for _ in 0..MAX_LINES_PER_STEP {
            let bytes = match process.next_line() {
                Incoming::Nothing => break,
                Incoming::Ended => return self.crashed(process),
                Incoming::Line(line) => line,
            };
            match read(bytes) {
                Ok(FromStory::Answer {
                    id,
                    answer,
                    companion,
                }) => self.take_answer(id, answer.as_ref(), companion),
                Ok(FromStory::ModelCall { call, prompt }) => {
                    self.start_model_call(&process, call, prompt);
                }
                Ok(FromStory::Hello { .. }) => self.bad_line("a second hello"),
                Err(bad) => self.bad_line(&format!("{bad:?}")),
            }
            if self.bad_lines > MAX_BAD_LINES {
                return self.stop(process, "too many bad lines");
            }
        }
        self.answer_model_calls(&process);
        let now = Instant::now();
        let hang = self
            .sent
            .values()
            .any(|w| w.wants_reply() && now >= w.deadline);
        self.expire_sent();
        if hang {
            return self.stop(process, "no answer in time");
        }
        Life::Ready { process }
    }

    /// `None` is an answer line over its limit: an error, never a cut line. An answer for
    /// an id that already ended is late, and goes. An id that the bridge never gave is a
    /// bad line.
    fn take_answer(&mut self, id: RequestId, answer: Option<&Answer>, companion: CompanionCheck) {
        if companion == CompanionCheck::Dropped {
            log(&format!(
                "timeways: dropped the companion line of #{}",
                id.0
            ));
        }
        let Some(waiting) = self.sent.remove(&id) else {
            if id.0 < self.next_id {
                log(&format!("timeways: dropped a late answer for #{}", id.0));
            } else {
                self.bad_line(&format!("an answer for unknown id {}", id.0));
            }
            return;
        };
        let Some(answer) = answer else {
            log(&format!("timeways: the answer for #{} is too long", id.0));
            self.end_unanswered(waiting, TOO_LONG);
            return;
        };
        if answer.is_events_seen() == waiting.wants_reply() {
            self.bad_line(&format!("an answer of the wrong type for #{}", id.0));
            self.sent.insert(id, waiting);
            return;
        }
        let note = self.take_warning().then_some(NO_SANDBOX);
        match reply_text(answer, note) {
            Some(text) => self.replies.push((waiting.message, Ok(text))),
            None => self.end_unanswered(waiting, TOO_LONG),
        }
    }

    /// A call that cannot run gets `model_failed` at once.
    fn start_model_call(&mut self, process: &StoryProcess, call: CallId, prompt: String) {
        if let Err(refused) = self.models.start(call, prompt) {
            log(&format!(
                "timeways: model call {} failed at once: {refused:?}",
                call.0
            ));
            process.write(model_failed_line(call));
        }
    }

    fn answer_model_calls(&mut self, process: &StoryProcess) {
        for (call, answer) in self.models.finished() {
            match answer {
                Ok(text) => process.write(model_answered_line(call, &text)),
                Err(why) => {
                    log(&format!("timeways: model call {} failed: {why}", call.0));
                    process.write(model_failed_line(call));
                }
            }
        }
    }

    fn bad_line(&mut self, what: &str) {
        self.bad_lines += 1;
        log(&format!("timeways: skipped a bad line: {what}"));
    }

    fn crashed(&mut self, mut process: StoryProcess) -> Life {
        let reason = process.stderr_tail();
        self.stop(process, &reason)
    }

    /// Kills the process group. Each sent batch ends. A batch that is not sent yet waits
    /// for the next start.
    fn stop(&mut self, process: StoryProcess, reason: &str) -> Life {
        log(&format!("timeways: story program stopped: {reason}"));
        self.models.stop_all();
        drop(process);
        for (_, waiting) in std::mem::take(&mut self.sent) {
            self.end_unanswered(waiting, STOPPED);
        }
        self.down()
    }

    fn down(&mut self) -> Life {
        let wait = self.backoff.after_stop(self.started.elapsed());
        log(&format!("timeways: story program starts again in {wait:?}"));
        Life::Down {
            until: Instant::now() + wait,
        }
    }

    fn forward_queue(&mut self) {
        let Life::Ready { process } = &self.life else {
            return;
        };
        for waiting in std::mem::take(&mut self.queue) {
            let id = RequestId(self.next_id);
            self.next_id += 1;
            for line in &waiting.lines {
                process.write(forwarded_line(id, line));
            }
            // A reply line ends its batch itself. So each batch gets one answer line.
            if !waiting.wants_reply() {
                process.write(batch_end_line(id));
            }
            self.sent.insert(id, waiting);
        }
    }

    fn expire_queue(&mut self) {
        let now = Instant::now();
        let (late, waiting): (Vec<Waiting>, _) = std::mem::take(&mut self.queue)
            .into_iter()
            .partition(|w| now >= w.deadline);
        self.queue = waiting;
        for waiting in late {
            self.end_unanswered(waiting, NO_ANSWER);
        }
    }

    fn expire_sent(&mut self) {
        let now = Instant::now();
        let late: Vec<RequestId> = self
            .sent
            .iter()
            .filter(|(_, w)| now >= w.deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            if let Some(waiting) = self.sent.remove(&id) {
                self.end_unanswered(waiting, NO_ANSWER);
            }
        }
    }

    fn take_all_waiting(&mut self) -> Vec<Waiting> {
        let mut all = std::mem::take(&mut self.queue);
        all.extend(std::mem::take(&mut self.sent).into_values());
        all
    }

    /// A batch of game events ends done and empty, never with an error.
    fn end_unanswered(&mut self, waiting: Waiting, error: &str) {
        if waiting.wants_reply() {
            self.answer_error(waiting.message, error);
        } else {
            self.replies.push((waiting.message, Ok(String::new())));
        }
    }

    /// An error text is plain, so the warning goes on a line of its own after it.
    fn answer_error(&mut self, message: StoryMessage, text: &str) {
        let text = if self.take_warning() {
            format!("{text}\n{NO_SANDBOX}")
        } else {
            text.to_owned()
        };
        self.replies.push((message, Err(text)));
    }

    /// True once: the first answer that shows text carries the warning.
    fn take_warning(&mut self) -> bool {
        let due = self.warning == Warning::Due;
        self.warning = Warning::Given;
        due
    }
}

fn refusal_text(refused: Refused) -> &'static str {
    match refused {
        Refused::Order => OUT_OF_ORDER,
        Refused::Character => BAD_CHARACTER,
    }
}

/// Mode 0700: the story folder holds the story of each character.
fn make_story_folder(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn read(line: RawLine) -> Result<FromStory, BadLine> {
    let bytes = line.map_err(|TooLong| BadLine::TooLong)?;
    app_protocol::read_line(&bytes)
}

enum Incoming {
    Line(RawLine),
    Nothing,
    Ended,
}

/// The running story program. Dropping it kills its process group.
struct StoryProcess {
    child: Child,
    to_story: Sender<String>,
    lines: Receiver<RawLine>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: Receiver<()>,
}

const STDERR_WAIT: Duration = Duration::from_millis(500);

impl StoryProcess {
    /// No shell, the environment allowlist of SPEC.md 6.2, and its own folder as the
    /// working folder.
    fn start(spec: &StorySpec) -> Result<StoryProcess, String> {
        let (program, args) =
            story_sandbox::command_line(&spec.sandbox, &spec.walls, &spec.program, &spec.args);
        make_story_folder(&spec.walls.folder)
            .map_err(|e| format!("cannot make {}: {e}", spec.walls.folder.display()))?;
        let mut command = process::allowlisted(&program, &[]);
        command
            .args(args)
            .current_dir(&spec.walls.folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        own_process_group(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", program.display()))?;
        let (Some(stdin), Some(stdout), Some(stderr_pipe)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err("the story program has no pipes".into());
        };
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (done, stderr_done) = channel();
        process::keep_tail(stderr_pipe, Arc::clone(&stderr), done);
        Ok(StoryProcess {
            child,
            to_story: write_lines(stdin),
            lines: process::read_raw_lines(stdout, app_protocol::MAX_LINE),
            stderr,
            stderr_done,
        })
    }

    /// A program that stopped shows up as the end of its output.
    fn write(&self, line: String) {
        let _ = self.to_story.send(line);
    }

    fn next_line(&self) -> Incoming {
        match self.lines.try_recv() {
            Ok(line) => Incoming::Line(line),
            Err(TryRecvError::Empty) => Incoming::Nothing,
            Err(TryRecvError::Disconnected) => Incoming::Ended,
        }
    }

    /// The last line of stderr, for the log. The text goes through the log escape.
    fn stderr_tail(&mut self) -> String {
        let _ = self.stderr_done.recv_timeout(STDERR_WAIT);
        let tail = self.stderr.lock().map(|t| t.clone()).unwrap_or_default();
        let tail = String::from_utf8_lossy(&tail);
        match tail.lines().rev().find(|l| !l.trim().is_empty()) {
            Some(last) => format!("it ended: {}", process::cut(last.trim(), 300)),
            None => "it ended".into(),
        }
    }
}

impl Drop for StoryProcess {
    fn drop(&mut self) {
        kill_group(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A thread writes, so a program that does not read its input never blocks the bridge.
fn write_lines(mut stdin: std::process::ChildStdin) -> Sender<String> {
    use std::io::Write;
    let (tx, rx) = channel::<String>();
    thread::spawn(move || {
        for line in rx {
            if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() {
                return;
            }
        }
    });
    tx
}

/// The story program leads its own process group, so a kill of the group also stops
/// its children.
#[cfg(unix)]
fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_command: &mut Command) {}

/// The group leader is not reaped yet, so its id cannot name another group.
#[cfg(unix)]
fn kill_group(pid: u32) {
    let _ = quiet(Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]));
}

#[cfg(windows)]
fn kill_group(pid: u32) {
    let _ = quiet(Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]));
}

fn quiet(command: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StoryProgram;

    #[test]
    fn a_story_section_with_no_program_gives_nothing_to_start() {
        let root = tempfile::tempdir().unwrap();
        let config = StoryConfig {
            program: None,
            timeout: Duration::from_secs(9),
            model: crate::model::ModelSpec::none(),
        };
        let spec = StorySpec::from_config(&config, root.path(), root.path(), root.path());
        assert!(spec.unwrap().is_none());
        assert!(!root.path().join("timeways").exists(), "no story folder");
    }

    #[test]
    fn the_spec_of_the_config_passes_the_lore_pack_and_hides_the_config_and_data() {
        let root = tempfile::tempdir().unwrap();
        let (config_dir, data) = (root.path().join("config"), root.path().join("data"));
        std::fs::create_dir_all(&config_dir).unwrap();
        let pack = root.path().join("lore.sqlite");
        std::fs::write(&pack, "").unwrap();
        let config = StoryConfig {
            program: Some(StoryProgram {
                program: PathBuf::from("/opt/timeways-story"),
                lore_pack: pack.clone(),
            }),
            timeout: Duration::from_secs(9),
            model: crate::model::ModelSpec::none(),
        };

        let spec = StorySpec::from_config(&config, &config_dir, &data, root.path())
            .unwrap()
            .unwrap();

        let real = |p: &Path| p.canonicalize().unwrap();
        let folder = real(&data.join("timeways/story"));
        assert_eq!(spec.walls.folder, folder);
        assert!(spec.walls.hidden.contains(&real(&config_dir)));
        assert!(spec.walls.hidden.contains(&real(&data)));
        assert_eq!(spec.walls.readable, [real(&pack)]);
        assert_eq!(spec.program, PathBuf::from("/opt/timeways-story"));
        assert_eq!(
            spec.args,
            [pack.to_string_lossy(), folder.to_string_lossy()]
        );
        assert_eq!(spec.timeout, Duration::from_secs(9));
    }

    #[cfg(unix)]
    #[test]
    fn the_story_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("timeways/story");

        make_story_folder(&folder).unwrap();

        let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn the_backoff_doubles_from_one_second_up_to_one_minute() {
        let mut backoff = Backoff::new();
        let short = Duration::from_secs(2);
        let waits: Vec<u64> = (0..8)
            .map(|_| backoff.after_stop(short).as_secs())
            .collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[test]
    fn the_backoff_starts_again_after_a_program_that_ran_for_a_minute() {
        let mut backoff = Backoff::new();
        for _ in 0..5 {
            backoff.after_stop(Duration::ZERO);
        }
        assert_eq!(backoff.after_stop(Duration::from_mins(1)), FIRST_WAIT);
        assert_eq!(backoff.after_stop(Duration::ZERO), Duration::from_secs(2));
    }
}
