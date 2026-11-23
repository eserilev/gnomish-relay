//! The child process that carries the link to the server: ssh, or the `command` of
//! `[remote]` (SPEC.md 11.7.4).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::link;
use crate::process_group;

/// An error names the last lines of ssh, for example "Permission denied (publickey)".
const KEPT_ERROR_LINES: usize = 5;
/// The end of stdout can come before the last line of stderr.
const ERRORS_WAIT: Duration = Duration::from_secs(1);

pub enum Event {
    Line(String),
    /// The end of the output, with the error of the read, if any.
    End(Option<String>),
}

pub struct ChildLink {
    child: Child,
    events: Receiver<Event>,
    out: Sender<String>,
    errors: Arc<Mutex<VecDeque<String>>>,
    errors_done: Receiver<()>,
}

impl ChildLink {
    /// The child leads its own process group, so the exit of the desktop app ends it.
    pub fn start(words: &[String]) -> Result<ChildLink> {
        let Some((program, args)) = words.split_first() else {
            bail!("[remote] command is empty");
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process_group::spawn(&mut command)
            .with_context(|| format!("cannot start {program}"))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = process_group::end(&mut child);
            bail!("{program} has no pipes");
        };
        let (event_tx, events) = channel();
        std::thread::spawn(move || read_lines(BufReader::new(stdout), &event_tx));
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        let kept = Arc::clone(&errors);
        let (done, errors_done) = channel();
        std::thread::spawn(move || {
            keep_errors(BufReader::new(stderr), &kept);
            let _ = done.send(());
        });
        let (out, lines) = channel::<String>();
        std::thread::spawn(move || write_lines(stdin, &lines));
        Ok(ChildLink {
            child,
            events,
            out,
            errors,
            errors_done,
        })
    }

    pub fn send(&self, line: String) {
        let _ = self.out.send(line);
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// After the end of stdout: the last line of stderr can still be on its way.
    pub fn errors_after_end(&self) -> String {
        let _ = self.errors_done.recv_timeout(ERRORS_WAIT);
        self.errors()
    }

    /// The last lines that the child wrote to stderr, on one line, with no wait.
    pub fn errors(&self) -> String {
        let lines = self.errors.lock().unwrap_or_else(PoisonError::into_inner);
        lines.iter().cloned().collect::<Vec<_>>().join(" ")
    }

    /// True once the child exited, for example ssh after a refused login.
    pub fn has_ended(&mut self) -> bool {
        process_group::has_exited(&mut self.child)
    }

    pub fn stop(mut self) {
        let _ = process_group::end(&mut self.child);
    }
}

fn read_lines(mut reader: impl BufRead, events: &Sender<Event>) {
    loop {
        match link::read_line(&mut reader) {
            Ok(Some(line)) => {
                if events.send(Event::Line(line)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = events.send(Event::End(None));
                return;
            }
            Err(e) => {
                let _ = events.send(Event::End(Some(format!("{e:#}"))));
                return;
            }
        }
    }
}

fn keep_errors(reader: impl BufRead, kept: &Mutex<VecDeque<String>>) {
    for line in reader.lines().map_while(Result::ok) {
        let mut lines = kept.lock().unwrap_or_else(PoisonError::into_inner);
        lines.push_back(line.trim().to_owned());
        while lines.len() > KEPT_ERROR_LINES {
            lines.pop_front();
        }
    }
}

fn write_lines(mut stdin: impl Write, lines: &Receiver<String>) {
    for line in lines {
        if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() {
            return;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Instant;

    use super::*;

    fn words(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into()]
    }

    fn events_until_end(link: &ChildLink) -> Vec<String> {
        let start = Instant::now();
        let mut lines = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            for event in link.events() {
                match event {
                    Event::Line(text) => lines.push(text),
                    Event::End(_) => return lines,
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no end");
    }

    #[test]
    fn a_line_sent_to_the_child_comes_back_through_its_output() {
        let link = ChildLink::start(&words("read line; echo \"got $line\"")).unwrap();

        link.send("hello\n".into());

        assert_eq!(events_until_end(&link), ["got hello"]);
        link.stop();
    }

    #[test]
    fn a_failed_connect_keeps_the_last_lines_of_its_errors() {
        let link = ChildLink::start(&words(
            "echo one >&2; echo 'Permission denied (publickey).' >&2; exit 255",
        ))
        .unwrap();

        events_until_end(&link);

        assert_eq!(
            link.errors_after_end(),
            "one Permission denied (publickey)."
        );
        link.stop();
    }

    #[test]
    fn a_missing_program_is_an_error_at_the_start() {
        assert!(ChildLink::start(&["/no/such/program".to_owned()]).is_err());
        assert!(ChildLink::start(&[]).is_err());
    }
}
