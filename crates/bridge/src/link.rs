//! The lines of the link between the game half and the server half (SPEC.md 11.7.3).

use std::io::BufRead;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::desktop::{self, Pending, Verdict};
use crate::ids::hex;
use protocol::link::clocks_agree;
use protocol::record::{Record, parse_records, serialize_records};

pub const PROTO: u32 = 1;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The same limit as a line of an agent process (SPEC.md 9.4).
pub const MAX_LINE: usize = 8 << 20;
/// The relay keeps the windows of the three newest tokens (SPEC.md 7.3).
const MAX_WINDOWS: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub proto: u32,
    pub version: String,
    pub now: u32,
}

impl Hello {
    pub fn ours(now: u32) -> Hello {
        Hello {
            proto: PROTO,
            version: VERSION.to_owned(),
            now,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Strip,
    Outbox,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Source::Strip => "strip",
            Source::Outbox => "outbox",
        }
    }
}

/// Records in the form of the strip payload, in hex. `parse_records` reads them back (S3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Records {
    pub n: u32,
    pub tag: String,
    pub now: u32,
    pub source: Source,
    pub records: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResetWindow {
    pub n: u32,
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SavedToken {
    pub n: u32,
    pub account: String,
    pub token: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Approve,
    Deny,
}

impl From<Answer> for Verdict {
    fn from(answer: Answer) -> Verdict {
        match answer {
            Answer::Approve => Verdict::Approve,
            Answer::Deny => Verdict::Deny,
        }
    }
}

impl From<Verdict> for Answer {
    fn from(verdict: Verdict) -> Answer {
        match verdict {
            Verdict::Approve => Answer::Approve,
            Verdict::Deny => Answer::Deny,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    pub id: String,
    pub answer: Answer,
}

/// What the game half sends. The numbered ones wait in its outbox until an ack.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToServer {
    Hello(Hello),
    Records(Records),
    ResetWindow(ResetWindow),
    SavedToken(SavedToken),
    Answer(Answered),
}

impl ToServer {
    /// The number of an input of the outbox. `None` for the hello and an answer.
    pub fn number(&self) -> Option<u32> {
        match self {
            ToServer::Records(r) => Some(r.n),
            ToServer::ResetWindow(r) => Some(r.n),
            ToServer::SavedToken(s) => Some(s.n),
            ToServer::Hello(_) | ToServer::Answer(_) => None,
        }
    }

    #[must_use]
    pub fn with_number(self, n: u32) -> ToServer {
        match self {
            ToServer::Records(r) => ToServer::Records(Records { n, ..r }),
            ToServer::ResetWindow(r) => ToServer::ResetWindow(ResetWindow { n, ..r }),
            ToServer::SavedToken(s) => ToServer::SavedToken(SavedToken { n, ..s }),
            other => other,
        }
    }
}

/// The three slot files of the relay, in hex, and the slot window of each token.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    pub body: String,
    pub restore: String,
    pub live: String,
    pub windows: Vec<usize>,
}

/// A waiting desktop request: the fields of its request file (SPEC.md 6.6.3).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub created: u32,
    pub agent: String,
    pub folder: String,
    pub text: String,
    pub kind: desktop::Kind,
    pub wait_minutes: u64,
}

impl Request {
    pub fn of(pending: &Pending) -> Request {
        Request {
            id: pending.id.clone(),
            created: pending.created,
            agent: pending.agent.clone(),
            folder: pending.folder.clone(),
            text: pending.text.clone(),
            kind: pending.kind,
            wait_minutes: pending.wait_minutes,
        }
    }

    /// The server comes from the config of the game computer, never from the server.
    pub fn copy_on(self, server: &str) -> Pending {
        Pending {
            id: self.id,
            created: self.created,
            agent: self.agent,
            folder: self.folder,
            text: self.text,
            kind: self.kind,
            wait_minutes: self.wait_minutes,
            server: Some(server.to_owned()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Closed {
    pub id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    pub n: u32,
}

/// What the server half sends.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToGame {
    Hello(Hello),
    Refused(String),
    Ack(Ack),
    Publish(Publish),
    Request(Request),
    Closed(Closed),
}

pub fn records_text(records: &[Record]) -> String {
    hex(&serialize_records(records))
}

pub fn read_records(text: &str) -> Result<Vec<Record>> {
    let bytes = unhex(text)?;
    parse_records(&bytes).map_err(|_| anyhow::anyhow!("the records do not parse"))
}

pub fn tag_text(tag: [u8; 8]) -> String {
    hex(&tag)
}

pub fn read_tag(text: &str) -> Result<[u8; 8]> {
    let bytes = unhex(text)?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("a frame tag is 8 bytes"))
}

pub fn unhex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        bail!("not hex");
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).context("not hex"))
        .collect()
}

/// The publish of the server half has only slot windows that the slot pool holds.
pub fn check_publish(publish: &Publish) -> Result<()> {
    if publish.windows.len() > MAX_WINDOWS {
        bail!(
            "{} slot windows, at most {MAX_WINDOWS}",
            publish.windows.len()
        );
    }
    if let Some(bad) = publish
        .windows
        .iter()
        .find(|w| **w == 0 || **w > protocol::slot::SLOTS)
    {
        bail!("slot window {bad} is outside the slot pool");
    }
    Ok(())
}

/// A copy of a request lands in a file named by its id, so the id must be a request id.
pub fn check_request(request: &Request) -> Result<()> {
    if !desktop::is_request_id(&request.id) {
        bail!("{:?} is not a request id", request.id);
    }
    Ok(())
}

/// Why the other side cannot be the other half, in words for `status`.
pub fn hello_problem(theirs: &Hello, ours: &Hello, host: &str) -> Option<String> {
    if theirs.proto != ours.proto || theirs.version != ours.version {
        return Some(format!(
            "{host} has Gnomish Relay {}, and this computer has {}. Update both, then run `gnomish-relay restart`.",
            theirs.version, ours.version
        ));
    }
    if !clocks_agree(theirs.now, ours.now) {
        return Some(format!(
            "the clock of {host} is {} s off. Turn on network time on both computers.",
            theirs.now.abs_diff(ours.now)
        ));
    }
    None
}

/// One line, at most `MAX_LINE` bytes. `None` at the end of the stream.
pub fn read_line(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    let read =
        std::io::Read::take(&mut *reader, MAX_LINE as u64 + 1).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') {
        bail!("a line of the link is longer than {MAX_LINE} bytes, or has no end");
    }
    bytes.pop();
    String::from_utf8(bytes)
        .map(Some)
        .context("a line of the link is not UTF-8")
}

pub fn line_of<T: Serialize>(message: &T) -> Result<String> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    Ok(line)
}

/// The installers put the program in `~/.local/bin`, which a shell of ssh with no
/// terminal often lacks on its PATH (found by Switchboard).
pub const REMOTE_ATTACH: &str =
    "PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\" exec gnomish-relay attach";

/// The fixed options of SPEC.md 11.7.4 go after the first word of the command.
pub fn ssh_command(host: &str, command: &[String]) -> Vec<String> {
    let base: Vec<String> = if command.is_empty() {
        vec!["ssh".into(), host.into(), REMOTE_ATTACH.into()]
    } else {
        command.to_vec()
    };
    let mut words = vec![base[0].clone()];
    if is_ssh(&base[0]) {
        words.extend(SSH_OPTIONS.iter().map(|o| (*o).to_owned()));
    }
    words.extend(base[1..].iter().cloned());
    words
}

fn is_ssh(program: &str) -> bool {
    let name = std::path::Path::new(program)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.eq_ignore_ascii_case("ssh")
}

pub const SSH_OPTIONS: [&str; 15] = [
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
    "-o",
    "ForwardAgent=no",
    "-o",
    "ForwardX11=no",
    "-o",
    "ClearAllForwardings=yes",
    "-o",
    "PermitLocalCommand=no",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn record(text: &str) -> Record {
        Record {
            token: b"tok1".to_vec(),
            chat: b"c1".to_vec(),
            id: 7,
            cwd: b"~/code".to_vec(),
            flags: b"".to_vec(),
            name: b"Claude".to_vec(),
            text: text.as_bytes().to_vec(),
        }
    }

    #[test]
    fn records_cross_the_link_unchanged() {
        let records = vec![record("hi\tthere\nnext"), record("")];

        let back = read_records(&records_text(&records)).unwrap();

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].text, records[0].text);
        assert_eq!(back[1].cwd, records[1].cwd);
    }

    #[test]
    fn a_message_line_reads_back_as_the_same_message() {
        let sent = ToServer::Records(Records {
            n: 3,
            tag: tag_text([1, 2, 3, 4, 5, 6, 7, 8]),
            now: 100,
            source: Source::Strip,
            records: records_text(&[record("x")]),
        });

        let line = line_of(&sent).unwrap();
        let back: ToServer = serde_json::from_str(line.trim_end()).unwrap();

        assert_eq!(back, sent);
        assert!(line.starts_with("{\"records\":{"));
    }

    #[test]
    fn a_line_with_an_unknown_field_is_refused() {
        let line = r#"{"ack":{"n":1,"more":2}}"#;

        assert!(serde_json::from_str::<ToGame>(line).is_err());
    }

    #[test]
    fn the_hello_refuses_another_version_and_says_why() {
        let ours = Hello::ours(1000);
        let theirs = Hello {
            version: "0.0.1".into(),
            ..ours.clone()
        };

        let problem = hello_problem(&theirs, &ours, "me@box").unwrap();

        assert!(problem.starts_with("me@box has Gnomish Relay 0.0.1, and this computer has"));
    }

    #[test]
    fn the_hello_refuses_clocks_more_than_a_minute_apart() {
        let ours = Hello::ours(1000);

        assert_eq!(hello_problem(&Hello::ours(1060), &ours, "me@box"), None);
        let problem = hello_problem(&Hello::ours(1095), &ours, "me@box").unwrap();
        assert_eq!(
            problem,
            "the clock of me@box is 95 s off. Turn on network time on both computers."
        );
    }

    #[test]
    fn the_ssh_command_turns_off_agent_and_port_forwarding() {
        let words = ssh_command("me@box", &[]);

        assert_eq!(words[0], "ssh");
        for option in [
            "BatchMode=yes",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ClearAllForwardings=yes",
            "PermitLocalCommand=no",
        ] {
            assert!(words.iter().any(|w| w == option), "{option}");
        }
        assert_eq!(&words[words.len() - 2..], ["me@box", REMOTE_ATTACH]);
    }

    #[test]
    fn a_custom_ssh_command_still_gets_the_fixed_options_after_the_program() {
        let command: Vec<String> = [
            "/usr/bin/ssh",
            "-p",
            "2222",
            "me@box",
            "gnomish-relay",
            "attach",
        ]
        .map(String::from)
        .to_vec();

        let words = ssh_command("me@box", &command);

        assert_eq!(words[0], "/usr/bin/ssh");
        assert_eq!(words[1], "-T");
        assert_eq!(&words[words.len() - 5..], &command[1..]);
    }

    #[test]
    fn a_command_that_is_not_ssh_gets_no_ssh_options() {
        let command: Vec<String> = ["gnomish-relay", "attach"].map(String::from).to_vec();

        assert_eq!(ssh_command("me@box", &command), command);
    }

    #[test]
    fn a_publish_with_a_window_outside_the_pool_is_refused() {
        let publish = Publish {
            body: String::new(),
            restore: String::new(),
            live: String::new(),
            windows: vec![protocol::slot::SLOTS + 1],
        };

        assert!(check_publish(&publish).is_err());
        assert!(
            check_publish(&Publish {
                windows: vec![1, 4],
                ..publish
            })
            .is_ok()
        );
    }

    #[test]
    fn a_request_whose_id_is_not_a_request_id_is_refused() {
        let request = Request {
            id: "../x".into(),
            created: 0,
            agent: "claude".into(),
            folder: "~/code".into(),
            text: "rm -rf".into(),
            kind: desktop::Kind::ToolCall,
            wait_minutes: 5,
        };

        assert!(check_request(&request).is_err());
        assert!(
            check_request(&Request {
                id: "0123456789ab".into(),
                ..request
            })
            .is_ok()
        );
    }

    #[test]
    fn a_line_without_an_end_is_refused() {
        let mut reader = std::io::Cursor::new(b"{\"ack\":{\"n\":1}}".to_vec());

        assert!(read_line(&mut reader).is_err());
    }

    #[test]
    fn a_line_over_the_limit_is_refused() {
        let mut long = vec![b'a'; MAX_LINE + 1];
        long.push(b'\n');
        let mut reader = std::io::Cursor::new(long);

        assert!(read_line(&mut reader).is_err());
    }

    #[test]
    fn the_end_of_the_stream_reads_as_none() {
        let mut reader = std::io::Cursor::new(Vec::new());

        assert_eq!(read_line(&mut reader).unwrap(), None);
    }
}
