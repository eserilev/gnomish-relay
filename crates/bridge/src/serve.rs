//! The server half of the link: the inputs of the game half, the acks, the publishes,
//! and the desktop requests (SPEC.md 11.7).

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, Sender};

use anyhow::Result;
use protocol::record::Record;

use crate::desktop::Approvals;
use crate::ids::hex;
use crate::link::{
    self, Ack, Closed, Publish, Records, Request, ResetWindow, SavedToken, Source, ToGame, ToServer,
};
use crate::run::log;
use crate::slots::Files;

/// What the connection threads tell the main loop.
pub enum Inbound {
    /// A game half passed the hello. It takes over from the one before.
    Connected {
        conn: u64,
        out: Sender<String>,
    },
    Message {
        conn: u64,
        message: ToServer,
    },
    Gone {
        conn: u64,
    },
}

/// An input of the game half, for the relay lane.
pub enum Input {
    Records {
        tag: [u8; 8],
        records: Vec<Record>,
        now: u32,
        source: Source,
    },
    ResetWindow {
        token: String,
    },
    SavedToken {
        account: String,
        token: String,
    },
}

/// What to do with an input of number `n` after the input `last`.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Take,
    /// A copy of an input that this connection took already.
    Skip,
    /// An input after a gap. An ack of a later number would also ack the gap.
    Gap,
}

pub fn next_input(last: u32, n: u32) -> Next {
    if n <= last {
        Next::Skip
    } else if Some(n) == last.checked_add(1) {
        Next::Take
    } else {
        Next::Gap
    }
}

pub struct ServeLink {
    inbound: Receiver<Inbound>,
    current: Option<(u64, Sender<String>)>,
    last_n: u32,
    acked: u32,
    /// `Prompt::OnGameComputer`: the request files are the only truth, and the game half
    /// shows them.
    approvals: Approvals,
    sent: BTreeSet<String>,
    publish_due: bool,
}

impl ServeLink {
    pub fn new(inbound: Receiver<Inbound>, approvals: Approvals) -> ServeLink {
        ServeLink {
            inbound,
            current: None,
            last_n: 0,
            acked: 0,
            approvals,
            sent: BTreeSet::new(),
            publish_due: false,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.current.is_some()
    }

    /// The inputs that came since the last step, in their order. Answers go straight into
    /// the request folder.
    pub fn take_inputs(&mut self) -> Vec<Input> {
        let mut inputs = Vec::new();
        while let Ok(inbound) = self.inbound.try_recv() {
            match inbound {
                Inbound::Connected { conn, out } => self.connect(conn, out),
                Inbound::Gone { conn } => self.forget(conn),
                Inbound::Message { conn, message } if self.is_current(conn) => {
                    inputs.extend(self.take_message(message));
                }
                Inbound::Message { .. } => {}
            }
        }
        inputs
    }

    fn is_current(&self, conn: u64) -> bool {
        self.current.as_ref().is_some_and(|(c, _)| *c == conn)
    }

    /// The numbers start again, every waiting request goes again, and the slot files
    /// go again.
    fn connect(&mut self, conn: u64, out: Sender<String>) {
        if self.current.is_some() {
            log("link: another game computer took over");
        }
        log("link: the game computer is connected");
        self.current = Some((conn, out));
        self.last_n = 0;
        self.acked = 0;
        self.sent.clear();
        self.publish_due = true;
    }

    fn forget(&mut self, conn: u64) {
        if self.is_current(conn) {
            log("link: the game computer is gone");
            self.current = None;
        }
    }

    fn take_message(&mut self, message: ToServer) -> Option<Input> {
        if let ToServer::Answer(answered) = &message {
            self.take_answer(&answered.id, answered.answer);
            return None;
        }
        let n = message.number()?;
        match next_input(self.last_n, n) {
            Next::Take => self.last_n = n,
            Next::Skip => return None,
            Next::Gap => {
                log(&format!(
                    "link: input {n} came after {}, so it waits for the ones between",
                    self.last_n
                ));
                return None;
            }
        }
        match input_of(message) {
            Ok(input) => input,
            Err(e) => {
                log(&format!("link: input {n} dropped: {e:#}"));
                None
            }
        }
    }

    fn take_answer(&self, id: &str, answer: link::Answer) {
        match self.approvals.answer(id, answer.into()) {
            Ok(()) => log(&format!(
                "desktop request {id}: answered on the game computer"
            )),
            Err(e) => log(&format!("desktop request {id}: {e:#}")),
        }
    }

    /// `state.json` is on disk, so the ack can cover every input taken so far.
    pub fn stored(&mut self) {
        if self.last_n > self.acked {
            self.acked = self.last_n;
            self.send(&ToGame::Ack(Ack { n: self.acked }));
        }
    }

    pub fn publish(&mut self, files: &Files, windows: &[usize]) {
        self.send(&ToGame::Publish(Publish {
            body: hex(&files.body),
            restore: hex(&files.restore),
            live: hex(&files.live),
            windows: windows.to_vec(),
        }));
    }

    /// True once after a new connection: the game half needs the slot files at once.
    pub fn take_publish_due(&mut self) -> bool {
        std::mem::take(&mut self.publish_due)
    }

    /// A `request` for each new waiting request, and a `closed` for each one that is gone.
    pub fn sync_requests(&mut self) {
        if !self.is_connected() {
            return;
        }
        let waiting: Vec<_> = self
            .approvals
            .list()
            .into_iter()
            .filter(|p| self.approvals.answer_of(&p.id).is_none())
            .collect();
        let ids: BTreeSet<String> = waiting.iter().map(|p| p.id.clone()).collect();
        for gone in self.sent.difference(&ids).cloned().collect::<Vec<_>>() {
            self.send(&ToGame::Closed(Closed { id: gone }));
        }
        let new: Vec<Request> = waiting
            .iter()
            .filter(|p| !self.sent.contains(&p.id))
            .map(Request::of)
            .collect();
        for request in new {
            self.send(&ToGame::Request(request));
        }
        self.sent = ids;
    }

    fn send(&mut self, message: &ToGame) {
        let Some((_, out)) = &self.current else {
            return;
        };
        let line = match link::line_of(message) {
            Ok(line) => line,
            Err(e) => {
                log(&format!("link: cannot write a line: {e:#}"));
                return;
            }
        };
        if out.send(line).is_err() {
            self.current = None;
        }
    }
}

fn input_of(message: ToServer) -> Result<Option<Input>> {
    Ok(match message {
        ToServer::Records(Records {
            tag,
            now,
            source,
            records,
            ..
        }) => Some(Input::Records {
            tag: link::read_tag(&tag)?,
            records: link::read_records(&records)?,
            now,
            source,
        }),
        ToServer::ResetWindow(ResetWindow { token, .. }) => Some(Input::ResetWindow { token }),
        ToServer::SavedToken(SavedToken { account, token, .. }) => {
            Some(Input::SavedToken { account, token })
        }
        ToServer::Hello(_) | ToServer::Answer(_) => None,
    })
}

/// In the data folder of the server (SPEC.md 11.7.8).
pub const SOCKET_FILE: &str = "serve.sock";

#[cfg(unix)]
pub use socket::listen;

/// The serve socket (SPEC.md 11.7.8). Only `gnomish-relay attach` connects to it.
#[cfg(unix)]
mod socket {
    use std::io::{BufReader, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use anyhow::{Context, Result, bail};

    use super::{Inbound, SOCKET_FILE};
    use crate::link::{self, Hello, ToGame, ToServer};
    use crate::run::{log, now};

    const HELLO_WAIT: Duration = Duration::from_secs(20);

    /// Takes the place of an old socket file. The lock of 8.4 says that no other serve runs.
    pub fn listen(data: &Path) -> Result<Receiver<Inbound>> {
        let path = data.join(SOCKET_FILE);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if !meta.file_type().is_socket() {
                bail!(
                    "{} is not a socket. Remove it, then start again",
                    path.display()
                );
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("cannot listen on {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let (tx, rx) = channel();
        std::thread::spawn(move || accept(&listener, &tx));
        Ok(rx)
    }

    /// The stream of the game half that passed the last hello.
    type Current = Arc<Mutex<Option<UnixStream>>>;

    fn accept(listener: &UnixListener, tx: &Sender<Inbound>) {
        let next = AtomicU64::new(1);
        let current: Current = Arc::default();
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let conn = next.fetch_add(1, Ordering::SeqCst);
            let tx = tx.clone();
            let current = Arc::clone(&current);
            std::thread::spawn(move || {
                if let Err(e) = serve_connection(conn, &stream, &tx, &current) {
                    log(&format!("link: connection {conn}: {e:#}"));
                }
                let _ = tx.send(Inbound::Gone { conn });
            });
        }
    }

    fn serve_connection(
        conn: u64,
        stream: &UnixStream,
        tx: &Sender<Inbound>,
        current: &Current,
    ) -> Result<()> {
        let mut writer = stream.try_clone()?;
        #[cfg(target_os = "linux")]
        if let Some(why) = crate::serve_peer::refusal(stream) {
            writer.write_all(link::line_of(&ToGame::Refused(why.to_owned()))?.as_bytes())?;
            bail!("{why}");
        }
        stream.set_read_timeout(Some(HELLO_WAIT))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let ours = Hello::ours(now());
        let Some(Ok(ToServer::Hello(theirs))) =
            link::read_line(&mut reader)?.map(|l| serde_json::from_str(&l))
        else {
            bail!("the first line was not a hello");
        };
        if let Some(problem) = link::hello_problem(&theirs, &ours, "the game computer") {
            writer.write_all(link::line_of(&ToGame::Refused(problem.clone()))?.as_bytes())?;
            bail!("{problem}");
        }
        writer.write_all(link::line_of(&ToGame::Hello(ours))?.as_bytes())?;
        take_over(current, stream.try_clone()?);
        stream.set_read_timeout(None)?;
        let (out, lines) = channel::<String>();
        std::thread::spawn(move || write_lines(&mut writer, &lines));
        tx.send(Inbound::Connected { conn, out })?;
        while let Some(line) = link::read_line(&mut reader)? {
            let message: ToServer =
                serde_json::from_str(&line).context("a line of the game half")?;
            tx.send(Inbound::Message { conn, message })?;
        }
        Ok(())
    }

    /// A writer of the old connection can wait on a dead link for a long time, so the
    /// takeover shuts the old stream itself.
    fn take_over(current: &Current, stream: UnixStream) {
        let mut current = current.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = current.replace(stream) {
            let _ = old.shutdown(Shutdown::Both);
        }
    }

    /// The end of the channel is a takeover or a lost link. The shutdown ends the reader too.
    fn write_lines(writer: &mut UnixStream, lines: &Receiver<String>) {
        for line in lines {
            if writer.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
        let _ = writer.shutdown(Shutdown::Both);
    }

    #[cfg(test)]
    mod tests {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        use super::*;

        fn attach(data: &Path) -> BufReader<UnixStream> {
            let mut stream = UnixStream::connect(data.join(SOCKET_FILE)).unwrap();
            let hello = link::line_of(&ToServer::Hello(Hello::ours(now()))).unwrap();
            stream.write_all(hello.as_bytes()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("{\"hello\""), "{line}");
            reader
        }

        /// A writer that waits on a dead link never reaches its own shutdown, so the
        /// takeover closes the old connection itself. Found in a review on 2026-11-23.
        #[test]
        fn a_takeover_closes_the_old_connection_with_no_help_from_the_main_loop() {
            let data = tempfile::tempdir().unwrap();
            let _inbound = listen(data.path()).unwrap();
            let mut old = attach(data.path());

            let _new = attach(data.path());

            let mut rest = String::new();
            let read = old.read_line(&mut rest).unwrap();
            assert_eq!(read, 0, "the old connection is closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;
    use crate::desktop::Prompt;
    use crate::link::{Answered, records_text, tag_text};

    fn records_message(n: u32, text: &str) -> ToServer {
        let record = Record {
            token: b"tok1".to_vec(),
            chat: b"c1".to_vec(),
            id: n,
            cwd: Vec::new(),
            flags: Vec::new(),
            name: Vec::new(),
            text: text.as_bytes().to_vec(),
        };
        ToServer::Records(Records {
            n,
            tag: tag_text([u8::try_from(n).unwrap(); 8]),
            now: 100,
            source: Source::Strip,
            records: records_text(&[record]),
        })
    }

    struct Fixture {
        _data: tempfile::TempDir,
        tx: Sender<Inbound>,
        link: ServeLink,
        approvals: Approvals,
    }

    fn fixture() -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let approvals = Approvals::new(data.path(), Prompt::Off);
        let (tx, rx) = channel();
        Fixture {
            link: ServeLink::new(rx, approvals.clone()),
            approvals,
            _data: data,
            tx,
        }
    }

    fn connect(f: &Fixture, conn: u64) -> Receiver<String> {
        let (out, lines) = channel();
        f.tx.send(Inbound::Connected { conn, out }).unwrap();
        lines
    }

    fn send(f: &Fixture, conn: u64, message: ToServer) {
        f.tx.send(Inbound::Message { conn, message }).unwrap();
    }

    fn received(lines: &Receiver<String>) -> Vec<ToGame> {
        lines
            .try_iter()
            .map(|l| serde_json::from_str(l.trim_end()).unwrap())
            .collect()
    }

    fn texts(inputs: &[Input]) -> Vec<Vec<u8>> {
        inputs
            .iter()
            .filter_map(|i| match i {
                Input::Records { records, .. } => Some(records[0].text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_server_takes_numbers_in_order_and_skips_copies() {
        assert_eq!(next_input(0, 1), Next::Take);
        assert_eq!(next_input(3, 3), Next::Skip);
        assert_eq!(next_input(3, 1), Next::Skip);
        assert_eq!(next_input(3, 5), Next::Gap);
        assert_eq!(next_input(u32::MAX, u32::MAX), Next::Skip);
    }

    #[test]
    fn inputs_of_a_connection_come_out_in_order_once() {
        let mut f = fixture();
        let _lines = connect(&f, 1);
        send(&f, 1, records_message(1, "a"));
        send(&f, 1, records_message(1, "a"));
        send(&f, 1, records_message(3, "c"));
        send(&f, 1, records_message(2, "b"));

        let inputs = f.link.take_inputs();

        assert_eq!(texts(&inputs), [b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn the_server_acks_only_after_the_state_is_on_disk() {
        let mut f = fixture();
        let lines = connect(&f, 1);
        send(&f, 1, records_message(1, "a"));
        send(&f, 1, records_message(2, "b"));
        f.link.take_inputs();

        let before = received(&lines);
        f.link.stored();
        f.link.stored();

        assert!(!before.iter().any(|m| matches!(m, ToGame::Ack(_))));
        let acks: Vec<_> = received(&lines)
            .into_iter()
            .filter(|m| matches!(m, ToGame::Ack(_)))
            .collect();
        assert_eq!(acks, [ToGame::Ack(Ack { n: 2 })]);
    }

    #[test]
    fn a_new_connection_starts_the_numbers_again_and_asks_for_a_publish() {
        let mut f = fixture();
        let _old = connect(&f, 1);
        send(&f, 1, records_message(1, "a"));
        f.link.take_inputs();
        f.link.take_publish_due();

        let _new = connect(&f, 2);
        send(&f, 2, records_message(1, "a again"));
        let inputs = f.link.take_inputs();

        assert_eq!(texts(&inputs), [b"a again".to_vec()]);
        assert!(f.link.take_publish_due());
    }

    #[test]
    fn a_second_attach_takes_over_the_first() {
        let mut f = fixture();
        let old = connect(&f, 1);
        let new = connect(&f, 2);
        f.link.take_inputs();

        send(&f, 1, records_message(1, "late key of the old window"));
        f.tx.send(Inbound::Gone { conn: 1 }).unwrap();
        let inputs = f.link.take_inputs();
        f.link.stored();
        f.link
            .publish(&Files::empty(protocol::apps::App::Relay, 0), &[1]);

        assert!(inputs.is_empty());
        assert!(f.link.is_connected());
        assert!(received(&old).is_empty());
        assert_eq!(received(&new).len(), 1);
    }

    #[test]
    fn a_desktop_request_on_the_server_goes_to_the_game_once_and_closes() {
        let mut f = fixture();
        let lines = connect(&f, 1);
        f.link.take_inputs();
        let opened = f
            .approvals
            .open("claude", "/srv/code", "cargo test", "", 1)
            .unwrap();

        f.link.sync_requests();
        f.link.sync_requests();
        f.approvals.close(&opened.id);
        f.link.sync_requests();

        let sent = received(&lines);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert!(
            matches!(&sent[0], ToGame::Request(r) if r.id == opened.id && r.text == "cargo test")
        );
        assert_eq!(sent[1], ToGame::Closed(Closed { id: opened.id }));
    }

    #[test]
    fn an_answer_on_the_game_computer_reaches_the_server() {
        let mut f = fixture();
        let lines = connect(&f, 1);
        let opened = f
            .approvals
            .open("claude", "/srv/code", "cargo test", "", 1)
            .unwrap();
        send(
            &f,
            1,
            ToServer::Answer(Answered {
                id: opened.id.clone(),
                answer: link::Answer::Approve,
            }),
        );

        f.link.take_inputs();
        f.link.sync_requests();

        assert_eq!(
            f.approvals.answer_of(&opened.id),
            Some(crate::desktop::Verdict::Approve)
        );
        assert!(
            !received(&lines)
                .iter()
                .any(|m| matches!(m, ToGame::Request(_)))
        );
    }

    #[test]
    fn a_reconnect_sends_each_waiting_request_again() {
        let mut f = fixture();
        let _old = connect(&f, 1);
        f.link.take_inputs();
        f.approvals
            .open("claude", "/srv/code", "cargo test", "", 1)
            .unwrap();
        f.link.sync_requests();

        let new = connect(&f, 2);
        f.link.take_inputs();
        f.link.sync_requests();

        assert!(matches!(received(&new)[..], [ToGame::Request(_)]));
    }

    #[test]
    fn records_that_do_not_parse_are_dropped_and_the_next_input_still_comes() {
        let mut f = fixture();
        let _lines = connect(&f, 1);
        send(
            &f,
            1,
            ToServer::Records(Records {
                n: 1,
                tag: tag_text([0; 8]),
                now: 1,
                source: Source::Strip,
                records: "zz".into(),
            }),
        );
        send(&f, 1, records_message(2, "b"));

        let inputs = f.link.take_inputs();

        assert_eq!(texts(&inputs), [b"b".to_vec()]);
    }
}
