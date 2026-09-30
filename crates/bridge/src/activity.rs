//! What the agents do now: the last steps of each run, and the permission requests
//! that wait for the game (SPEC.md 9.3). It fills `Live.lua`. No I/O here.

use protocol::apps::App;
use protocol::live::{
    MAX_LINES, Notices, OptionKind, PermOption, Progress, Request, live_body, prepare_progress,
    prepare_requests,
};

use sha2::{Digest, Sha256};

use crate::agent::Choice;
use crate::config::Permission;
use crate::desktop::{NOTICE, Notice, Waiting};
use crate::flags::PermAnswer;
use crate::ids::hex;
use crate::relay::{ChatId, MessageId};

struct Steps {
    chat: ChatId,
    id: MessageId,
    /// The level line of the bridge. It stays first, so the addon always finds it.
    level: Option<String>,
    /// The last desktop request of the run. Its line comes right after the level line.
    desktop: Option<Notice>,
    lines: Vec<String>,
}

impl Steps {
    fn room(&self) -> usize {
        MAX_LINES - usize::from(self.level.is_some()) - usize::from(self.desktop.is_some())
    }

    fn trim(&mut self) {
        let extra = self.lines.len().saturating_sub(self.room());
        self.lines.drain(..extra);
    }
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

/// Only the bridge writes a line with this start (SPEC.md 8.2).
const WAITING: &str = "Waiting: ";

/// The line of a message that waits for the limit on parallel runs.
pub fn waiting_line(running: usize, ahead: usize) -> String {
    let chats = if running == 1 {
        "1 other chat is running".to_owned()
    } else {
        format!("{running} other chats are running")
    };
    if ahead == 0 {
        return format!("{WAITING}{chats}");
    }
    format!("{WAITING}{chats}, {ahead} ahead of this one")
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
    hex(&Sha256::digest(text)[..8])
}

impl Activity {
    /// The first line of a run: its level.
    pub fn begin(&mut self, chat: &ChatId, id: MessageId, level: String) {
        self.steps_of(chat, id).level = Some(level);
    }

    /// An agent line never looks like a line of the bridge.
    pub fn step(&mut self, chat: &ChatId, id: MessageId, line: String) {
        let bridge_like = [LEVEL, NOTICE, WAITING]
            .iter()
            .any(|p| line.starts_with(p.trim_end()));
        let line = if bridge_like {
            format!("agent: {line}")
        } else {
            line
        };
        let steps = self.steps_of(chat, id);
        steps.lines.push(line);
        steps.trim();
    }

    /// A message that waits for a free run has this one line. Returns true when the
    /// line changed, so the bridge publishes only then.
    pub fn wait(&mut self, chat: &ChatId, id: MessageId, line: String) -> bool {
        let steps = self.steps_of(chat, id);
        if steps.lines == [line.as_str()] {
            return false;
        }
        steps.lines = vec![line];
        true
    }

    /// The desktop request of a run opened or ended (SPEC.md 6.6.3).
    pub fn desktop(&mut self, chat: &ChatId, id: MessageId, notice: Notice) {
        let steps = self.steps_of(chat, id);
        steps.desktop = Some(notice);
        steps.trim();
    }

    /// True while a run of `chat` waits for an answer in the game or on the desktop.
    pub fn waits(&self, chat: &ChatId) -> bool {
        let desktop = self.steps.iter().any(|s| {
            &s.chat == chat
                && s.desktop
                    .as_ref()
                    .is_some_and(|d| d.waiting == Waiting::Open)
        });
        desktop || self.asked.iter().any(|a| &a.chat == chat)
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
                desktop: None,
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
            && asked.hash_of(answer.option) == answer.hash;
        if fits {
            let asked = self.asked.remove(at);
            self.answers.push((asked.request, Some(answer.option)));
        }
        fits
    }

    /// The open question of a run needs no answer any more. A run waits for one
    /// question at a time.
    pub fn withdraw(&mut self, chat: &ChatId, id: MessageId) {
        self.asked.retain(|a| !(&a.chat == chat && a.id == id));
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

    pub fn file(&self, notices: &Notices) -> Vec<u8> {
        let progress: Vec<Progress> = self
            .steps
            .iter()
            .map(|s| Progress {
                chat: s.chat.as_bytes().to_vec(),
                id: s.id.0,
                lines: s
                    .level
                    .iter()
                    .cloned()
                    .chain(s.desktop.as_ref().map(Notice::line))
                    .chain(s.lines.iter().cloned())
                    .map(String::into_bytes)
                    .collect(),
            })
            .collect();
        let requests: Vec<Request> = self.asked.iter().map(Asked::to_request).collect();
        live_body(
            App::Relay,
            &prepare_progress(&progress),
            &prepare_requests(&requests),
            notices,
        )
    }
}

impl Asked {
    /// The hash of what the popup showed for this choice. For "Always allow" that is
    /// also its rule line, so the hash binds the rule that the user saw (SPEC.md 6.6.5).
    fn hash_of(&self, option: usize) -> String {
        let choice = &self.choices[option];
        if !matches!(choice.kind, OptionKind::AllowAlways) {
            return text_hash(&self.text);
        }
        let mut shown = self.text.clone();
        shown.push(b'\n');
        shown.extend_from_slice(choice.label.as_bytes());
        text_hash(&shown)
    }

    fn to_request(&self) -> Request {
        Request {
            request: self.request.as_bytes().to_vec(),
            chat: self.chat.as_bytes().to_vec(),
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
    use protocol::live::{OptionKind, no_notices};

    fn chat() -> ChatId {
        ChatId::new("c1")
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
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
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
        assert!(!activity.answer(&ChatId::new("c2"), &answer(&request, 0, b"cargo test")));
        assert!(activity.take_answers().is_empty());
        assert!(activity.is_open(&request));
    }

    fn with_always() -> Vec<Choice> {
        vec![
            Choice {
                kind: OptionKind::AllowOnce,
                label: "Allow".into(),
            },
            Choice {
                kind: OptionKind::AllowAlways,
                label: "make * in Code/app".into(),
            },
        ]
    }

    #[test]
    fn an_always_answer_counts_only_with_the_hash_of_the_text_and_the_rule_line() {
        let mut activity = Activity::default();
        let request = activity.ask(&chat(), MessageId(7), b"make".to_vec(), with_always(), 1);
        assert!(!activity.answer(&chat(), &answer(&request, 1, b"make")));
        assert!(!activity.answer(&chat(), &answer(&request, 1, b"make\nmake * in Code")));
        assert!(activity.answer(&chat(), &answer(&request, 1, b"make\nmake * in Code/app")));
        assert_eq!(activity.take_answers(), [(request, Some(1))]);
    }

    #[test]
    fn allow_once_next_to_always_keeps_the_hash_of_the_text() {
        let mut activity = Activity::default();
        let request = activity.ask(&chat(), MessageId(7), b"make".to_vec(), with_always(), 1);
        assert!(activity.answer(&chat(), &answer(&request, 0, b"make")));
    }

    #[test]
    fn a_withdrawn_question_leaves_the_file_and_takes_no_answer() {
        let mut activity = Activity::default();
        let request = activity.ask(&chat(), MessageId(7), b"make".to_vec(), choices(), 1);
        activity.withdraw(&chat(), MessageId(7));
        assert!(!activity.is_open(&request));
        assert!(!activity.answer(&chat(), &answer(&request, 0, b"make")));
    }

    #[test]
    fn a_run_keeps_its_last_steps_until_it_ends() {
        let mut activity = Activity::default();
        for n in 0..8 {
            activity.step(&chat(), MessageId(7), format!("step {n}"));
        }
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(r#"lines = {"step 3", "step 4", "step 5", "step 6", "step 7", }"#),
            "{file}"
        );
        activity.end(&chat(), MessageId(7));
        assert!(
            !String::from_utf8(activity.file(&no_notices()))
                .unwrap()
                .contains("step")
        );
    }

    #[test]
    fn the_level_line_stays_first_when_the_agent_sends_many_steps() {
        let mut activity = Activity::default();
        let level = level_line(Permission::Ask, Permission::AutoEdit);
        activity.begin(&chat(), MessageId(7), level);
        for n in 0..8 {
            activity.step(&chat(), MessageId(7), format!("step {n}"));
        }
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
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
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
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

    fn notice(waiting: Waiting) -> Notice {
        Notice {
            id: "a1b2c3d4e5f6".into(),
            prompted: crate::desktop::Prompted::Dialog,
            waiting,
            topic: crate::desktop::Topic::Action,
        }
    }

    #[test]
    fn the_desktop_line_comes_right_after_the_level_line_and_changes_on_each_answer() {
        let mut activity = Activity::default();
        activity.begin(&chat(), MessageId(7), "Level: ask".into());
        for n in 0..8 {
            activity.step(&chat(), MessageId(7), format!("step {n}"));
        }
        activity.desktop(&chat(), MessageId(7), notice(Waiting::Open));
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(
                r#"lines = {"Level: ask", "Desktop: wait a1b2c3d4e5f6 dialog", "step 5", "step 6", "step 7", }"#
            ),
            "{file}"
        );
        assert!(activity.waits(&chat()));
        activity.desktop(&chat(), MessageId(7), notice(Waiting::Denied));
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(r#""Desktop: denied a1b2c3d4e5f6 dialog""#),
            "{file}"
        );
        assert!(!activity.waits(&chat()));
    }

    #[test]
    fn an_agent_line_that_starts_with_desktop_gets_a_prefix() {
        let mut activity = Activity::default();
        activity.step(
            &chat(),
            MessageId(7),
            "Desktop: approved a1b2c3d4e5f6 dialog".into(),
        );
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(r#"lines = {"agent: Desktop: approved a1b2c3d4e5f6 dialog", }"#),
            "{file}"
        );
    }

    #[test]
    fn the_waiting_line_names_the_running_chats_and_the_messages_ahead() {
        assert_eq!(waiting_line(1, 0), "Waiting: 1 other chat is running");
        assert_eq!(waiting_line(3, 0), "Waiting: 3 other chats are running");
        assert_eq!(
            waiting_line(3, 2),
            "Waiting: 3 other chats are running, 2 ahead of this one"
        );
    }

    #[test]
    fn a_waiting_line_is_the_only_line_of_its_message_and_reports_a_change() {
        let mut activity = Activity::default();

        let first = activity.wait(&chat(), MessageId(7), waiting_line(3, 0));
        let again = activity.wait(&chat(), MessageId(7), waiting_line(3, 0));
        let changed = activity.wait(&chat(), MessageId(7), waiting_line(3, 1));

        assert_eq!((first, again, changed), (true, false, true));
        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(
                r#"lines = {"Waiting: 3 other chats are running, 1 ahead of this one", }"#
            ),
            "{file}"
        );
    }

    #[test]
    fn an_agent_line_that_starts_with_waiting_gets_a_prefix() {
        let mut activity = Activity::default();

        activity.step(&chat(), MessageId(7), "Waiting: 9 other chats".into());

        let file = String::from_utf8(activity.file(&no_notices())).unwrap();
        assert!(
            file.contains(r#"lines = {"agent: Waiting: 9 other chats", }"#),
            "{file}"
        );
    }

    #[test]
    fn a_question_in_the_game_counts_as_a_wait_until_it_is_answered() {
        let mut activity = Activity::default();
        assert!(!activity.waits(&chat()));
        let request = activity.ask(&chat(), MessageId(7), b"make".to_vec(), choices(), 1);
        assert!(activity.waits(&chat()));
        assert!(!activity.waits(&ChatId::new("c2")));
        activity.answer(&chat(), &answer(&request, 0, b"make"));
        assert!(!activity.waits(&chat()));
    }

    #[test]
    fn the_hash_is_the_first_eight_bytes_of_sha256() {
        assert_eq!(text_hash(b"abc"), "ba7816bf8f01cfea");
    }
}
