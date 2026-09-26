//! The bridge state machine. It follows `models/transport.qnt`: records stay in the
//! body until the addon reads them, a full body refuses new messages, and every
//! message runs at most once. No I/O here.

use std::collections::{BTreeMap, BTreeSet};

use protocol::apps::App;
use protocol::folder::resolve_folder;
use protocol::rate::{ChatQueue, MAX_QUEUE, enqueue};
use protocol::record::Record;
use protocol::restore::{prepare_restore, restore_body};
use protocol::slot::Status;

use serde::{Deserialize, Serialize};

use crate::activity::Activity;
use crate::agent::{Choice, SessionInfo};
use crate::config::{
    Permission, Policy, folder_request, native_folder, path_bytes, relative_folder,
};
use crate::flags::{self, TransportFlags};
use crate::history::{ChatLog, History, Speaker};
pub use crate::lane::{ChatId, MessageId};
use crate::lane::{Lane, NotAdmitted, keep_last};
use crate::reply::render_reply;
use crate::state::State;

const BAD_FOLDER: &str = "Folder not allowed.";
const BAD_AGENT: &str = "Agent not set up.";
const STOPPED: &str = "Stopped.";
const RESTARTED: &str = "Stopped: the bridge restarted.";
/// The agent sessions of the chats with the latest runs.
const MAX_SESSIONS: usize = 64;
/// The sessions in one list for the game, newest first.
const MAX_LISTED: usize = 30;
const MAX_TITLE: usize = 100;
/// A session that changed this recently is probably open in a terminal.
const ACTIVE_FOR: u32 = 300;
const NO_SESSION: &str = "Session not found. Open Resume again.";

/// The agent session of a chat. The next message of the chat resumes it, if its
/// agent and its folder are the same (SPEC.md 9.5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentSession {
    pub chat: ChatId,
    pub agent: String,
    pub cwd: String,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Session {
    New,
    Resume,
}

/// What a job does. Only a prompt reaches the model.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    #[default]
    Prompt,
    /// The saved sessions of every agent, for Resume in the game.
    ListSessions,
    /// A new chat continues this session. A session that is open in a terminal gets a
    /// fork, so the two never write into one session.
    Attach { session: String, fork: bool },
}

/// A session of the last list. The game can resume only these, so the folder check
/// of the list also guards every resume.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Listed {
    agent: String,
    id: String,
    /// The folder in the form that jobs use.
    cwd: String,
    /// The folder relative to the base, as the game sends it back.
    folder: String,
    title: String,
    updated: u32,
}

/// Where agents can work (SPEC.md 6.2, rule 1). A folder from the game is relative
/// to `base`, and must stay inside one of `roots`.
pub struct Folders {
    pub roots: Vec<Vec<u8>>,
    pub base: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub agent: String,
    /// A job from an older state file has none, and gets the strictest level.
    #[serde(default)]
    pub permission: Permission,
    pub cwd: String,
    pub session: Session,
    /// The agent session to resume, set when the job starts.
    #[serde(default)]
    pub resume: Option<String>,
    pub text: String,
    #[serde(default)]
    pub work: Work,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Duplicate,
    /// Not marked as seen, so the addon sends it again later.
    Refused,
    /// Seen, and answered with an error. It never runs.
    BadFolder,
    /// Seen, and answered with an error: the config has no such agent.
    BadAgent,
    /// Seen, and answered with an error: the last list had no such session.
    BadSession,
    /// Seen, and answered with the update text of its app (SPEC.md 7.7).
    WrongVersion,
    Control,
}

/// The coding app over its lane: the policy, the queues, the agent sessions, and the
/// history for a restore.
pub struct Relay {
    policy: Policy,
    lane: Lane,
    queues: BTreeMap<ChatId, ChatQueue>,
    jobs: BTreeMap<(ChatId, MessageId), Job>,
    running: BTreeSet<ChatId>,
    history: History,
    /// The new token after a saved-data wipe, until it reports `restored`.
    restore_for: Option<String>,
    sessions: Vec<AgentSession>,
    /// Chats whose run in progress got a Stop. The bridge signals each run.
    cancels: Vec<ChatId>,
    /// Chats that the game deleted. A run of one that ends later leaves no trace.
    deleted: Vec<ChatId>,
    listed: Vec<Listed>,
    activity: Activity,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A tab or a line break inside a field would break the lines of a list.
fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn cut_chars(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl Relay {
    pub fn new(policy: Policy) -> Relay {
        Relay {
            policy,
            lane: Lane::new(App::Relay),
            queues: BTreeMap::new(),
            jobs: BTreeMap::new(),
            running: BTreeSet::new(),
            history: History::default(),
            restore_for: None,
            sessions: Vec::new(),
            cancels: Vec::new(),
            deleted: Vec::new(),
            listed: Vec::new(),
            activity: Activity::default(),
        }
    }

    pub fn client_build(&self) -> Option<&str> {
        self.lane.client_build()
    }

    pub fn addon_version(&self) -> Option<u32> {
        self.lane.addon_version()
    }

    pub fn next_slot(&self) -> usize {
        self.lane.next_slot()
    }

    pub fn reset_window(&mut self) {
        self.lane.reset_window();
    }

    pub fn unread(&self) -> usize {
        self.lane.unread()
    }

    /// Takes one frame. The flags of its first record carry the report of the addon.
    pub fn on_frame(&mut self, records: &[Record], now: u32) -> Vec<Outcome> {
        if let Some(first) = records.first() {
            self.take_report(&text(&first.token), &flags::transport(&first.flags));
        }
        records.iter().map(|r| self.on_record(r, now)).collect()
    }

    fn take_report(&mut self, token: &str, flags: &TransportFlags) {
        self.lane.take_report(token, flags);
        self.take_restore_report(token, flags);
    }

    /// A hello from a new token after a saved-data wipe starts a restore. The
    /// `restored` flag of that token ends it, and the older tokens retire (SPEC.md 7.6).
    fn take_restore_report(&mut self, token: &str, flags: &TransportFlags) {
        if flags.restored && self.restore_for.as_deref() == Some(token) {
            self.restore_for = None;
            self.lane.retire_all_but(token);
            return;
        }
        if !flags.hello || self.lane.knows_token(token) {
            return;
        }
        if self.lane.has_tokens() && !self.history.is_empty() {
            self.restore_for = Some(token.to_owned());
        }
        self.lane.add_token(token);
    }

    fn on_record(&mut self, r: &Record, now: u32) -> Outcome {
        let chat = ChatId(text(&r.chat));
        if flags::transport(&r.flags).hello {
            return Outcome::Control;
        }
        let flags = flags::coding(&r.flags);
        if flags.stop {
            self.stop(&chat);
            return Outcome::Control;
        }
        if flags.delete {
            self.delete(chat);
            return Outcome::Control;
        }
        if let Some(answer) = &flags.perm {
            self.activity.answer(&chat, answer);
            return Outcome::Control;
        }
        if let Err(outcome) = self.admit(r, &chat, now) {
            return outcome;
        }
        if let Some(update) = self.lane.update_text() {
            let (token, id) = (text(&r.token), MessageId(r.id));
            self.set_record(&token, &chat, id, Status::Error, update.into());
            return Outcome::WrongVersion;
        }
        if flags.list {
            return self.enqueue_list(r, chat);
        }
        if let Some(session) = &flags.attach {
            return self.attach(r, chat, session, now);
        }
        let agent = flags
            .agent
            .unwrap_or_else(|| self.policy.default_agent.clone());
        let log = ChatLog {
            chat: chat.clone(),
            name: text(&r.name),
            agent: agent.clone(),
            cwd: text(&r.cwd),
            lines: Vec::new(),
        };
        self.history
            .add_message(log, MessageId(r.id), &text(&r.text));
        let folders = &self.policy.folders;
        let request = folder_request(&r.cwd, cfg!(windows));
        let resolved = request.and_then(|cwd| resolve_folder(&folders.roots, &folders.base, &cwd));
        let Some(cwd) = resolved else {
            self.set_record(
                &text(&r.token),
                &chat,
                MessageId(r.id),
                Status::Error,
                BAD_FOLDER.into(),
            );
            return Outcome::BadFolder;
        };
        let Some(permission) = self.policy.agents.get(&agent) else {
            self.set_record(
                &text(&r.token),
                &chat,
                MessageId(r.id),
                Status::Error,
                BAD_AGENT.into(),
            );
            return Outcome::BadAgent;
        };
        let permission = permission.ceiling(flags.level);
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent,
            permission,
            cwd: text(&native_folder(cwd, cfg!(windows))),
            session: if flags.new_session {
                Session::New
            } else {
                Session::Resume
            },
            resume: None,
            text: text(&r.text),
            work: Work::Prompt,
        })
    }

    fn enqueue_list(&mut self, r: &Record, chat: ChatId) -> Outcome {
        let base = self.policy.folders.base.clone();
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: self.policy.default_agent.clone(),
            permission: Permission::Ask,
            cwd: text(&native_folder(base, cfg!(windows))),
            session: Session::New,
            resume: None,
            text: String::new(),
            work: Work::ListSessions,
        })
    }

    fn attach(&mut self, r: &Record, chat: ChatId, session: &str, now: u32) -> Outcome {
        let Some(listed) = self.listed.iter().find(|l| l.id == session).cloned() else {
            self.set_record(
                &text(&r.token),
                &chat,
                MessageId(r.id),
                Status::Error,
                NO_SESSION.into(),
            );
            return Outcome::BadSession;
        };
        self.enqueue_job(Job {
            token: text(&r.token),
            chat,
            id: MessageId(r.id),
            agent: listed.agent,
            permission: Permission::Ask,
            cwd: listed.cwd,
            session: Session::New,
            resume: None,
            text: String::new(),
            work: Work::Attach {
                session: listed.id,
                fork: now.saturating_sub(listed.updated) < ACTIVE_FOR,
            },
        })
    }

    /// Marks the message as seen, or says why not. Every refusal of the chat queue
    /// comes before the lane marks it as seen: a refused message must not count as seen.
    fn admit(&mut self, r: &Record, chat: &ChatId, now: u32) -> Result<(), Outcome> {
        let queued = self.queues.get(chat).map_or(0, |q| q.ids.len());
        if queued >= MAX_QUEUE {
            return Err(Outcome::Refused);
        }
        // Jobs wait under their chat and id. A second token with the same pair waits
        // until the first job leaves, or it overwrites the first job.
        let waiting = self.jobs.get(&(chat.clone(), MessageId(r.id)));
        if waiting.is_some_and(|job| job.token.as_bytes() != r.token) {
            return Err(Outcome::Refused);
        }
        self.lane.admit(&r.token, r.id, now).map_err(|e| match e {
            NotAdmitted::Refused => Outcome::Refused,
            NotAdmitted::Duplicate => Outcome::Duplicate,
        })
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
        if self.running.contains(chat) {
            self.cancels.push(chat.clone());
        }
        let Some(queue) = self.queues.remove(chat) else {
            return;
        };
        for id in queue.ids {
            if let Some(job) = self.jobs.remove(&(chat.clone(), MessageId(id))) {
                self.set_record(&job.token, chat, job.id, Status::Error, STOPPED.into());
            }
        }
    }

    /// The game has no chat to show a reply in, so a reply of the chat could never be
    /// read and would stay in the body for good (SPEC.md 7.3).
    fn delete(&mut self, chat: ChatId) {
        self.stop(&chat);
        self.lane.remove_chat(&chat);
        self.sessions.retain(|s| s.chat != chat);
        self.history.remove(&chat);
        self.deleted.push(chat);
        keep_last(&mut self.deleted, MAX_SESSIONS);
    }

    fn is_deleted(&self, chat: &ChatId) -> bool {
        self.deleted.contains(chat)
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
        let mut job = self.jobs.remove(&(chat.clone(), MessageId(id)))?;
        if job.session == Session::Resume {
            job.resume = self
                .sessions
                .iter()
                .find(|s| s.chat == job.chat && s.agent == job.agent && s.cwd == job.cwd)
                .map(|s| s.id.clone());
        }
        self.running.insert(chat);
        Some(job)
    }

    pub fn take_cancels(&mut self) -> Vec<ChatId> {
        std::mem::take(&mut self.cancels)
    }

    pub fn keep_session(&mut self, job: &Job, id: Option<String>) {
        let Some(id) = id else {
            return;
        };
        if self.is_deleted(&job.chat) {
            return;
        }
        self.sessions.retain(|s| s.chat != job.chat);
        self.sessions.push(AgentSession {
            chat: job.chat.clone(),
            agent: job.agent.clone(),
            cwd: job.cwd.clone(),
            id,
        });
        keep_last(&mut self.sessions, MAX_SESSIONS);
    }

    pub fn step(&mut self, chat: &ChatId, id: MessageId, line: String) {
        self.activity.step(chat, id, line);
    }

    pub fn ask(
        &mut self,
        chat: &ChatId,
        id: MessageId,
        text: Vec<u8>,
        choices: Vec<Choice>,
        now: u32,
    ) -> String {
        self.activity.ask(chat, id, text, choices, now)
    }

    pub fn take_answers(&mut self) -> Vec<(String, Option<usize>)> {
        self.activity.take_answers()
    }

    pub fn is_asked(&self, request: &str) -> bool {
        self.activity.is_open(request)
    }

    pub fn live_file(&self) -> Vec<u8> {
        self.activity.file()
    }

    pub fn finish(&mut self, job: &Job, result: Result<String, String>) {
        self.activity.end(&job.chat, job.id);
        self.running.remove(&job.chat);
        if self.is_deleted(&job.chat) {
            return;
        }
        let (status, text) = match result {
            Ok(text) => (Status::Done, render_reply(&job.work, &text)),
            Err(text) => (Status::Error, text),
        };
        self.set_record(&job.token, &job.chat, job.id, status, text);
    }

    /// Keeps the sessions whose folder is in a root, and answers the list request with
    /// one line per session (SPEC.md 9.6).
    pub fn finish_list(
        &mut self,
        job: &Job,
        found: Result<Vec<(String, SessionInfo)>, String>,
        now: u32,
    ) {
        self.activity.end(&job.chat, job.id);
        self.running.remove(&job.chat);
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                self.set_record(&job.token, &job.chat, job.id, Status::Error, e);
                return;
            }
        };
        let mut listed: Vec<Listed> = found
            .into_iter()
            .filter_map(|(agent, info)| self.to_listed(agent, info))
            .collect();
        listed.sort_by_key(|l| std::cmp::Reverse(l.updated));
        listed.truncate(MAX_LISTED);
        self.listed = listed;
        let text = self.list_text(now);
        self.set_record(&job.token, &job.chat, job.id, Status::Done, text);
    }

    fn to_listed(&self, agent: String, info: SessionInfo) -> Option<Listed> {
        if !flags::is_session_id(&info.id) {
            return None;
        }
        let folders = &self.policy.folders;
        let mut target = path_bytes(std::path::Path::new(&info.cwd));
        // A Windows path starts with its drive. The resolver takes it as absolute only
        // with a `/` first.
        if !target.starts_with(b"/") {
            target.insert(0, b'/');
        }
        let resolved = resolve_folder(&folders.roots, &folders.base, &target)?;
        Some(Listed {
            agent,
            id: info.id,
            folder: text(&relative_folder(&folders.base, &resolved)),
            cwd: text(&native_folder(resolved, cfg!(windows))),
            title: info.title,
            updated: info.updated,
        })
    }

    /// Tab-separated: agent, session, age in seconds, 1 if active, the chat that has
    /// it or nothing, the folder, the name of the folder, and the title.
    fn list_text(&self, now: u32) -> String {
        let mut lines = Vec::new();
        for l in &self.listed {
            let chat = self.sessions.iter().find(|s| s.id == l.id);
            let age = now.saturating_sub(l.updated);
            let name = l
                .cwd
                .rsplit(['/', '\\'])
                .find(|p| !p.is_empty())
                .unwrap_or("");
            let fields = [
                l.agent.clone(),
                l.id.clone(),
                age.to_string(),
                if age < ACTIVE_FOR { "1" } else { "0" }.to_owned(),
                chat.map_or(String::new(), |s| s.chat.0.clone()),
                field(&l.folder),
                field(name),
                field(cut_chars(&l.title, MAX_TITLE)),
            ];
            lines.push(fields.join("\t"));
        }
        lines.join("\n")
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
        let speaker = match status {
            Status::Working => None,
            Status::Done => Some(Speaker::Agent),
            Status::Error => Some(Speaker::Error),
        };
        if let Some(speaker) = speaker {
            self.history.add_reply(chat, speaker, id, &text);
        }
        self.lane.set_record(token, chat, id, status, text);
    }

    pub fn to_state(&self) -> State {
        State {
            lane: self.lane.to_state(),
            waiting: self.waiting_jobs(),
            history: self.history.clone(),
            restore_for: self.restore_for.clone(),
            sessions: self.sessions.clone(),
        }
    }

    fn waiting_jobs(&self) -> Vec<Job> {
        let mut waiting = Vec::new();
        for (chat, queue) in &self.queues {
            for id in &queue.ids {
                waiting.extend(self.jobs.get(&(chat.clone(), MessageId(*id))).cloned());
            }
        }
        waiting
    }

    /// A run that was in progress at the stop ends as an error. It never runs again:
    /// it can have changed files already.
    pub fn from_state(policy: Policy, state: State) -> Relay {
        let mut relay = Relay::new(policy);
        relay.lane = Lane::from_state(App::Relay, state.lane);
        relay.history = state.history;
        relay.restore_for = state.restore_for;
        relay.sessions = state.sessions;
        for job in state.waiting {
            let queue = relay
                .queues
                .entry(job.chat.clone())
                .or_insert(ChatQueue { ids: Vec::new() });
            queue.ids.push(job.id.0);
            relay.jobs.insert((job.chat.clone(), job.id), job);
        }
        let jobs = &relay.jobs;
        let ended = relay
            .lane
            .end_working(|chat, id| jobs.contains_key(&(chat.clone(), id)), RESTARTED);
        for (chat, id) in ended {
            relay
                .history
                .add_reply(&chat, Speaker::Error, id, RESTARTED);
        }
        relay
    }

    /// An empty token matches no addon, so the file stays harmless with no restore.
    pub fn restore_file(&self) -> Vec<u8> {
        let Some(token) = &self.restore_for else {
            return restore_body(App::Relay, b"", &[]);
        };
        restore_body(
            App::Relay,
            token.as_bytes(),
            &prepare_restore(&self.history.to_restore()),
        )
    }

    pub fn body(&self, now: u32) -> Vec<u8> {
        self.lane.body(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_790_211_079;

    fn policy() -> Policy {
        Policy {
            folders: Folders {
                roots: vec![b"/home/x/Code".to_vec()],
                base: b"/home/x/Code".to_vec(),
            },
            agents: [
                ("claude".to_owned(), Permission::AutoEdit),
                ("codex".to_owned(), Permission::Ask),
            ]
            .into(),
            default_agent: "claude".into(),
        }
    }

    fn relay() -> Relay {
        Relay::new(policy())
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
    fn a_second_token_with_the_same_chat_and_id_waits_for_the_first() {
        let other = || Record {
            token: b"other".to_vec(),
            ..record("c1", 1, "", "from a new token")
        };
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "first")], NOW);
        assert_eq!(relay.on_frame(&[other()], NOW), [Outcome::Refused]);
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);

        assert_eq!(relay.on_frame(&[other()], NOW), [Outcome::Accepted]);
        assert_eq!(run_all(&mut relay).len(), 1);
        assert!(!body(&relay).contains("status = \"working\""));
    }

    fn from_token(token: &str, r: Record) -> Record {
        Record {
            token: token.as_bytes().to_vec(),
            ..r
        }
    }

    fn restore_text(relay: &Relay) -> String {
        String::from_utf8(relay.restore_file()).unwrap()
    }

    /// One chat with a finished message from `tok`, then a hello from `new`.
    fn wiped() -> Relay {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        relay.on_frame(&[record("c1", 1, "", "before the wipe")], NOW);
        run_all(&mut relay);
        relay.on_frame(&[from_token("new", record("relay", 0, "h", ""))], NOW);
        relay
    }

    #[test]
    fn the_first_token_gets_no_restore() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        assert!(restore_text(&relay).contains("token = \"\""));
    }

    #[test]
    fn a_hello_from_a_new_token_gets_the_chats_back() {
        let relay = wiped();
        let text = restore_text(&relay);
        assert!(text.contains("token = \"new\""));
        assert!(text.contains("text = \"before the wipe\""));
        assert!(text.contains("text = \"\\027M1\\010p\\031echo: before the wipe\\010\""));
    }

    #[test]
    fn a_done_reply_and_its_history_hold_blocks_but_an_error_stays_plain() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        relay.on_frame(&[record("c2", 2, "", "b")], NOW);
        let first = relay.next_job().unwrap();
        let second = relay.next_job().unwrap();
        relay.finish(&first, Ok("**done**".into()));
        relay.finish(&second, Err("no **luck**".into()));

        let state = relay.to_state();
        let texts: Vec<&str> = state.lane.records.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["\x1bM1\np\x1f|cffffd100done|r\n", "no **luck**"]);
        let replies: Vec<Vec<u8>> = relay
            .history
            .to_restore()
            .into_iter()
            .map(|c| c.history[1].text.clone())
            .collect();
        assert_eq!(replies, [texts[0].as_bytes(), texts[1].as_bytes()]);
    }

    #[test]
    fn the_restored_flag_ends_the_restore_and_retires_the_old_token() {
        let mut relay = wiped();
        relay.on_frame(&[record("c1", 2, "", "still running")], NOW);
        let job = relay.next_job().unwrap();

        relay.on_frame(
            &[from_token("new", record("relay", 0, "h;restored", ""))],
            NOW,
        );
        relay.finish(&job, Ok("late".into()));

        assert!(restore_text(&relay).contains("token = \"\""));
        assert!(!body(&relay).contains("echo: before the wipe"));
        assert!(!body(&relay).contains("late"));
    }

    #[test]
    fn a_restore_goes_on_after_a_bridge_restart() {
        let relay = wiped();
        assert!(restore_text(&restart(&relay)).contains("token = \"new\""));
    }

    #[test]
    fn a_message_runs_at_the_lower_of_the_config_and_the_game_level() {
        let mut relay = relay();
        relay.on_frame(
            &[
                record("c1", 1, "level=full-auto", "raise"),
                record("c2", 2, "level=ask", "lower"),
                record("c3", 3, "agent=codex;level=auto-edit", "raise codex"),
            ],
            NOW,
        );
        let levels: Vec<Permission> = run_all(&mut relay).iter().map(|j| j.permission).collect();
        assert_eq!(
            levels,
            [Permission::AutoEdit, Permission::Ask, Permission::Ask]
        );
    }

    #[test]
    fn an_agent_that_is_not_in_the_config_never_runs() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record("c1", 1, "agent=gemini", "hi")], NOW);
        assert_eq!(outcomes, [Outcome::BadAgent]);
        assert!(run_all(&mut relay).is_empty());
        assert!(body(&relay).contains(BAD_AGENT));
    }

    #[test]
    fn the_build_counts_as_good_only_when_both_channels_work() {
        let mut relay = relay();
        relay.on_frame(
            &[record("relay", 0, "h;build=70009;out=shot;in=missing", "")],
            NOW,
        );
        assert_eq!(relay.client_build(), None);
        relay.on_frame(
            &[record("relay", 0, "h;build=70009;out=shot;in=slots", "")],
            NOW,
        );
        assert_eq!(relay.client_build(), Some("70009"));
        relay.on_frame(
            &[record("relay", 0, "h;build=70100;out=fail;in=slots", "")],
            NOW,
        );
        assert_eq!(restart(&relay).client_build(), Some("70009"));
    }

    fn first_run(relay: &mut Relay) {
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.keep_session(&job, Some("s9".into()));
        relay.finish(&job, Ok(String::new()));
    }

    #[test]
    fn the_next_message_of_a_chat_resumes_its_agent_session() {
        let mut relay = relay();
        first_run(&mut relay);
        relay.on_frame(&[record("c1", 2, "", "b")], NOW);
        assert_eq!(relay.next_job().unwrap().resume.as_deref(), Some("s9"));
    }

    #[test]
    fn a_new_session_flag_another_agent_or_a_restart_keeps_the_rules() {
        let mut relay = relay();
        first_run(&mut relay);
        relay.on_frame(&[record("c1", 2, "n", "fresh")], NOW);
        assert_eq!(run_all(&mut relay)[0].resume, None);
        relay.on_frame(&[record("c1", 3, "agent=codex", "other agent")], NOW);
        assert_eq!(run_all(&mut relay)[0].resume, None);

        let mut restarted = restart(&relay);
        restarted.on_frame(&[record("c1", 4, "", "after restart")], NOW);
        assert_eq!(restarted.next_job().unwrap().resume.as_deref(), Some("s9"));
    }

    #[test]
    fn stop_signals_the_run_in_progress_once() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "long task")], NOW);
        relay.next_job().unwrap();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert_eq!(relay.take_cancels(), [ChatId("c1".into())]);
        assert!(relay.take_cancels().is_empty());
    }

    #[test]
    fn stop_with_no_run_in_progress_signals_nothing() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 0, "stop", "")], NOW);
        assert!(relay.take_cancels().is_empty());
    }

    #[test]
    fn the_report_gives_the_addon_version() {
        let mut relay = relay();
        assert_eq!(relay.addon_version(), None);
        relay.on_frame(&[record("relay", 0, "h;ver=1", "")], NOW);
        assert_eq!(relay.addon_version(), Some(1));
        relay.on_frame(&[record("relay", 0, "h", "")], NOW);
        assert_eq!(
            relay.addon_version(),
            Some(1),
            "a report with no version keeps the last one"
        );
    }

    #[test]
    fn a_relay_addon_newer_than_the_bridge_gets_the_update_text_and_never_runs() {
        let mut relay = relay();
        let frame = [record("relay", 0, "h;ver=2", ""), record("c1", 1, "", "hi")];
        let outcomes = relay.on_frame(&frame, NOW);
        assert_eq!(outcomes, [Outcome::Control, Outcome::WrongVersion]);
        assert!(relay.next_job().is_none());
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(body.contains(crate::story::UPDATE_BRIDGE), "{body}");
    }

    #[test]
    fn a_relay_addon_older_than_the_bridge_is_asked_to_reload() {
        let mut relay = relay();
        let frame = [record("relay", 0, "h;ver=0", ""), record("c1", 1, "", "hi")];
        assert_eq!(relay.on_frame(&frame, NOW)[1], Outcome::WrongVersion);
        let body = String::from_utf8(relay.body(NOW)).unwrap();
        assert!(body.contains(crate::versions::RELOAD_RELAY), "{body}");
    }

    #[test]
    fn a_relay_addon_with_a_supported_or_no_version_runs() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "no version yet")], NOW);
        relay.on_frame(
            &[record("relay", 0, "h;ver=1", ""), record("c2", 2, "", "hi")],
            NOW,
        );
        assert!(relay.next_job().is_some());
        assert!(relay.next_job().is_some());
    }

    fn restart(relay: &Relay) -> Relay {
        Relay::from_state(policy(), relay.to_state())
    }

    #[test]
    fn a_message_that_ran_before_a_restart_never_runs_again() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "once")], NOW);
        run_all(&mut relay);

        let mut restarted = restart(&relay);
        assert_eq!(
            restarted.on_frame(&[record("c1", 1, "", "once")], NOW),
            [Outcome::Duplicate]
        );
        assert!(body(&restarted).contains("echo: once"));
        assert!(run_all(&mut restarted).is_empty());
    }

    #[test]
    fn a_restart_ends_the_run_in_progress_as_an_error_and_keeps_the_queue() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        relay.next_job().unwrap();
        relay.on_frame(&[record("relay", 0, "h;next=57", "")], NOW);

        let mut restarted = restart(&relay);
        assert!(body(&restarted).contains(RESTARTED));
        assert_eq!(restarted.next_slot(), 57);
        let jobs = run_all(&mut restarted);
        assert_eq!(
            jobs.iter().map(|j| j.id).collect::<Vec<_>>(),
            [MessageId(2)]
        );
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
    fn delete_drops_the_replies_the_session_and_the_history_of_the_chat() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c2", 2, "", "b")], NOW);
        let jobs = run_all(&mut relay);
        relay.keep_session(&jobs[0], Some("s1".into()));

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);

        assert!(
            !body(&relay).contains("echo: a"),
            "no reply of c1 is left to block the body"
        );
        assert!(body(&relay).contains("echo: b"));
        assert_eq!(relay.unread(), 1);
        let state = relay.to_state();
        assert!(state.sessions.is_empty());
        assert!(state.history.to_restore().iter().all(|c| c.id != b"c1"));
    }

    #[test]
    fn a_run_of_a_deleted_chat_ends_with_no_reply_and_no_session() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a"), record("c1", 2, "", "b")], NOW);
        let running = relay.next_job().unwrap();

        relay.on_frame(&[record("c1", 0, "d", "")], NOW);
        assert_eq!(relay.take_cancels(), [ChatId("c1".into())]);
        relay.keep_session(&running, Some("s1".into()));
        relay.finish(&running, Err("Stopped.".into()));

        assert_eq!(relay.unread(), 0);
        assert!(relay.to_state().sessions.is_empty());
        assert!(relay.next_job().is_none(), "the waiting message never runs");
    }

    fn info(id: &str, cwd: &str, title: &str, updated: u32) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            cwd: cwd.into(),
            title: title.into(),
            updated,
        }
    }

    /// Runs a list request of chat `relay` with what the agents found.
    fn list(relay: &mut Relay, id: u32, found: Vec<(String, SessionInfo)>) -> String {
        relay.on_frame(&[record("relay", id, "list", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(job.work, Work::ListSessions);
        relay.finish_list(&job, Ok(found), NOW);
        body(relay)
    }

    #[test]
    fn a_list_shows_the_sessions_in_the_roots_newest_first() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![
                (
                    "claude".into(),
                    info("old", "/home/x/Code/app", "Old work", NOW - 7200),
                ),
                (
                    "claude".into(),
                    info("new", "/home/x/Code/app", "New work", NOW - 60),
                ),
                ("claude".into(), info("ssh", "/home/x/.ssh", "Keys", NOW)),
                ("codex".into(), info("bad;id", "/home/x/Code", "Bad", NOW)),
            ],
        );
        assert_eq!(
            relay.list_text(NOW),
            "claude\tnew\t60\t1\t\tapp\tapp\tNew work\n\
             claude\told\t7200\t0\t\tapp\tapp\tOld work",
            "a session outside the roots or with a bad id never shows"
        );
    }

    #[test]
    fn a_title_with_line_breaks_stays_on_one_line_and_is_cut() {
        let mut relay = relay();
        let title = format!("a\tb\nc{}", "é".repeat(80));
        list(
            &mut relay,
            1,
            vec![("claude".into(), info("s1", "/home/x/Code", &title, NOW))],
        );
        let text = relay.list_text(NOW);
        assert_eq!(text.lines().count(), 1);
        let shown = text.rsplit('\t').next().unwrap();
        assert!(shown.starts_with("a b c"));
        assert!(shown.len() <= MAX_TITLE);
    }

    #[test]
    fn a_failed_list_publishes_the_error() {
        let mut relay = relay();
        relay.on_frame(&[record("relay", 1, "list", "")], NOW);
        let job = relay.next_job().unwrap();
        relay.finish_list(&job, Err("agent crashed".into()), NOW);
        assert!(body(&relay).contains(r#"status = "error", text = "agent crashed""#));
    }

    #[test]
    fn an_attach_continues_a_listed_session_and_later_messages_resume_it() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![(
                "codex".into(),
                info("s1", "/home/x/Code/app", "Work", NOW - 3600),
            )],
        );

        relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        let attach = relay.next_job().unwrap();
        assert_eq!(attach.agent, "codex");
        assert_eq!(attach.cwd, "/home/x/Code/app");
        assert_eq!(
            attach.work,
            Work::Attach {
                session: "s1".into(),
                fork: false
            }
        );
        relay.keep_session(&attach, Some("s1".into()));
        relay.finish(&attach, Ok("last exchange".into()));

        relay.on_frame(&[record_in("app", "c9", 3, "agent=codex", "go on")], NOW);
        let next = relay.next_job().unwrap();
        assert_eq!(next.resume.as_deref(), Some("s1"));
    }

    #[test]
    fn an_attach_to_an_active_session_forks_it() {
        let mut relay = relay();
        list(
            &mut relay,
            1,
            vec![(
                "claude".into(),
                info("s1", "/home/x/Code", "Work", NOW - 10),
            )],
        );
        relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        let job = relay.next_job().unwrap();
        assert_eq!(
            job.work,
            Work::Attach {
                session: "s1".into(),
                fork: true
            }
        );
    }

    #[test]
    fn an_attach_to_a_session_that_the_list_did_not_show_gets_an_error() {
        let mut relay = relay();
        let outcomes = relay.on_frame(&[record("c9", 2, "attach=s1", "")], NOW);
        assert_eq!(outcomes, [Outcome::BadSession]);
        assert!(relay.next_job().is_none());
        assert!(body(&relay).contains(NO_SESSION));
    }

    #[test]
    fn a_listed_session_of_a_chat_names_that_chat() {
        let mut relay = relay();
        relay.on_frame(&[record("c1", 1, "", "a")], NOW);
        let job = relay.next_job().unwrap();
        relay.keep_session(&job, Some("s1".into()));
        relay.finish(&job, Ok("done".into()));

        list(
            &mut relay,
            2,
            vec![(
                "claude".into(),
                info("s1", "/home/x/Code", "Work", NOW - 3600),
            )],
        );
        assert!(relay.list_text(NOW).contains("\t0\tc1\t"));
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
