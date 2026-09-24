//! The bridge state machine. It follows `models/transport.qnt`: records stay in the
//! body until the addon reads them, a full body refuses new messages, and every
//! message runs at most once. No I/O here.

use std::collections::{BTreeMap, BTreeSet};

use protocol::folder::resolve_folder;
use protocol::rate::{ChatQueue, MAX_QUEUE, RateLimiter, admit_message, enqueue};
use protocol::record::Record;
use protocol::seen::{Seen, admit, new_seen};
use protocol::slot::{MAX_REPLIES, Reply, Status, prepare_replies, slot_body};

use crate::flags::{self, Flags};

const DEFAULT_AGENT: &str = "claude";
const BAD_FOLDER: &str = "Folder not allowed.";
const STOPPED: &str = "Stopped.";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChatId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    New,
    Resume,
}

/// Where agents can work (SPEC.md 6.2, rule 1). A folder from the game is relative
/// to `base`, and must stay inside one of `roots`.
pub struct Folders {
    pub roots: Vec<Vec<u8>>,
    pub base: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub agent: String,
    pub cwd: String,
    pub session: Session,
    pub text: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Duplicate,
    /// Not marked as seen, so the addon sends it again later.
    Refused,
    /// Seen, and answered with an error. It never runs.
    BadFolder,
    Control,
}

struct Entry {
    token: String,
    chat: ChatId,
    id: MessageId,
    status: Status,
    text: String,
}

pub struct Relay {
    folders: Folders,
    seen: Seen,
    limiter: RateLimiter,
    /// Every record the addon has not read, newest last.
    records: Vec<Entry>,
    queues: BTreeMap<ChatId, ChatQueue>,
    jobs: BTreeMap<(ChatId, MessageId), Job>,
    running: BTreeSet<ChatId>,
    next_slot: usize,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Relay {
    pub fn new(folders: Folders) -> Relay {
        Relay {
            folders,
            seen: new_seen(),
            limiter: RateLimiter { times: Vec::new() },
            records: Vec::new(),
            queues: BTreeMap::new(),
            jobs: BTreeMap::new(),
            running: BTreeSet::new(),
            next_slot: 1,
        }
    }

    pub fn next_slot(&self) -> usize {
        self.next_slot
    }

    /// A `/reload` frees every slot, so the next body starts at slot 1 (SPEC.md 7.3).
    pub fn reset_window(&mut self) {
        self.next_slot = 1;
    }

    /// The records that the addon has not read. The body holds all of them.
    pub fn unread(&self) -> usize {
        self.records.len()
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
            let read = e.token == token && flags.read.contains(&e.id.0);
            !read || matches!(e.status, Status::Working)
        });
    }

    fn on_record(&mut self, r: &Record, now: u32) -> Outcome {
        let flags = flags::parse(&r.flags);
        let chat = ChatId(text(&r.chat));
        if flags.hello {
            return Outcome::Control;
        }
        if flags.stop {
            self.stop(&chat);
            return Outcome::Control;
        }
        if let Err(outcome) = self.admit(r, &chat, now) {
            return outcome;
        }
        let Some(cwd) = resolve_folder(&self.folders.roots, &self.folders.base, &r.cwd) else {
            self.set_record(
                &text(&r.token),
                &chat,
                MessageId(r.id),
                Status::Error,
                BAD_FOLDER.into(),
            );
            return Outcome::BadFolder;
        };
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: flags.agent.unwrap_or_else(|| DEFAULT_AGENT.to_owned()),
            cwd: text(&cwd),
            session: if flags.new_session {
                Session::New
            } else {
                Session::Resume
            },
            text: text(&r.text),
        })
    }

    /// Marks the message as seen, or says why not.
    ///
    /// The order is a trap. Every refusal comes before `admit`, because a refused
    /// message must not count as seen: the addon sends it again later. The rate
    /// limiter changes only after `admit`, so a duplicate uses no rate.
    fn admit(&mut self, r: &Record, chat: &ChatId, now: u32) -> Result<(), Outcome> {
        let queued = self.queues.get(chat).map_or(0, |q| q.ids.len());
        if self.records.len() >= MAX_REPLIES || queued >= MAX_QUEUE {
            return Err(Outcome::Refused);
        }
        let (rate_ok, limiter) = admit_message(&self.limiter, now);
        if !rate_ok {
            return Err(Outcome::Refused);
        }
        let (fresh, seen) = admit(
            std::mem::replace(&mut self.seen, new_seen()),
            &r.token,
            r.id,
        );
        self.seen = seen;
        if !fresh {
            return Err(Outcome::Duplicate);
        }
        self.limiter = limiter;
        Ok(())
    }

    fn enqueue_job(&mut self, job: Job) -> Outcome {
        let queue = self
            .queues
            .remove(&job.chat)
            .unwrap_or(ChatQueue { ids: Vec::new() });
        let Some(queue) = enqueue(queue, job.id.0) else {
            return Outcome::Refused;
        };
        self.queues.insert(job.chat.clone(), queue);
        self.set_record(
            &job.token,
            &job.chat,
            job.id,
            Status::Working,
            String::new(),
        );
        self.jobs.insert((job.chat.clone(), job.id), job);
        Outcome::Accepted
    }

    /// The waiting messages of a chat end as errors. A run in progress goes on.
    fn stop(&mut self, chat: &ChatId) {
        let Some(queue) = self.queues.remove(chat) else {
            return;
        };
        for id in queue.ids {
            if let Some(job) = self.jobs.remove(&(chat.clone(), MessageId(id))) {
                self.set_record(&job.token, chat, job.id, Status::Error, STOPPED.into());
            }
        }
    }

    /// The oldest waiting message of a chat that has no run in progress.
    pub fn next_job(&mut self) -> Option<Job> {
        let chat = self
            .queues
            .iter()
            .find(|(chat, q)| !q.ids.is_empty() && !self.running.contains(*chat))?
            .0
            .clone();
        let id = self.queues.get_mut(&chat)?.ids.remove(0);
        let job = self.jobs.remove(&(chat.clone(), MessageId(id)))?;
        self.running.insert(chat);
        Some(job)
    }

    pub fn finish(&mut self, job: &Job, result: Result<String, String>) {
        self.running.remove(&job.chat);
        let (status, text) = match result {
            Ok(text) => (Status::Done, text),
            Err(text) => (Status::Error, text),
        };
        self.set_record(&job.token, &job.chat, job.id, status, text);
    }

    /// Puts the record of a message at the newest place with its new state.
    fn set_record(
        &mut self,
        token: &str,
        chat: &ChatId,
        id: MessageId,
        status: Status,
        text: String,
    ) {
        self.records.retain(|e| !(e.token == token && e.id == id));
        self.records.push(Entry {
            token: token.to_owned(),
            chat: chat.clone(),
            id,
            status,
            text,
        });
    }

    pub fn body(&self, now: u32) -> Vec<u8> {
        let replies: Vec<Reply> = self
            .records
            .iter()
            .map(|e| Reply {
                chat: e.chat.0.as_bytes().to_vec(),
                id: e.id.0,
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

    fn relay() -> Relay {
        Relay::new(Folders {
            roots: vec![b"/home/x/Code".to_vec()],
            base: b"/home/x/Code".to_vec(),
        })
    }

    fn record_in(cwd: &str, chat: &str, id: u32, flags: &str, text: &str) -> Record {
        Record {
            token: b"tok".to_vec(),
            chat: chat.as_bytes().to_vec(),
            id,
            cwd: cwd.as_bytes().to_vec(),
            flags: flags.as_bytes().to_vec(),
            name: Vec::new(),
            text: text.as_bytes().to_vec(),
        }
    }

    fn record(chat: &str, id: u32, flags: &str, text: &str) -> Record {
        record_in("", chat, id, flags, text)
    }

    fn body(relay: &Relay) -> String {
        String::from_utf8(relay.body(NOW)).unwrap()
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
        let mut relay = relay();
        let frame = [record("c1", 1, "agent=codex;n", "hi")];
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Accepted]);
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Duplicate]);
        let jobs = run_all(&mut relay);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].agent, "codex");
        assert_eq!(jobs[0].session, Session::New);
    }

    #[test]
    fn a_chat_runs_its_messages_in_order_one_at_a_time() {
        let mut relay = relay();
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
        assert_eq!((first.id, other.id), (MessageId(1), MessageId(3)));
        assert!(relay.next_job().is_none());
        relay.finish(&first, Ok(String::new()));
        assert_eq!(relay.next_job().unwrap().id, MessageId(2));
    }

    #[test]
    fn a_full_chat_queue_refuses_without_marking_seen() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 100, "", "x")], NOW);
        let _running = relay.next_job().unwrap();
        for id in 0..20 {
            assert_eq!(
                relay.on_frame(&[record("c1", id, "", "x")], NOW + id * 7),
                [Outcome::Accepted]
            );
        }
        assert_eq!(
            relay.on_frame(&[record("c1", 20, "", "x")], NOW + 200),
            [Outcome::Refused]
        );
    }

    #[test]
    fn a_full_body_refuses_a_message_without_marking_it_seen() {
        let mut relay = relay();
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
        assert_eq!(
            relay.on_frame(&[record("late", 99, &report, "x")], NOW + 301),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn the_rate_limit_refuses_without_marking_seen() {
        let mut relay = relay();
        for id in 0..10 {
            relay.on_frame(&[record(&format!("c{id}"), id, "", "x")], NOW);
        }
        assert_eq!(
            relay.on_frame(&[record("c10", 10, "", "x")], NOW),
            [Outcome::Refused]
        );
        assert_eq!(
            relay.on_frame(&[record("c10", 10, "", "x")], NOW + 60),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn a_duplicate_uses_no_rate() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "x")], NOW);
        for _ in 0..20 {
            relay.on_frame(&[record("c1", 1, "", "x")], NOW);
        }
        assert_eq!(
            relay.on_frame(&[record("c1", 2, "", "x")], NOW),
            [Outcome::Accepted]
        );
    }

    #[test]
    fn a_read_final_reply_leaves_the_body_but_a_working_one_stays() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Ok("done".into()));
        relay.on_frame(&[record("relay", 0, "h;read=1,2", "")], NOW);
        assert!(!body(&relay).contains("id = 1,"));
        assert!(body(&relay).contains("id = 2,"));
    }

    #[test]
    fn a_read_list_from_another_token_removes_nothing() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        run_all(&mut relay);
        let mut other = record("relay", 0, "h;read=1", "");
        other.token = b"other".to_vec();
        relay.on_frame(&[other], NOW);
        assert!(body(&relay).contains("id = 1,"));
    }

    #[test]
    fn the_next_flag_moves_the_slot_window() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);
        assert_eq!(relay.next_slot(), 57);
    }

    #[test]
    fn a_reload_starts_the_slot_window_at_one() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);
        relay.reset_window();
        assert_eq!(relay.next_slot(), 1);
    }

    #[test]
    fn stop_ends_the_waiting_messages_of_a_chat_as_errors() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        let _running = relay.next_job().unwrap();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains(r#"id = 2, status = "error", text = "Stopped.""#));
    }

    #[test]
    fn a_failed_run_publishes_an_error() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish(&job, Err("agent crashed".into()));
        assert!(body(&relay).contains(r#"status = "error", text = "agent crashed""#));
    }

    #[test]
    fn a_folder_resolves_against_the_base_and_stays_in_a_root() {
        let mut relay = relay();
        relay.on_frame(
            &[
                record_in("app/../lib", "c1", 1, "", "a"),
                record_in("", "c2", 2, "", "b"),
            ],
            NOW,
        );
        let jobs = run_all(&mut relay);
        assert_eq!(jobs[0].cwd, "/home/x/Code/lib");
        assert_eq!(jobs[1].cwd, "/home/x/Code");
    }

    #[test]
    fn a_folder_outside_every_root_never_runs_and_gets_an_error() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record_in("../../.ssh", "c1", 1, "", "a")], NOW);
        assert_eq!(outcomes, [Outcome::BadFolder]);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains(r#"id = 1, status = "error", text = "Folder not allowed.""#));
        assert_eq!(
            relay.on_frame(&[record_in("../../.ssh", "c1", 1, "", "a")], NOW),
            [Outcome::Duplicate]
        );
    }
}
