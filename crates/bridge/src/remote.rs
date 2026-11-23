//! The game half of the link (SPEC.md 11.7): it sends the relay records to the server
//! half, writes the slot files that come back, and shows the desktop requests of the
//! server.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use protocol::apps::App;
use protocol::link::acked_count;
use protocol::record::Record;
use serde::{Deserialize, Serialize};

use crate::child_link::{ChildLink, Event};
use crate::config::Remote;
use crate::desktop::{Approvals, Prompt};
use crate::game_folders::GameFolders;

use crate::line_choice::{LineFile, with_line};
use crate::link::{
    self, Answered, Hello, Publish, Records, ResetWindow, SavedToken, Source, ToGame, ToServer,
};
use crate::receive::KeySet;
use crate::relay::FrameTag;
use crate::run::{log, now, outbox_records, publish_in_each_game};
use crate::saved;
use crate::slots::{Files, with_bad_tags};

/// A crash of the game half loses the inputs that the server did not ack (SPEC.md 11.7.5).
const MAX_OUTBOX: usize = 1000;
const HELLO_WAIT: Duration = Duration::from_secs(20);
/// A login shell can print a banner before `attach` starts.
const MAX_SKIPPED_LINES: usize = 100;
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_mins(1);
pub const STATUS_FILE: &str = "remote.json";

pub fn next_wait(wait: Duration) -> Duration {
    (wait * 2).min(LONGEST_WAIT)
}

/// What `gnomish-relay status` prints about the server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RemoteStatus {
    pub host: String,
    pub connected: bool,
    pub problem: Option<String>,
}

impl RemoteStatus {
    pub fn line(&self) -> String {
        match (&self.problem, self.connected) {
            (_, true) => format!("Server: {}, connected", self.host),
            (Some(problem), false) => format!("Server: {problem}"),
            (None, false) => format!("Server: connecting to {}…", self.host),
        }
    }
}

pub fn read_status(data: &Path) -> Option<RemoteStatus> {
    let text = std::fs::read_to_string(data.join(STATUS_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

enum Conn {
    Down {
        retry_at: Instant,
    },
    Opening {
        link: ChildLink,
        since: Instant,
        skipped: usize,
    },
    Up {
        link: ChildLink,
    },
}

pub struct RemoteLane {
    host: String,
    words: Vec<String>,
    saved: Vec<saved::Watcher>,
    outbox: VecDeque<ToServer>,
    next_n: u32,
    conn: Conn,
    wait: Duration,
    /// The copies of the waiting requests of the server, with the dialog of this computer.
    copies: Approvals,
    copied: BTreeSet<String>,
    answered: BTreeSet<String>,
    /// Strips with a bad tag since the last good relay strip.
    pub bad_tags: u32,
    data: PathBuf,
    status: Option<RemoteStatus>,
}

impl RemoteLane {
    pub fn new(remote: &Remote, games: &[GameFolders], data: &Path, prompt: Prompt) -> RemoteLane {
        let copies = Approvals::new(data, prompt);
        copies.clear();
        RemoteLane {
            host: remote.host.clone(),
            words: link::ssh_command(&remote.host, &remote.command),
            saved: games
                .iter()
                .map(|g| saved::Watcher::new(g, App::Relay))
                .collect(),
            outbox: VecDeque::new(),
            next_n: 1,
            conn: Conn::Down {
                retry_at: Instant::now(),
            },
            wait: FIRST_WAIT,
            copies,
            copied: BTreeSet::new(),
            answered: BTreeSet::new(),
            bad_tags: 0,
            data: data.to_owned(),
            status: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.conn, Conn::Up { .. })
    }

    pub fn take_records(&mut self, tag: FrameTag, records: &[Record], source: Source) {
        log(&format!(
            "{}: {} records for {}",
            source.word(),
            records.len(),
            self.host
        ));
        self.push(ToServer::Records(Records {
            n: 0,
            tag: link::tag_text(tag),
            now: now(),
            source,
            records: link::records_text(records),
        }));
    }

    pub fn step(&mut self, keys: &KeySet, addons: &[PathBuf], strip_line: &mut LineFile) {
        self.take_saved_variables(keys);
        self.drive(addons, strip_line);
        self.send_answers();
    }

    /// A changed file means a `/reload`: the same steps as the relay lane, on the server.
    fn take_saved_variables(&mut self, keys: &KeySet) {
        let files: Vec<_> = self
            .saved
            .iter_mut()
            .flat_map(saved::Watcher::changed)
            .collect();
        for file in files {
            if let Some(token) = saved::saved_token(&file.text) {
                self.push(ToServer::ResetWindow(ResetWindow {
                    n: 0,
                    token: token.clone(),
                }));
                self.push(ToServer::SavedToken(SavedToken {
                    n: 0,
                    account: file.account.clone(),
                    token,
                }));
            }
            for (tag, records) in outbox_records(App::Relay, &file.text, keys) {
                self.take_records(tag, &records, Source::Outbox);
            }
        }
    }

    fn push(&mut self, input: ToServer) {
        if self.outbox.len() >= MAX_OUTBOX {
            log(&format!(
                "link: {MAX_OUTBOX} inputs wait for {}, so the oldest is dropped",
                self.host
            ));
            self.outbox.pop_front();
        }
        let input = input.with_number(self.next_n);
        self.next_n += 1;
        if let Conn::Up { link } = &self.conn {
            send(link, &input);
        }
        self.outbox.push_back(input);
    }

    fn drive(&mut self, addons: &[PathBuf], strip_line: &mut LineFile) {
        match std::mem::replace(
            &mut self.conn,
            Conn::Down {
                retry_at: Instant::now(),
            },
        ) {
            Conn::Down { retry_at } if Instant::now() < retry_at => {
                self.conn = Conn::Down { retry_at };
            }
            Conn::Down { .. } => self.open(),
            Conn::Opening {
                link,
                since,
                skipped,
            } => self.wait_for_hello(link, since, skipped, addons, strip_line),
            Conn::Up { link } => self.take_messages(link, addons, strip_line),
        }
    }

    fn open(&mut self) {
        match ChildLink::start(&self.words) {
            Ok(link) => {
                send(&link, &ToServer::Hello(Hello::ours(now())));
                self.conn = Conn::Opening {
                    link,
                    since: Instant::now(),
                    skipped: 0,
                };
            }
            Err(e) => self.fail(None, format!("can't reach {}: {e:#}", self.host)),
        }
    }

    fn wait_for_hello(
        &mut self,
        link: ChildLink,
        since: Instant,
        mut skipped: usize,
        addons: &[PathBuf],
        strip_line: &mut LineFile,
    ) {
        let mut events = link.events().into_iter();
        while let Some(event) = events.next() {
            let text = match event {
                Event::Line(text) => text,
                Event::End(_) => {
                    let why = unreachable_line(&self.host, &link.errors_after_end());
                    return self.fail(Some(link), why);
                }
            };
            match serde_json::from_str::<ToGame>(&text) {
                Ok(ToGame::Hello(theirs)) => {
                    let ours = Hello::ours(now());
                    if let Some(problem) = link::hello_problem(&theirs, &ours, &self.host) {
                        return self.fail(Some(link), problem);
                    }
                    self.up(link);
                    // The lines after the hello came in the same read.
                    return self.take_events(events.collect(), addons, strip_line);
                }
                Ok(ToGame::Refused(why)) => return self.fail(Some(link), why),
                Ok(_) | Err(_) => skipped += 1,
            }
            if skipped > MAX_SKIPPED_LINES {
                let why = format!(
                    "{} sent {MAX_SKIPPED_LINES} lines that aren't Gnomish Relay. Check that `gnomish-relay` is on the PATH there.",
                    self.host
                );
                return self.fail(Some(link), why);
            }
        }
        if since.elapsed() > HELLO_WAIT {
            let why = unreachable_line(&self.host, &link.errors());
            return self.fail(Some(link), why);
        }
        self.conn = Conn::Opening {
            link,
            since,
            skipped,
        };
    }

    /// The numbers start again at 1, and the whole outbox goes again (SPEC.md 11.7.5). The
    /// server sends each waiting request again, so the old copies go.
    fn up(&mut self, link: ChildLink) {
        log(&format!("link: connected to {}", self.host));
        self.wait = FIRST_WAIT;
        self.next_n = 1;
        let inputs: Vec<ToServer> = self.outbox.drain(..).collect();
        for input in inputs {
            let input = input.with_number(self.next_n);
            self.next_n += 1;
            send(&link, &input);
            self.outbox.push_back(input);
        }
        self.close_every_copy();
        self.conn = Conn::Up { link };
        self.set_status(true, None);
    }

    fn take_messages(&mut self, link: ChildLink, addons: &[PathBuf], strip_line: &mut LineFile) {
        let events = link.events();
        self.conn = Conn::Up { link };
        self.take_events(events, addons, strip_line);
    }

    fn take_events(&mut self, events: Vec<Event>, addons: &[PathBuf], strip_line: &mut LineFile) {
        for event in events {
            match event {
                Event::Line(text) => match serde_json::from_str::<ToGame>(&text) {
                    Ok(message) => self.take_message(message, addons, strip_line),
                    Err(e) => log(&format!("link: a line of {} is not valid: {e}", self.host)),
                },
                Event::End(_) => return self.lose_link(),
            }
        }
    }

    fn lose_link(&mut self) {
        let Conn::Up { link } = std::mem::replace(
            &mut self.conn,
            Conn::Down {
                retry_at: Instant::now(),
            },
        ) else {
            return;
        };
        let why = format!(
            "lost the link to {}. {}",
            self.host,
            link.errors_after_end()
        );
        self.fail(Some(link), why.trim_end().to_owned());
    }

    fn take_message(&mut self, message: ToGame, addons: &[PathBuf], strip_line: &mut LineFile) {
        match message {
            ToGame::Ack(ack) => self.take_ack(ack.n),
            ToGame::Publish(publish) => self.write_slots(&publish, addons, strip_line),
            ToGame::Request(request) => {
                if let Err(e) = link::check_request(&request) {
                    log(&format!("link: request refused: {e:#}"));
                    return;
                }
                if self.copied.contains(&request.id) {
                    return;
                }
                let copy = request.copy_on(&self.host);
                match self.copies.open_copy(&copy) {
                    Ok(()) => {
                        log(&format!("desktop request {}: from {}", copy.id, self.host));
                        self.copied.insert(copy.id);
                    }
                    Err(e) => log(&format!("desktop request {}: {e:#}", copy.id)),
                }
            }
            ToGame::Closed(closed) => self.close_copy(&closed.id),
            ToGame::Hello(_) | ToGame::Refused(_) => {}
        }
    }

    fn take_ack(&mut self, ack: u32) {
        let numbers: Vec<u32> = self.outbox.iter().filter_map(ToServer::number).collect();
        let done = acked_count(&numbers, ack);
        self.outbox.drain(..done);
    }

    /// The strip line and the bad tag count belong to this computer, so they go on here.
    fn write_slots(&mut self, publish: &Publish, addons: &[PathBuf], strip_line: &mut LineFile) {
        if let Err(e) = link::check_publish(publish) {
            log(&format!("link: publish refused: {e:#}"));
            return;
        }
        let files = match files_of(publish) {
            Ok(files) => files,
            Err(e) => {
                log(&format!("link: publish refused: {e:#}"));
                return;
            }
        };
        let files = Files {
            body: with_bad_tags(
                with_line(files.body, App::Relay, strip_line.choice()),
                App::Relay,
                self.bad_tags,
            ),
            ..files
        };
        publish_in_each_game(addons, App::Relay, &files, &publish.windows);
    }

    fn close_copy(&mut self, id: &str) {
        if self.copied.remove(id) {
            self.copies.close(id);
        }
        self.answered.remove(id);
    }

    fn close_every_copy(&mut self) {
        for id in std::mem::take(&mut self.copied) {
            self.copies.close(&id);
        }
        self.answered.clear();
    }

    /// An answer of the dialog or of `gnomish-relay approve` on this computer.
    fn send_answers(&mut self) {
        let Conn::Up { link } = &self.conn else {
            return;
        };
        for id in self
            .copied
            .difference(&self.answered)
            .cloned()
            .collect::<Vec<_>>()
        {
            let Some(verdict) = self.copies.answer_of(&id) else {
                continue;
            };
            send(
                link,
                &ToServer::Answer(Answered {
                    id: id.clone(),
                    answer: verdict.into(),
                }),
            );
            self.answered.insert(id);
        }
    }

    fn fail(&mut self, link: Option<ChildLink>, why: String) {
        if let Some(link) = link {
            link.stop();
        }
        log(&format!("link: {why}"));
        self.close_every_copy();
        self.conn = Conn::Down {
            retry_at: Instant::now() + self.wait,
        };
        self.wait = next_wait(self.wait);
        self.set_status(false, Some(why));
    }

    fn set_status(&mut self, connected: bool, problem: Option<String>) {
        let status = RemoteStatus {
            host: self.host.clone(),
            connected,
            problem,
        };
        if self.status.as_ref() == Some(&status) {
            return;
        }
        let written = serde_json::to_string(&status)
            .map_err(anyhow::Error::from)
            .and_then(|text| crate::fs_safe::write_private(&self.data, STATUS_FILE, &text));
        if let Err(e) = written {
            log(&format!("cannot write {STATUS_FILE}: {e:#}"));
        }
        self.status = Some(status);
    }
}

/// One hello to the server, for `setup --remote`. The error is the line for the player.
pub fn try_link(remote: &Remote) -> Result<(), String> {
    let words = link::ssh_command(&remote.host, &remote.command);
    let mut link =
        ChildLink::start(&words).map_err(|e| format!("can't reach {}: {e:#}", remote.host))?;
    send(&link, &ToServer::Hello(Hello::ours(now())));
    let start = Instant::now();
    let result = loop {
        // Ended first: a child that wrote its last line and then exited still counts.
        let ended = link.has_ended();
        if let Some(result) = hello_in(&link.events(), &remote.host) {
            break result;
        }
        if ended || start.elapsed() > HELLO_WAIT {
            let errors = if ended {
                link.errors_after_end()
            } else {
                link.errors()
            };
            break Err(unreachable_line(&remote.host, &errors));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    link.stop();
    result
}

fn hello_in(events: &[Event], host: &str) -> Option<Result<(), String>> {
    for event in events {
        let text = match event {
            Event::Line(text) => text,
            Event::End(_) => return None,
        };
        match serde_json::from_str::<ToGame>(text) {
            Ok(ToGame::Hello(theirs)) => {
                let problem = link::hello_problem(&theirs, &Hello::ours(now()), host);
                return Some(problem.map_or(Ok(()), Err));
            }
            Ok(ToGame::Refused(why)) => return Some(Err(why)),
            _ => {}
        }
    }
    None
}

fn unreachable_line(host: &str, errors: &str) -> String {
    if errors.is_empty() {
        return format!(
            "can't reach {host}. Check that `ssh {host}` works, then run `gnomish-relay restart`."
        );
    }
    format!(
        "can't reach {host}: {errors} Check that `ssh {host}` works, then run `gnomish-relay restart`."
    )
}

fn send(link: &ChildLink, message: &ToServer) {
    match link::line_of(message) {
        Ok(text) => link.send(text),
        Err(e) => log(&format!("link: cannot write a line: {e:#}")),
    }
}

fn files_of(publish: &Publish) -> anyhow::Result<Files> {
    Ok(Files {
        body: link::unhex(&publish.body)?,
        restore: link::unhex(&publish.restore)?,
        live: link::unhex(&publish.live)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_before_a_reconnect_doubles_up_to_a_minute() {
        let mut wait = FIRST_WAIT;
        let mut waits = Vec::new();
        for _ in 0..8 {
            waits.push(wait.as_secs());
            wait = next_wait(wait);
        }

        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[test]
    fn the_status_line_names_the_server_and_the_problem() {
        let up = RemoteStatus {
            host: "me@box".into(),
            connected: true,
            problem: None,
        };
        let down = RemoteStatus {
            connected: false,
            problem: Some("can't reach me@box: Permission denied (publickey).".into()),
            ..up.clone()
        };

        assert_eq!(up.line(), "Server: me@box, connected");
        assert_eq!(
            down.line(),
            "Server: can't reach me@box: Permission denied (publickey)."
        );
    }
}
