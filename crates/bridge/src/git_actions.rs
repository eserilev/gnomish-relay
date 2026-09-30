//! The git actions of the player on a chat (SPEC.md 6.6.6, 9.10). Each one is a message
//! of the chat, so it waits for a run of the chat to end.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::Control;
use crate::chat_branch::{self, ChatWorktree};
use crate::chat_merge;
use crate::desktop::Approvals;
use crate::gate;
use crate::git_host::GitHost;
use crate::run::{log, now};
use crate::turn::{Answer, Turn};

const NO_BRANCH: &str = "This chat has no branch of its own.";
const COPY_GONE: &str = "This chat's copy is gone. Send a message to make a new one.";
const NO_DESKTOP: &str =
    "Merge needs your approval on the desktop, and the desktop app can't ask here.";
const NOT_MERGED: &str = "Not merged.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitAction {
    Merge,
    Discard,
}

/// The value of `git=`: `merge` or `discard`.
pub fn git_action(value: &str) -> Option<GitAction> {
    match value {
        "merge" => Some(GitAction::Merge),
        "discard" => Some(GitAction::Discard),
        _ => None,
    }
}

/// How the bridge asks the desktop before a merge.
#[derive(Clone, Debug)]
pub struct MergeDesk {
    pub approvals: Approvals,
    pub wait: Duration,
}

/// What an action needs to know, from the state of the bridge.
pub struct Context<'a> {
    pub git: &'a GitHost,
    /// The own branch of the chat, if it has one.
    pub worktree: Option<&'a ChatWorktree>,
    pub desk: Option<&'a MergeDesk>,
    pub control: &'a Control,
}

/// What an action changed in the state of the bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Nothing,
    /// The own branch of the chat is gone.
    Discarded,
}

pub struct Done {
    pub reply: Result<String, String>,
    pub effect: Effect,
}

fn done(reply: Result<String, String>, effect: Effect) -> Done {
    let effect = if reply.is_ok() {
        effect
    } else {
        Effect::Nothing
    };
    Done { reply, effect }
}

pub fn perform(action: &GitAction, context: &Context) -> Done {
    match action {
        GitAction::Merge => done(merge(context), Effect::Nothing),
        GitAction::Discard => done(discard(context), Effect::Discarded),
    }
}

fn own_branch<'a>(context: &Context<'a>) -> Result<&'a ChatWorktree, String> {
    let worktree = context.worktree.ok_or_else(|| NO_BRANCH.to_owned())?;
    if !worktree.exists() {
        return Err(COPY_GONE.into());
    }
    Ok(worktree)
}

/// `~/Code/app` for a folder in the home folder, as the player reads it.
fn shown_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn merge(context: &Context) -> Result<String, String> {
    let worktree = own_branch(context)?;
    let ready = chat_merge::prepare(context.git, worktree)?;
    let desk = context.desk.ok_or_else(|| NO_DESKTOP.to_owned())?;
    let text = chat_merge::approval_text(worktree, &ready, &shown_path(&worktree.repo));
    let opened = desk
        .approvals
        .open_merge(&worktree.repo, &text, now())
        .map_err(|e| format!("Couldn't ask on the desktop: {e:#}"))?;
    log(&format!(
        "merge {}: asked as {}",
        worktree.branch, opened.id
    ));
    let mut turn = Turn::new(desk.wait, desk.wait, context.control.clone());
    let answer = gate::wait_on_the_desktop(&desk.approvals, &opened, None, &mut turn);
    if !matches!(answer, Answer::Desktop(true)) {
        return Err(NOT_MERGED.into());
    }
    chat_merge::apply(context.git, worktree, &ready)
}

fn discard(context: &Context) -> Result<String, String> {
    let worktree = context.worktree.ok_or_else(|| NO_BRANCH.to_owned())?;
    chat_branch::discard(context.git, worktree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_host::UserConfig;

    #[test]
    fn each_action_of_the_game_parses_and_nothing_else() {
        assert_eq!(git_action("merge"), Some(GitAction::Merge));
        assert_eq!(git_action("discard"), Some(GitAction::Discard));
        for bad in ["push", "merge:main", ""] {
            assert_eq!(git_action(bad), None, "{bad}");
        }
    }

    #[test]
    fn merge_and_discard_need_an_own_branch() {
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        let control = Control::default();
        let context = Context {
            git: &git,
            worktree: None,
            desk: None,
            control: &control,
        };

        assert_eq!(
            perform(&GitAction::Merge, &context).reply,
            Err(NO_BRANCH.into())
        );
        assert_eq!(
            perform(&GitAction::Discard, &context).reply,
            Err(NO_BRANCH.into())
        );
    }

    #[test]
    fn a_path_in_the_home_folder_shows_with_a_tilde() {
        let home = std::env::var("HOME").unwrap();

        assert_eq!(shown_path(&format!("{home}/Code/app")), "~/Code/app");
        assert_eq!(shown_path("/srv/app"), "/srv/app");
    }
}
