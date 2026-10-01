//! Full-auto for one chat (SPEC.md 9.3, "Full-auto for one chat"). The first message of
//! a chat at full-auto waits for one desktop Approve. The approval of each chat and its
//! real folder stays in `state.json`, so later messages ask nothing.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::Control;
use crate::config::Permission;
use crate::desktop::{Approvals, Topic};
use crate::gate;
use crate::lane::ChatId;
use crate::raise::Raised;
use crate::relay::Job;
use crate::run::{log, now};
use crate::turn::{Answer, Turn};

/// A new approval pushes out the oldest one.
const MAX_CHATS: usize = 64;
/// The name comes from the game, so the dialog shows only a short, plain part of it.
const MAX_NAME: usize = 40;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub chat: ChatId,
    /// The real folder of the run that the user approved.
    pub folder: String,
}

/// The chats that the user approved on the desktop, oldest first.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct FullAutoChats {
    approvals: Vec<Approval>,
}

impl FullAutoChats {
    pub fn is_empty(&self) -> bool {
        self.approvals.is_empty()
    }

    /// Another folder needs a new Approve: the player picked it, or a link moved.
    pub fn holds(&self, chat: &ChatId, folder: &str) -> bool {
        self.approvals
            .iter()
            .any(|a| &a.chat == chat && a.folder == folder)
    }

    pub fn approve(&mut self, chat: &ChatId, folder: &str) {
        self.forget(chat);
        self.approvals.push(Approval {
            chat: chat.clone(),
            folder: folder.to_owned(),
        });
        if self.approvals.len() > MAX_CHATS {
            self.approvals.remove(0);
        }
    }

    pub fn forget(&mut self, chat: &ChatId) {
        self.approvals.retain(|a| &a.chat != chat);
    }
}

/// At most `MAX_NAME` characters, with each control character as a space, so the name
/// cannot add lines that look like the text of the bridge.
fn plain_name(name: &str) -> String {
    name.chars()
        .take(MAX_NAME)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Fixed text, the agent from the config, the chat name, and the real folder.
pub fn dialog_text(agent: &str, name: &str, folder: &str) -> String {
    let name = plain_name(name);
    format!(
        "Let {agent} run anything with no question in the chat \"{name}\" ({folder})? It \
         stays in the sandbox, but it can push, publish, and change files in this folder \
         that git and other tools run later. Approve only if you just picked full-auto in \
         WoW."
    )
}

/// The key of a chat in the guard of the raises. An agent name has no space.
pub fn guard_key(chat: &ChatId) -> String {
    format!("chat {chat}")
}

/// What a full-auto request needs. With none, no chat runs at full-auto.
#[derive(Clone, Debug)]
pub struct FullAutoAsker {
    pub approvals: Approvals,
    pub permission_timeout: Duration,
    /// The agents whose every command runs in the command sandbox: kind `claude` on a
    /// computer with `bwrap` or `sandbox-exec`.
    pub agents: Vec<String>,
}

impl FullAutoAsker {
    /// Full-auto promises "It stays in the sandbox", so only these agents get it.
    pub fn walls_hold(&self, agent: &str) -> bool {
        self.agents.iter().any(|a| a == agent)
    }

    /// Runs in the thread of the run, before the agent starts. The game shows a notice
    /// with no buttons.
    pub fn ask(&self, job: &Job, name: &str, control: &Control) -> Raised {
        let text = dialog_text(&job.agent, name, &job.cwd);
        let opened = self
            .approvals
            .open_full_auto(&job.agent, &job.cwd, &text, now());
        let Ok(opened) = opened else {
            log(&format!("full-auto {}: no desktop request", job.chat));
            return Raised::NotRaised;
        };
        log(&format!("full-auto {}: asked as {}", job.chat, opened.id));
        let mut turn = Turn::new(
            self.permission_timeout,
            self.permission_timeout,
            control.clone(),
        );
        let topic = Topic::Raise(Permission::FullAuto);
        let answer = gate::wait_on_the_desktop(&self.approvals, &opened, topic, &mut turn);
        let raised = outcome(&answer);
        log(&format!("full-auto {}: {raised:?}", job.chat));
        raised
    }
}

fn outcome(answer: &Answer) -> Raised {
    match answer {
        Answer::Desktop(true) => Raised::Approved,
        Answer::Desktop(false) => Raised::DeniedOnTheDesktop,
        Answer::Game(_) | Answer::None | Answer::NewMessage | Answer::Covered => Raised::NotRaised,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{Prompt, Verdict};
    use crate::relay::{MessageId, Session, Work};

    #[test]
    fn an_approval_holds_only_for_its_chat_and_its_folder() {
        let mut chats = FullAutoChats::default();
        let c1 = ChatId::new("c1");

        chats.approve(&c1, "/home/x/Code/app");

        assert!(chats.holds(&c1, "/home/x/Code/app"));
        assert!(!chats.holds(&c1, "/home/x/Code/other"));
        assert!(!chats.holds(&ChatId::new("c2"), "/home/x/Code/app"));
    }

    #[test]
    fn a_new_folder_replaces_the_old_approval_of_the_chat() {
        let mut chats = FullAutoChats::default();
        let c1 = ChatId::new("c1");

        chats.approve(&c1, "/a");
        chats.approve(&c1, "/b");

        assert!(!chats.holds(&c1, "/a"));
        assert!(chats.holds(&c1, "/b"));
    }

    #[test]
    fn forget_removes_the_approval_of_the_chat() {
        let mut chats = FullAutoChats::default();
        let c1 = ChatId::new("c1");
        chats.approve(&c1, "/a");

        chats.forget(&c1);

        assert!(chats.is_empty());
    }

    #[test]
    fn the_store_keeps_the_newest_64_approvals() {
        let mut chats = FullAutoChats::default();

        for i in 0..70 {
            chats.approve(&ChatId::new(format!("c{i}")), "/a");
        }

        assert!(!chats.holds(&ChatId::new("c5"), "/a"));
        assert!(chats.holds(&ChatId::new("c6"), "/a"));
        assert!(chats.holds(&ChatId::new("c69"), "/a"));
    }

    #[test]
    fn the_store_reads_back_from_json() {
        let mut chats = FullAutoChats::default();
        chats.approve(&ChatId::new("c1"), "/home/x/app");

        let json = serde_json::to_string(&chats).unwrap();
        let back: FullAutoChats = serde_json::from_str(&json).unwrap();

        assert_eq!(json, r#"[{"chat":"c1","folder":"/home/x/app"}]"#);
        assert_eq!(back, chats);
    }

    #[test]
    fn the_dialog_names_the_agent_the_chat_and_the_folder_and_says_what_it_allows() {
        let text = dialog_text("claude", "Fix tests", "/home/x/Code/app");

        assert!(
            text.starts_with(
                "Let claude run anything with no question in the chat \"Fix tests\" \
                 (/home/x/Code/app)? It stays in the sandbox"
            ),
            "{text}"
        );
        assert!(text.contains("push, publish"), "{text}");
    }

    #[test]
    fn the_dialog_cuts_the_chat_name_and_turns_control_characters_into_spaces() {
        let long = "x".repeat(100);

        let cut = dialog_text("claude", &long, "/a");
        let plain = dialog_text("claude", "a\nApprove: yes", "/a");

        assert!(cut.contains(&format!("\"{}\"", "x".repeat(40))), "{cut}");
        assert!(!cut.contains(&"x".repeat(41)), "{cut}");
        assert!(plain.contains("\"a Approve: yes\""), "{plain}");
    }

    #[test]
    fn only_an_agent_whose_walls_hold_gets_full_auto() {
        let data = tempfile::tempdir().unwrap();
        let asker = FullAutoAsker {
            approvals: Approvals::new(data.path(), Prompt::Off),
            permission_timeout: Duration::from_secs(1),
            agents: vec!["claude".into()],
        };

        assert!(asker.walls_hold("claude"));
        assert!(!asker.walls_hold("codex"));
    }

    fn job(cwd: &str) -> Job {
        Job {
            token: "tok".into(),
            chat: ChatId::new("c1"),
            id: MessageId(1),
            agent: "claude".into(),
            permission: Permission::AutoEdit,
            asked: Permission::FullAuto,
            cwd: cwd.into(),
            session: Session::New,
            resume: None,
            text: "hi".into(),
            work: Work::Prompt,
            new_folder: false,
        }
    }

    #[test]
    fn an_approve_on_the_desktop_approves_and_a_deny_does_not() {
        for (verdict, raised) in [
            (Verdict::Approve, Raised::Approved),
            (Verdict::Deny, Raised::DeniedOnTheDesktop),
        ] {
            let data = tempfile::tempdir().unwrap();
            let approvals = Approvals::new(data.path(), Prompt::Off);
            let asker = FullAutoAsker {
                approvals: approvals.clone(),
                permission_timeout: Duration::from_secs(10),
                agents: vec!["claude".into()],
            };
            let answering = std::thread::spawn(move || {
                for _ in 0..250 {
                    if let Some(open) = approvals.list().pop() {
                        approvals.answer(&open.id, verdict).unwrap();
                        return open.text;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                String::new()
            });

            let answer = asker.ask(&job("/home/x/app"), "Chat 1", &Control::default());

            assert_eq!(answer, raised);
            assert!(
                answering
                    .join()
                    .unwrap()
                    .contains("\"Chat 1\" (/home/x/app)")
            );
        }
    }

    #[test]
    fn no_answer_does_not_approve() {
        let data = tempfile::tempdir().unwrap();
        let asker = FullAutoAsker {
            approvals: Approvals::new(data.path(), Prompt::Off),
            permission_timeout: Duration::from_millis(200),
            agents: vec!["claude".into()],
        };

        let answer = asker.ask(&job("/a"), "Chat 1", &Control::default());

        assert_eq!(answer, Raised::NotRaised);
        assert!(asker.approvals.list().is_empty(), "the request closed");
    }
}
