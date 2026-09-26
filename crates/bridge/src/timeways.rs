//! The Timeways app over its lane (SPEC.md 9.7, decisions 4, 6, and 7). It reads only the
//! transport flags, holds no agents, and has no restore. Its messages wait in a queue
//! for the story program. No I/O here.

use protocol::apps::App;
use protocol::record::Record;
use protocol::slot::Status;

use crate::flags::{self, TransportFlags};
use crate::lane::{ChatId, Lane, LaneState, MessageId, NotAdmitted};
use crate::relay::Outcome;

/// The reply to each message until the story program runs (SPEC.md 9.7, step 5).
pub const NO_STORY: &str = "Timeways story program not running.";
const NO_FOLDER: &str = "Timeways takes no folder.";
const RESTARTED: &str = "Stopped: the bridge restarted.";

/// A message for the story program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryMessage {
    pub token: String,
    pub chat: ChatId,
    pub id: MessageId,
    pub name: String,
    pub text: String,
}

pub struct Timeways {
    lane: Lane,
    /// Messages that the lane marked as seen, oldest first. The body caps them at 30.
    story: Vec<StoryMessage>,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Timeways {
    pub fn new() -> Timeways {
        Timeways {
            lane: Lane::new(App::Timeways),
            story: Vec::new(),
        }
    }

    /// A message that was working at the stop ends as an error, so the addon never waits
    /// for it.
    pub fn from_state(state: LaneState) -> Timeways {
        let mut lane = Lane::from_state(App::Timeways, state);
        lane.end_working(|_, _| false, RESTARTED);
        Timeways {
            lane,
            story: Vec::new(),
        }
    }

    pub fn to_state(&self) -> LaneState {
        self.lane.to_state()
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

    pub fn body(&self, now: u32) -> Vec<u8> {
        self.lane.body(now)
    }

    /// Takes one frame. The flags of its first record carry the report of the addon.
    pub fn on_frame(&mut self, records: &[Record], now: u32) -> Vec<Outcome> {
        if let Some(first) = records.first() {
            self.take_report(&text(&first.token), &flags::transport(&first.flags));
        }
        records.iter().map(|r| self.on_record(r, now)).collect()
    }

    /// A new token never starts a restore: the story state lives on the desktop.
    fn take_report(&mut self, token: &str, flags: &TransportFlags) {
        self.lane.take_report(token, flags);
        if flags.hello && !self.lane.knows_token(token) {
            self.lane.add_token(token);
        }
    }

    fn on_record(&mut self, r: &Record, now: u32) -> Outcome {
        if flags::transport(&r.flags).hello {
            return Outcome::Control;
        }
        if let Err(e) = self.lane.admit(&r.token, r.id, now) {
            return match e {
                NotAdmitted::Refused => Outcome::Refused,
                NotAdmitted::Duplicate => Outcome::Duplicate,
            };
        }
        let message = StoryMessage {
            token: text(&r.token),
            chat: ChatId(text(&r.chat)),
            id: MessageId(r.id),
            name: text(&r.name),
            text: text(&r.text),
        };
        if !r.cwd.is_empty() {
            self.set_reply(&message, Status::Error, NO_FOLDER.into());
            return Outcome::BadFolder;
        }
        self.set_reply(&message, Status::Working, String::new());
        self.story.push(message);
        Outcome::Accepted
    }

    /// The messages for the story program, oldest first. Each one needs an `answer`.
    pub fn take_messages(&mut self) -> Vec<StoryMessage> {
        std::mem::take(&mut self.story)
    }

    pub fn answer(&mut self, message: &StoryMessage, reply: Result<String, String>) {
        match reply {
            Ok(text) => self.set_reply(message, Status::Done, text),
            Err(text) => self.set_reply(message, Status::Error, text),
        }
    }

    fn set_reply(&mut self, message: &StoryMessage, status: Status, text: String) {
        self.lane
            .set_record(&message.token, &message.chat, message.id, status, text);
    }
}

impl Default for Timeways {
    fn default() -> Timeways {
        Timeways::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Permission, Policy};
    use crate::relay::{Folders, Relay};

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

    fn body(timeways: &Timeways) -> String {
        String::from_utf8(timeways.body(NOW)).unwrap()
    }

    fn relay() -> Relay {
        Relay::new(Policy {
            folders: Folders {
                roots: vec![b"/home/x".to_vec()],
                base: b"/home/x".to_vec(),
            },
            agents: [("claude".to_owned(), Permission::FullAuto)].into(),
            default_agent: "claude".into(),
        })
    }

    #[test]
    fn a_message_waits_for_the_story_program_once() {
        let mut timeways = Timeways::new();
        let frame = [record("c1", 1, "", "look around")];
        assert_eq!(timeways.on_frame(&frame, NOW), [Outcome::Accepted]);
        assert_eq!(timeways.on_frame(&frame, NOW), [Outcome::Duplicate]);
        let messages = timeways.take_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "look around");
        assert!(timeways.take_messages().is_empty());
        assert!(body(&timeways).contains(r#"id = 1, status = "working""#));
    }

    #[test]
    fn an_answer_goes_into_the_timeways_body() {
        let mut timeways = Timeways::new();
        timeways.on_frame(&[record("c1", 1, "", "hi")], NOW);
        let message = timeways.take_messages().remove(0);
        timeways.answer(&message, Err(NO_STORY.into()));
        let body = body(&timeways);
        assert!(body.starts_with("Timeways_SlotData = {"));
        assert!(
            body.contains(
                r#"id = 1, status = "error", text = "Timeways story program not running.""#
            )
        );
    }

    #[test]
    fn a_record_with_a_folder_is_refused_and_never_reaches_the_story() {
        let mut timeways = Timeways::new();
        let with_folder = Record {
            cwd: b"Code".to_vec(),
            ..record("c1", 1, "", "hi")
        };
        assert_eq!(timeways.on_frame(&[with_folder], NOW), [Outcome::BadFolder]);
        assert!(timeways.take_messages().is_empty());
        assert!(body(&timeways).contains(NO_FOLDER));
    }

    #[test]
    fn a_permission_answer_in_a_timeways_record_does_nothing() {
        let mut timeways = Timeways::new();
        let outcomes =
            timeways.on_frame(&[record("c1", 1, "perm=p1:o1:0123456789abcdef", "")], NOW);
        assert_eq!(outcomes, [Outcome::Accepted], "it is a plain story message");
        assert_eq!(timeways.take_messages()[0].text, "");
    }

    #[test]
    fn coding_flags_in_a_timeways_record_do_nothing() {
        let mut timeways = Timeways::new();
        timeways.on_frame(&[record("c1", 1, "", "first")], NOW);
        for (id, flag) in (2..).zip([
            "stop",
            "d",
            "n",
            "list",
            "attach=s1",
            "agent=codex",
            "level=full-auto",
        ]) {
            let outcomes = timeways.on_frame(&[record("c1", id, flag, "more")], NOW + id * 7);
            assert_eq!(outcomes, [Outcome::Accepted], "{flag}");
        }
        assert_eq!(timeways.take_messages().len(), 8);
        assert!(body(&timeways).contains(r#"id = 1, status = "working""#));
    }

    #[test]
    fn a_timeways_hello_from_a_new_token_starts_no_restore() {
        let mut timeways = Timeways::new();
        timeways.on_frame(&[record("tw", 0, "h", "")], NOW);
        let mut new = record("tw", 0, "h", "");
        new.token = b"new".to_vec();
        timeways.on_frame(&[new], NOW);
        let state = timeways.to_state();
        assert_eq!(state.tokens, ["tok", "new"]);
        assert!(state.retired.is_empty(), "no token retires");
    }

    #[test]
    fn a_read_flag_takes_a_final_reply_out_of_the_timeways_body() {
        let mut timeways = Timeways::new();
        timeways.on_frame(&[record("c1", 1, "", "hi")], NOW);
        let message = timeways.take_messages().remove(0);
        timeways.answer(&message, Ok("a story".into()));
        timeways.on_frame(&[record("tw", 0, "h;read=1;next=4", "")], NOW);
        assert_eq!(timeways.unread(), 0);
        assert_eq!(timeways.next_slot(), 4);
    }

    #[test]
    fn a_restart_ends_a_waiting_message_as_an_error_and_keeps_the_seen_store() {
        let mut timeways = Timeways::new();
        timeways.on_frame(&[record("c1", 1, "", "hi")], NOW);

        let mut restarted = Timeways::from_state(timeways.to_state());
        assert!(body(&restarted).contains(RESTARTED));
        assert!(restarted.take_messages().is_empty());
        assert_eq!(
            restarted.on_frame(&[record("c1", 1, "", "hi")], NOW),
            [Outcome::Duplicate]
        );
    }

    #[test]
    fn a_replay_in_one_lane_does_not_mark_the_message_as_seen_in_the_other() {
        let mut relay = relay();
        let mut timeways = Timeways::new();
        let frame = [record("c1", 1, "", "same token and id")];
        assert_eq!(timeways.on_frame(&frame, NOW), [Outcome::Accepted]);
        assert_eq!(timeways.on_frame(&frame, NOW), [Outcome::Duplicate]);
        assert_eq!(relay.on_frame(&frame, NOW), [Outcome::Accepted]);
    }

    #[test]
    fn a_full_timeways_body_does_not_block_the_relay() {
        let mut relay = relay();
        let mut timeways = Timeways::new();
        for id in 0..30 {
            timeways.on_frame(&[record("c1", id, "", "x")], NOW + id * 7);
        }
        assert_eq!(timeways.unread(), 30);
        assert_eq!(
            timeways.on_frame(&[record("c1", 30, "", "x")], NOW + 300),
            [Outcome::Refused]
        );
        assert_eq!(
            relay.on_frame(&[record("c1", 30, "", "x")], NOW + 300),
            [Outcome::Accepted]
        );
    }
}
