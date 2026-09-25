//! What the agents do now: the last steps of each run, and the permission requests
//! that wait for the game (SPEC.md 9.3). It fills `Live.lua`. No I/O here.

use protocol::apps::App;
use protocol::live::{
    MAX_LINES, PermOption, Progress, Request, live_body, prepare_progress, prepare_requests,
};
use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::agent::Choice;
use crate::flags::PermAnswer;
use crate::relay::{ChatId, MessageId};

struct Steps {
    chat: ChatId,
    id: MessageId,
    lines: Vec<String>,
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
    pub fn step(&mut self, chat: &ChatId, id: MessageId, line: String) {
        let of_run = |s: &&mut Steps| &s.chat == chat && s.id == id;
        if !self.steps.iter_mut().any(|s| of_run(&s)) {
            self.steps.push(Steps {
                chat: chat.clone(),
                id,
                lines: Vec::new(),
            });
        }
        let Some(steps) = self.steps.iter_mut().find(of_run) else {
            return;
        };
        steps.lines.push(line);
        if steps.lines.len() > MAX_LINES {
            steps.lines.remove(0);
        }
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
                lines: s.lines.iter().map(|l| l.as_bytes().to_vec()).collect(),
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
    fn the_hash_is_the_first_eight_bytes_of_sha256() {
        assert_eq!(text_hash(b"abc"), "ba7816bf8f01cfea");
    }
}
