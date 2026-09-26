//! What the agents do now: the last steps of each run, and the permission requests
//! that wait for the game (SPEC.md 9.3). It fills `Live.lua`. No I/O here.

use protocol::apps::App;
use protocol::live::{
    MAX_LINES, PermOption, Progress, Request, live_body, prepare_progress, prepare_requests,
};
use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::agent::Choice;
use crate::config::Permission;
use crate::flags::PermAnswer;
use crate::relay::{ChatId, MessageId};

struct Steps {
    chat: ChatId,
    id: MessageId,
    /// The level line of the bridge. It stays first, so the addon always finds it.
    level: Option<String>,
    lines: Vec<String>,
}

/// Only the bridge writes a line with this start (SPEC.md 9.3).
const LEVEL: &str = "Level: ";

/// The level that a run really has. "(config)" marks a level that the config lowered.
pub fn level_line(level: Permission, asked: Permission) -> String {
    if level < asked {
        format!("{LEVEL}{} (config)", level.word())
    } else {
        format!("{LEVEL}{}", level.word())
    }
}

struct Asked {
    request: String,
    chat: ChatId,
    id: MessageId,
    text: Vec<u8>,
    choices: Vec<Choice>,
}

#[derive(Default)]
pub struct Activity {
    steps: Vec<Steps>,
    asked: Vec<Asked>,
    /// Checked answers that the bridge has not passed to their run yet.
    answers: Vec<(String, Option<usize>)>,
    asked_count: u64,
}

/// The first 8 bytes of SHA-256, in hex. The addon hashes the text that it showed,
/// so an answer counts only for the exact popup (SPEC.md 6.6.1).
pub fn text_hash(text: &[u8]) -> String {
    Sha256::digest(text)[..8]
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

impl Activity {
    /// The first line of a run: its level.
    pub fn begin(&mut self, chat: &ChatId, id: MessageId, level: String) {
        self.steps_of(chat, id).level = Some(level);
    }

    /// An agent line never looks like the level line of the bridge.
    pub fn step(&mut self, chat: &ChatId, id: MessageId, line: String) {
        let line = if line.starts_with(LEVEL.trim_end()) {
            format!("agent: {line}")
        } else {
            line
        };
        let steps = self.steps_of(chat, id);
        steps.lines.push(line);
        let room = MAX_LINES - usize::from(steps.level.is_some());
        if steps.lines.len() > room {
            steps.lines.remove(0);
        }
    }

    fn steps_of(&mut self, chat: &ChatId, id: MessageId) -> &mut Steps {
        let at = self
            .steps
            .iter()
            .position(|s| &s.chat == chat && s.id == id);
        let at = at.unwrap_or_else(|| {
            self.steps.push(Steps {
                chat: chat.clone(),
                id,
                level: None,
                lines: Vec::new(),
            });
            self.steps.len() - 1
        });
        &mut self.steps[at]
    }

    /// Returns the id of the new request. The time in the id keeps an old strip from
    /// answering a new request after a restart of the bridge.
    pub fn ask(
        &mut self,
        chat: &ChatId,
        id: MessageId,
        text: Vec<u8>,
        choices: Vec<Choice>,
        now: u32,
    ) -> String {
        self.asked_count += 1;
        let request = format!("p{now:x}{}", self.asked_count);
        self.asked.push(Asked {
            request: request.clone(),
            chat: chat.clone(),
            id,
            text,
            choices,
        });
        request
    }

    /// Takes an answer from the game. It counts only for an open request of the same
    /// chat, a real option, and the hash of the popup text. Returns whether it counted.
    pub fn answer(&mut self, chat: &ChatId, answer: &PermAnswer) -> bool {
        let Some(at) = self.asked.iter().position(|a| a.request == answer.request) else {
            return false;
        };
        let asked = &self.asked[at];
        let fits = &asked.chat == chat
            && answer.option < asked.choices.len()
            && text_hash(&asked.text) == answer.hash;
        if fits {
            let asked = self.asked.remove(at);
            self.answers.push((asked.request, Some(answer.option)));
        }
        fits
    }

    pub fn take_answers(&mut self) -> Vec<(String, Option<usize>)> {
        std::mem::take(&mut self.answers)
    }

    pub fn is_open(&self, request: &str) -> bool {
        self.asked.iter().any(|a| a.request == request)
    }

    /// The run of a message ended: its steps and its open requests go.
    pub fn end(&mut self, chat: &ChatId, id: MessageId) {
        self.steps.retain(|s| !(&s.chat == chat && s.id == id));
        self.asked.retain(|a| !(&a.chat == chat && a.id == id));
    }

    pub fn file(&self) -> Vec<u8> {
        let progress: Vec<Progress> = self
            .steps
            .iter()
            .map(|s| Progress {
                chat: s.chat.0.as_bytes().to_vec(),
                id: s.id.0,
                lines: s
                    .level
                    .iter()
                    .chain(&s.lines)
                    .map(|l| l.as_bytes().to_vec())
                    .collect(),
            })
            .collect();
        let requests: Vec<Request> = self.asked.iter().map(Asked::to_request).collect();
        live_body(
            App::Relay,
            &prepare_progress(&progress),
            &prepare_requests(&requests),
        )
    }
}

impl Asked {
    fn to_request(&self) -> Request {
        Request {
            request: self.request.as_bytes().to_vec(),
            chat: self.chat.0.as_bytes().to_vec(),
            id: self.id.0,
            text: self.text.clone(),
            options: self
                .choices
                .iter()
                .enumerate()
                .map(|(i, c)| PermOption {
                    id: format!("o{}", i + 1).into_bytes(),
                    kind: c.kind,
                    label: c.label.as_bytes().to_vec(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::live::OptionKind;

    fn chat() -> ChatId {
        ChatId("c1".into())
    }

    fn choices() -> Vec<Choice> {
        vec![
            Choice {
                kind: OptionKind::AllowOnce,
                label: "Allow".into(),
            },
            Choice {
                kind: OptionKind::RejectOnce,
                label: "Reject".into(),
            },
        ]
    }

    fn answer(request: &str, option: usize, text: &[u8]) -> PermAnswer {
        PermAnswer {
            request: request.into(),
            option,
            hash: text_hash(text),
        }
    }

    #[test]
    fn a_question_shows_in_the_file_with_numbered_options() {
        let mut activity = Activity::default();
        let request = activity.ask(
            &chat(),
            MessageId(7),
            b"rm -rf build".to_vec(),
            choices(),
            0x1234,
        );
        assert_eq!(request, "p12341");
        let file = String::from_utf8(activity.file()).unwrap();
        assert!(
            file.contains(r#"{request = "p12341", chat = "c1", id = 7, text = "rm -rf build""#)
        );
        assert!(file.contains(r#"{id = "o2", kind = "reject_once", label = "Reject"}"#));
    }

    #[test]
    fn a_right_answer_counts_once() {
        let mut activity = Activity::default();
        let request = activity.ask(&chat(), MessageId(7), b"cargo test".to_vec(), choices(), 1);
        assert!(activity.answer(&chat(), &answer(&request, 1, b"cargo test")));
        assert_eq!(activity.take_answers(), [(request.clone(), Some(1))]);
        assert!(!activity.answer(&chat(), &answer(&request, 1, b"cargo test")));
        assert!(!activity.is_open(&request));
    }

    #[test]
    fn a_wrong_hash_option_or_chat_does_not_count() {
        let mut activity = Activity::default();
        let request = activity.ask(&chat(), MessageId(7), b"cargo test".to_vec(), choices(), 1);
        assert!(!activity.answer(&chat(), &answer(&request, 0, b"rm -rf ~")));
        assert!(!activity.answer(&chat(), &answer(&request, 2, b"cargo test")));
        assert!(!activity.answer(&ChatId("c2".into()), &answer(&request, 0, b"cargo test")));
        assert!(activity.take_answers().is_empty());
        assert!(activity.is_open(&request));
    }

    #[test]
    fn a_run_keeps_its_last_steps_until_it_ends() {
        let mut activity = Activity::default();
        for n in 0..8 {
            activity.step(&chat(), MessageId(7), format!("step {n}"));
        }
        let file = String::from_utf8(activity.file()).unwrap();
        assert!(
            file.contains(r#"lines = {"step 3", "step 4", "step 5", "step 6", "step 7", }"#),
            "{file}"
        );
        activity.end(&chat(), MessageId(7));
        assert!(!String::from_utf8(activity.file()).unwrap().contains("step"));
    }

    #[test]
    fn the_level_line_stays_first_when_the_agent_sends_many_steps() {
        let mut activity = Activity::default();
        let level = level_line(Permission::Ask, Permission::AutoEdit);
        activity.begin(&chat(), MessageId(7), level);
        for n in 0..8 {
            activity.step(&chat(), MessageId(7), format!("step {n}"));
        }
        let file = String::from_utf8(activity.file()).unwrap();
        assert!(
            file.contains(
                r#"lines = {"Level: ask (config)", "step 4", "step 5", "step 6", "step 7", }"#
            ),
            "{file}"
        );
    }

    #[test]
    fn an_agent_line_that_starts_with_level_gets_a_prefix() {
        let mut activity = Activity::default();
        activity.step(&chat(), MessageId(7), "Level: full-auto".into());
        activity.step(&chat(), MessageId(7), "Level:full-auto".into());
        let file = String::from_utf8(activity.file()).unwrap();
        assert!(
            file.contains(r#"lines = {"agent: Level: full-auto", "agent: Level:full-auto", }"#),
            "{file}"
        );
    }

    #[test]
    fn the_level_line_says_config_only_when_the_config_lowered_the_level() {
        use Permission::{Ask, AutoEdit, FullAuto};
        assert_eq!(level_line(Ask, AutoEdit), "Level: ask (config)");
        assert_eq!(level_line(AutoEdit, AutoEdit), "Level: auto-edit");
        assert_eq!(level_line(AutoEdit, FullAuto), "Level: auto-edit (config)");
        assert_eq!(level_line(FullAuto, FullAuto), "Level: full-auto");
        assert_eq!(level_line(Ask, Ask), "Level: ask");
    }

    #[test]
    fn the_hash_is_the_first_eight_bytes_of_sha256() {
        assert_eq!(text_hash(b"abc"), "ba7816bf8f01cfea");
    }
}
