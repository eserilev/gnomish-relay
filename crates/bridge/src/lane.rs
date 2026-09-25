//! The part of the bridge state that belongs to one app (SPEC.md 9.7, decision 4): the
//! replay store, the rate limit, the slot body, the slot window, and the tokens. It
//! knows nothing of agents, so an app with no agents can use it. No I/O here.

use protocol::apps::App;
use protocol::rate::{RateLimiter, admit_message};
use protocol::seen::{self, Seen, admit, new_seen};
use protocol::slot::{MAX_REPLIES, Reply, Status, prepare_replies, slot_body};
use serde::{Deserialize, Serialize};

use crate::flags::{Channel, Flags};

/// More tokens than this means many wipes. The oldest ones then go.
const MAX_TOKENS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChatId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageId(pub u32);

/// Why a message did not count as seen.
#[derive(Debug, PartialEq, Eq)]
pub enum NotAdmitted {
    /// The addon sends it again later.
    Refused,
    Duplicate,
}

/// The part of `state.json` that belongs to the lane. A field that an older bridge did
/// not write loads as its default.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LaneState {
    pub next_slot: usize,
    /// The replay store, oldest first.
    pub seen: Vec<(String, u32)>,
    pub records: Vec<SavedRecord>,
    pub tokens: Vec<String>,
    pub retired: Vec<String>,
    pub client_build: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SavedRecord {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub status: SavedStatus,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub enum SavedStatus {
    Working,
    Done,
    Error,
}

struct Entry {
    token: String,
    chat: ChatId,
    id: MessageId,
    status: Status,
    text: String,
}

impl Entry {
    fn to_saved(&self) -> SavedRecord {
        SavedRecord {
            token: self.token.clone(),
            chat: self.chat.clone(),
            id: self.id,
            status: match self.status {
                Status::Working => SavedStatus::Working,
                Status::Done => SavedStatus::Done,
                Status::Error => SavedStatus::Error,
            },
            text: self.text.clone(),
        }
    }

    fn from_saved(saved: SavedRecord) -> Entry {
        Entry {
            token: saved.token,
            chat: saved.chat,
            id: saved.id,
            status: match saved.status {
                SavedStatus::Working => Status::Working,
                SavedStatus::Done => Status::Done,
                SavedStatus::Error => Status::Error,
            },
            text: saved.text,
        }
    }
}

pub struct Lane {
    app: App,
    seen: Seen,
    limiter: RateLimiter,
    /// Every record the addon has not read, newest last.
    records: Vec<Entry>,
    next_slot: usize,
    /// The tokens that sent a hello, oldest first.
    tokens: Vec<String>,
    /// Tokens of wiped saved data. Their records never go into the body again.
    retired: Vec<String>,
    /// The last client build whose screenshots and slots both worked (SPEC.md 7.8).
    client_build: Option<String>,
    /// The protocol version that the addon reported last.
    addon_version: Option<u32>,
}

pub(crate) fn keep_last<T>(list: &mut Vec<T>, max: usize) {
    if list.len() > max {
        list.drain(..list.len() - max);
    }
}

impl Lane {
    pub fn new(app: App) -> Lane {
        Lane {
            app,
            seen: new_seen(),
            limiter: RateLimiter { times: Vec::new() },
            records: Vec::new(),
            next_slot: 1,
            tokens: Vec::new(),
            retired: Vec::new(),
            client_build: None,
            addon_version: None,
        }
    }

    pub fn client_build(&self) -> Option<&str> {
        self.client_build.as_deref()
    }

    pub fn addon_version(&self) -> Option<u32> {
        self.addon_version
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

    /// The transport part of the report on the first record of a frame (SPEC.md 7.1.1).
    pub fn take_report(&mut self, token: &str, flags: &Flags) {
        if let Some(next) = flags.next {
            self.next_slot = next.max(1);
        }
        self.records.retain(|e| {
            let read = e.token == token && flags.read.contains(&e.id.0);
            !read || matches!(e.status, Status::Working)
        });
        if flags.version.is_some() {
            self.addon_version = flags.version;
        }
        let works = Some(Channel::Works);
        if flags.out == works && flags.inbound == works && flags.build.is_some() {
            self.client_build.clone_from(&flags.build);
        }
    }

    pub fn knows_token(&self, token: &str) -> bool {
        self.tokens.iter().any(|t| t == token)
    }

    pub fn has_tokens(&self) -> bool {
        !self.tokens.is_empty()
    }

    pub fn add_token(&mut self, token: &str) {
        self.tokens.push(token.to_owned());
        keep_last(&mut self.tokens, MAX_TOKENS);
    }

    /// After a saved-data wipe, only the new token stays. The records of the others
    /// leave the body.
    pub fn retire_all_but(&mut self, token: &str) {
        let old = std::mem::replace(&mut self.tokens, vec![token.to_owned()]);
        self.retired.extend(old.into_iter().filter(|t| t != token));
        keep_last(&mut self.retired, MAX_TOKENS);
        self.records.retain(|e| e.token == token);
    }

    /// Marks the message as seen, or says why not.
    ///
    /// The order is a trap. Every refusal comes before `admit`, because a refused
    /// message must not count as seen. The rate limiter changes only after `admit`, so
    /// a duplicate uses no rate.
    pub fn admit(&mut self, token: &[u8], id: u32, now: u32) -> Result<(), NotAdmitted> {
        if self.records.len() >= MAX_REPLIES {
            return Err(NotAdmitted::Refused);
        }
        let (rate_ok, limiter) = admit_message(&self.limiter, now);
        if !rate_ok {
            return Err(NotAdmitted::Refused);
        }
        let (fresh, seen) = admit(std::mem::replace(&mut self.seen, new_seen()), token, id);
        self.seen = seen;
        if !fresh {
            return Err(NotAdmitted::Duplicate);
        }
        self.limiter = limiter;
        Ok(())
    }

    /// Puts the record of a message at the newest place with its new state.
    pub fn set_record(
        &mut self,
        token: &str,
        chat: &ChatId,
        id: MessageId,
        status: Status,
        text: String,
    ) {
        if self.retired.iter().any(|t| t == token) {
            return;
        }
        self.records.retain(|e| !(e.token == token && e.id == id));
        self.records.push(Entry {
            token: token.to_owned(),
            chat: chat.clone(),
            id,
            status,
            text,
        });
    }

    pub fn remove_chat(&mut self, chat: &ChatId) {
        self.records.retain(|e| e.chat != *chat);
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
        slot_body(self.app, now, &prepare_replies(&replies))
    }

    /// The rate limiter is not in the state: a restart gives a fresh minute.
    pub fn to_state(&self) -> LaneState {
        LaneState {
            next_slot: self.next_slot,
            seen: self
                .seen
                .entries
                .iter()
                .map(|e| (String::from_utf8_lossy(&e.token).into_owned(), e.id))
                .collect(),
            records: self.records.iter().map(Entry::to_saved).collect(),
            tokens: self.tokens.clone(),
            retired: self.retired.clone(),
            client_build: self.client_build.clone(),
        }
    }

    pub fn from_state(app: App, state: LaneState) -> Lane {
        let mut lane = Lane::new(app);
        lane.next_slot = state.next_slot.max(1);
        lane.seen.entries = state
            .seen
            .into_iter()
            .map(|(token, id)| seen::Entry {
                token: token.into_bytes(),
                id,
            })
            .collect();
        lane.records = state.records.into_iter().map(Entry::from_saved).collect();
        lane.tokens = state.tokens;
        lane.retired = state.retired;
        lane.client_build = state.client_build;
        lane
    }

    /// Each record that was working at a stop, and has no waiting job, ends as `text`.
    /// Returns the ones it changed.
    pub fn end_working(
        &mut self,
        waits: impl Fn(&ChatId, MessageId) -> bool,
        text: &str,
    ) -> Vec<(ChatId, MessageId)> {
        let mut ended = Vec::new();
        for entry in &mut self.records {
            if matches!(entry.status, Status::Working) && !waits(&entry.chat, entry.id) {
                entry.status = Status::Error;
                entry.text = text.into();
                ended.push((entry.chat.clone(), entry.id));
            }
        }
        ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags;

    const NOW: u32 = 1_790_211_079;

    fn chat() -> ChatId {
        ChatId("c1".into())
    }

    #[test]
    fn a_lane_admits_a_message_once() {
        let mut lane = Lane::new(App::Relay);
        assert_eq!(lane.admit(b"tok", 1, NOW), Ok(()));
        assert_eq!(lane.admit(b"tok", 1, NOW), Err(NotAdmitted::Duplicate));
        assert_eq!(lane.admit(b"other", 1, NOW), Ok(()));
    }

    #[test]
    fn a_full_body_refuses_before_the_message_counts_as_seen() {
        let mut lane = Lane::new(App::Relay);
        for id in 0..30 {
            lane.set_record("tok", &chat(), MessageId(id), Status::Done, String::new());
        }
        assert_eq!(lane.admit(b"tok", 99, NOW), Err(NotAdmitted::Refused));
        let read: Vec<String> = (0..30).map(|id| id.to_string()).collect();
        lane.take_report(
            "tok",
            &flags::parse(format!("read={}", read.join(",")).as_bytes()),
        );
        assert_eq!(lane.admit(b"tok", 99, NOW), Ok(()));
    }

    #[test]
    fn a_record_of_a_retired_token_never_enters_the_body() {
        let mut lane = Lane::new(App::Relay);
        lane.add_token("old");
        lane.add_token("new");
        lane.set_record("old", &chat(), MessageId(1), Status::Done, "a".into());
        lane.retire_all_but("new");
        lane.set_record("old", &chat(), MessageId(2), Status::Done, "b".into());
        assert_eq!(lane.unread(), 0);
        assert!(lane.knows_token("new"));
        assert!(!lane.knows_token("old"));
    }

    #[test]
    fn a_lane_writes_the_body_global_of_its_app() {
        let relay = Lane::new(App::Relay).body(NOW);
        let timeways = Lane::new(App::Timeways).body(NOW);
        assert!(relay.starts_with(b"GnomishRelay_SlotData = {"));
        assert!(timeways.starts_with(b"Timeways_SlotData = {"));
    }

    #[test]
    fn a_lane_state_loads_the_same() {
        let mut lane = Lane::new(App::Relay);
        lane.admit(b"tok", 7, NOW).unwrap();
        lane.add_token("tok");
        lane.set_record("tok", &chat(), MessageId(7), Status::Working, String::new());
        lane.take_report("tok", &flags::parse(b"next=9;build=7;out=shot;in=slots"));
        let state = lane.to_state();
        assert_eq!(
            Lane::from_state(App::Relay, lane.to_state()).to_state(),
            state
        );
        assert_eq!(state.next_slot, 9);
    }
}
