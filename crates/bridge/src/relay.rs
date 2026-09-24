//! The bridge state machine. It follows `models/transport.qnt`: records stay in the
//! body until the addon reads them, a full body refuses new messages, and every
//! message runs at most once. No I/O here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use protocol::rate::{RateLimiter, admit_message};
use protocol::record::Record;
use protocol::seen::{Seen, admit, new_seen};
use protocol::slot::{MAX_REPLIES, Reply, Status, prepare_replies, slot_body};

use crate::flags::{self, Flags};

/// A chat queue holds at most this many waiting messages (SPEC.md 6.2, rule 9).
const MAX_QUEUE: usize = 20;
const DEFAULT_AGENT: &str = "claude";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub token: String,
    pub chat: String,
    pub id: u32,
    pub agent: String,
    pub cwd: String,
    pub new_session: bool,
    pub text: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Duplicate,
    /// Not marked as seen, so the addon sends it again later.
    Refused,
    Control,
}

struct Entry {
    token: String,
    chat: String,
    id: u32,
    status: Status,
    text: String,
}

pub struct Relay {
    seen: Seen,
    limiter: RateLimiter,
    /// Every record the addon has not read, newest last.
    records: Vec<Entry>,
    queues: BTreeMap<String, VecDeque<Job>>,
    running: BTreeSet<String>,
    next_slot: usize,
}

impl Default for Relay {
    fn default() -> Relay {
        Relay {
            seen: new_seen(),
            limiter: RateLimiter { times: Vec::new() },
            records: Vec::new(),
            queues: BTreeMap::new(),
            running: BTreeSet::new(),
            next_slot: 1,
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Relay {
    pub fn next_slot(&self) -> usize {
        self.next_slot
    }

    /// Takes one frame. The flags of its first record carry the report of the addon.
    pub fn on_frame(&mut self, records: &[Record], now: u32) -> Vec<Outcome> {
        if let Some(first) = records.first() {
            self.take_report(&text(&first.token), &flags::parse(&first.flags));
        }
        records.iter().map(|r| self.on_record(r, now)).collect()
    }

    fn take_report(&mut self, token: &str, flags: &Flags) {
        if let Some(next) = flags.next {
            self.next_slot = next.max(1);
        }
        self.records.retain(|e| {
            !(e.token == token
                && !matches!(e.status, Status::Working)
                && flags.read.contains(&e.id))
        });
    }

    fn on_record(&mut self, r: &Record, now: u32) -> Outcome {
        let flags = flags::parse(&r.flags);
        let chat = text(&r.chat);
        if flags.hello {
            return Outcome::Control;
        }
        if flags.stop {
            for job in self.queues.remove(&chat).unwrap_or_default() {
                self.set_record(&job, Status::Error, "Stopped.".into());
            }
            return Outcome::Control;
        }
        let queue_len = self.queues.get(&chat).map_or(0, VecDeque::len);
        if self.records.len() >= MAX_REPLIES || queue_len >= MAX_QUEUE {
            return Outcome::Refused;
        }
        let (rate_ok, limiter) = admit_message(&self.limiter, now);
        if !rate_ok {
            return Outcome::Refused;
        }
        let (fresh, seen) = admit(
            std::mem::replace(&mut self.seen, new_seen()),
            &r.token,
            r.id,
        );
        self.seen = seen;
        if !fresh {
            return Outcome::Duplicate;
        }
        self.limiter = limiter;

        let job = Job {
            token: text(&r.token),
            chat: chat.clone(),
            id: r.id,
            agent: flags.agent.unwrap_or_else(|| DEFAULT_AGENT.to_owned()),
            cwd: text(&r.cwd),
            new_session: flags.new_session,
            text: text(&r.text),
        };
        self.records.push(Entry {
            token: job.token.clone(),
            chat: chat.clone(),
            id: r.id,
            status: Status::Working,
            text: String::new(),
        });
        self.queues.entry(chat).or_default().push_back(job);
        Outcome::Accepted
    }

    /// The oldest waiting message of a chat that has no run in progress.
    pub fn next_job(&mut self) -> Option<Job> {
        let chat = self
            .queues
            .iter()
            .find(|(chat, q)| !q.is_empty() && !self.running.contains(*chat))?
            .0
            .clone();
        let job = self.queues.get_mut(&chat)?.pop_front()?;
        self.running.insert(chat);
        Some(job)
    }

    pub fn finish(&mut self, job: &Job, result: Result<String, String>) {
        self.running.remove(&job.chat);
        match result {
            Ok(text) => self.set_record(job, Status::Done, text),
            Err(text) => self.set_record(job, Status::Error, text),
        }
    }

    /// Moves the record of `job` to the newest place with its new state.
    fn set_record(&mut self, job: &Job, status: Status, text: String) {
        self.records
            .retain(|e| !(e.token == job.token && e.id == job.id));
        self.records.push(Entry {
            token: job.token.clone(),
            chat: job.chat.clone(),
            id: job.id,
            status,
            text,
        });
    }

    pub fn body(&self, now: u32) -> Vec<u8> {
        let replies: Vec<Reply> = self
            .records
            .iter()
            .map(|e| Reply {
                chat: e.chat.as_bytes().to_vec(),
                id: e.id,
                status: e.status,
                text: e.text.as_bytes().to_vec(),
            })
            .collect();
        slot_body(now, &prepare_replies(&replies))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_790_211_079;

    fn record(chat: &str, id: u32, flags: &str, text: &str) -> Record {
        Record {
            token: b"tok".to_vec(),
            chat: chat.as_bytes().to_vec(),
            id,
            cwd: Vec::new(),
            flags: flags.as_bytes().to_vec(),
            name: Vec::new(),
            text: text.as_bytes().to_vec(),
        }
    }

    fn run_all(relay: &mut Relay) -> Vec<Job> {
        let mut jobs = Vec::new();
        while let Some(job) = relay.next_job() {
            relay.finish(&job, Ok(format!("echo: {}", job.text)));
            jobs.push(job);
        }
        jobs
    }

    #[test]
    fn a_message_runs_once_even_when_the_strip_is_read_twice() {
        let mut relay = Relay::default();
        let frame = [record("c1", 1, "agent=codex;n", "hi")];
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Accepted]);
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Duplicate]);
        let jobs = run_all(&mut relay);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].agent, "codex");
        assert!(jobs[0].new_session);
    }

    #[test]
    fn a_chat_runs_its_messages_in_order_one_at_a_time() {
        let mut relay = Relay::default();
        relay.on_frame(
            &[
                record("c1", 1, "", "a"),
                record("c1", 2, "", "b"),
                record("c2", 3, "", "c"),
            ],
            NOW,
        );
        let first = relay.next_job().unwrap();
        let other = relay.next_job().unwrap();
        assert_eq!((first.id, other.id), (1, 3));
        assert!(relay.next_job().is_none());
        relay.finish(&first, Ok(String::new()));
        assert_eq!(relay.next_job().unwrap().id, 2);
    }

    #[test]
    fn a_full_body_refuses_a_message_without_marking_it_seen() {
        let mut relay = Relay::default();
        for id in 0..30 {
            relay.on_frame(&[record(&format!("c{id}"), id, "", "x")], NOW + id * 7);
        }
        run_all(&mut relay);
        assert_eq!(
            relay.on_frame(&[record("late", 99, "", "x")], NOW + 300),
            [Outcome::Refused]
        );

        let read: Vec<String> = (0..30).map(|id| id.to_string()).collect();
        let report = format!("next=2;read={}", read.join(","));
        let outcomes = relay.on_frame(&[record("late", 99, &report, "x")], NOW + 301);
        assert_eq!(outcomes, [Outcome::Accepted]);
    }

    #[test]
    fn the_rate_limit_refuses_without_marking_seen() {
        let mut relay = Relay::default();
        for id in 0..10 {
            relay.on_frame(&[record("c1", id, "", "x")], NOW);
        }
        assert_eq!(
            relay.on_frame(&[record("c1", 10, "", "x")], NOW),
            [Outcome::Refused]
        );
        assert_eq!(
            relay.on_frame(&[record("c1", 10, "", "x")], NOW + 60),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn a_read_final_reply_leaves_the_body_but_a_working_one_stays() {
        let mut relay = Relay::default();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Ok("done".into()));
        relay.on_frame(&[record("relay", 0, "h;read=1,2", "")], NOW);
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(!body.contains("id = 1,"), "{body}");
        assert!(body.contains("id = 2,"), "{body}");
    }

    #[test]
    fn the_next_flag_moves_the_slot_window() {
        let mut relay = Relay::default();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);
        assert_eq!(relay.next_slot(), 57);
    }

    #[test]
    fn stop_drops_the_waiting_messages_of_a_chat() {
        let mut relay = Relay::default();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        let _running = relay.next_job().unwrap();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert!(relay.next_job().is_none());
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(
            body.contains(r#"id = 2, status = "error", text = "Stopped.""#),
            "{body}"
        );
    }

    #[test]
    fn a_failed_run_publishes_an_error() {
        let mut relay = Relay::default();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Err("agent crashed".into()));
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(
            body.contains(r#"status = "error", text = "agent crashed""#),
            "{body}"
        );
    }
}
