//! The last messages of each chat, for the restore bundle (SPEC.md 7.6).

use protocol::restore::{Chat, Entry, MAX_CHATS, MAX_ENTRY_TEXT, MAX_HISTORY, Role};
use serde::{Deserialize, Serialize};

use crate::relay::{ChatId, MessageId};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    User,
    Agent,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub speaker: Speaker,
    pub id: MessageId,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatLog {
    pub chat: ChatId,
    pub name: String,
    pub agent: String,
    pub cwd: String,
    pub lines: Vec<Line>,
}

/// The chats with the latest activity last. It keeps only what a bundle holds.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    chats: Vec<ChatLog>,
}

/// The core cuts at a byte count. This cut keeps whole characters.
fn cut(text: &str) -> String {
    let mut end = text.len().min(MAX_ENTRY_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl History {
    pub fn is_empty(&self) -> bool {
        self.chats.is_empty()
    }

    pub fn add_message(&mut self, log: ChatLog, id: MessageId, text: &str) {
        let lines = self.take(&log.chat).map_or_else(Vec::new, |old| old.lines);
        self.chats.push(ChatLog { lines, ..log });
        if self.chats.len() > MAX_CHATS {
            self.chats.remove(0);
        }
        self.add_line(Speaker::User, id, text);
    }

    /// A reply to a chat that the history no longer holds is dropped.
    pub fn add_reply(&mut self, chat: &ChatId, speaker: Speaker, id: MessageId, text: &str) {
        let Some(log) = self.take(chat) else {
            return;
        };
        self.chats.push(log);
        self.add_line(speaker, id, text);
    }

    pub fn remove(&mut self, chat: &ChatId) {
        self.take(chat);
    }

    fn take(&mut self, chat: &ChatId) -> Option<ChatLog> {
        let at = self.chats.iter().position(|c| &c.chat == chat)?;
        Some(self.chats.remove(at))
    }

    fn add_line(&mut self, speaker: Speaker, id: MessageId, text: &str) {
        let Some(log) = self.chats.last_mut() else {
            return;
        };
        log.lines.push(Line {
            speaker,
            id,
            text: cut(text),
        });
        if log.lines.len() > MAX_HISTORY {
            log.lines.remove(0);
        }
    }

    pub fn to_restore(&self) -> Vec<Chat> {
        self.chats.iter().map(ChatLog::to_restore).collect()
    }
}

impl ChatLog {
    fn to_restore(&self) -> Chat {
        Chat {
            id: self.chat.0.as_bytes().to_vec(),
            name: self.name.as_bytes().to_vec(),
            agent: self.agent.as_bytes().to_vec(),
            cwd: self.cwd.as_bytes().to_vec(),
            history: self.lines.iter().map(Line::to_restore).collect(),
        }
    }
}

impl Line {
    fn to_restore(&self) -> Entry {
        Entry {
            role: match self.speaker {
                Speaker::User => Role::User,
                Speaker::Agent => Role::Agent,
                Speaker::Error => Role::Error,
            },
            id: self.id.0,
            text: self.text.as_bytes().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(chat: &str) -> ChatLog {
        ChatLog {
            chat: ChatId(chat.into()),
            name: "lighthouse".into(),
            agent: "claude".into(),
            cwd: "Code/x".into(),
            lines: Vec::new(),
        }
    }

    #[test]
    fn a_chat_keeps_its_last_messages_and_replies() {
        let mut history = History::default();
        for id in 0..8 {
            history.add_message(log("c1"), MessageId(id), "ask");
            history.add_reply(
                &ChatId("c1".into()),
                Speaker::Agent,
                MessageId(id),
                "answer",
            );
        }
        let chats = history.to_restore();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].history.len(), MAX_HISTORY);
        assert_eq!(chats[0].history[0].id, 3);
    }

    #[test]
    fn the_chat_with_the_oldest_activity_goes_first() {
        let mut history = History::default();
        for n in 0..=MAX_CHATS {
            history.add_message(log(&format!("c{n}")), MessageId(1), "ask");
        }
        history.add_reply(&ChatId("c1".into()), Speaker::Agent, MessageId(1), "late");
        history.add_reply(&ChatId("c0".into()), Speaker::Agent, MessageId(1), "gone");

        let ids: Vec<Vec<u8>> = history.to_restore().into_iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), MAX_CHATS);
        assert!(!ids.contains(&b"c0".to_vec()));
        assert_eq!(ids.last().unwrap(), b"c1");
    }

    #[test]
    fn a_long_text_is_cut_at_a_character_boundary() {
        let mut history = History::default();
        let text = format!("a{}", "é".repeat(400));
        history.add_message(log("c1"), MessageId(1), &text);
        let text = &history.to_restore()[0].history[0].text;
        assert_eq!(text.len(), MAX_ENTRY_TEXT - 1);
        assert!(std::str::from_utf8(text).is_ok());
    }
}
