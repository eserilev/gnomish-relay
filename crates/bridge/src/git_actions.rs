//! The git actions of the player on a chat (SPEC.md 6.6.6, 9.11). Each one is a message
//! of the chat, so it waits for a run of the chat to end.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::Control;
use crate::chat_branch::{self, ChatWorktree};
use crate::chat_merge;
use crate::ci_checks::{self, CiChecks};
use crate::desktop::Approvals;
use crate::folder_path::path_bytes;
use crate::gate;
use crate::git_blocks::{ci_block, with_blocks};
use crate::git_host::GitHost;
use crate::lane::MessageId;
use crate::run::{home_folder, log, now};
use crate::run_actions;
use crate::run_changes::RunChanges;
use crate::settings_list::shown;
use crate::turn::{Answer, Turn};

const NO_BRANCH: &str = "This chat has no branch of its own.";
const COPY_GONE: &str = "This chat's copy is gone. Send a message to make a new one.";
const TOO_OLD: &str = "This change summary is too old. Nothing changed.";
const NO_DESKTOP: &str =
    "Merge needs your approval on the desktop, and the desktop app can't ask here.";
const NOT_MERGED: &str = "Not merged.";
const NOT_ON_A_BRANCH: &str = "This folder isn't on a branch.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitAction {
    /// The run of this message, with the text of the message as the commit message.
    Commit(MessageId),
    Revert(MessageId),
    Merge,
    Discard,
    Checks,
}

/// The value of `git=`: `commit:<id>`, `revert:<id>`, `merge`, `discard`, or `checks`.
pub fn git_action(value: &str) -> Option<GitAction> {
    let run = |id: &str| id.parse().ok().map(MessageId);
    match value.split_once(':') {
        Some(("commit", id)) => run(id).map(GitAction::Commit),
        Some(("revert", id)) => run(id).map(GitAction::Revert),
        None if value == "merge" => Some(GitAction::Merge),
        None if value == "discard" => Some(GitAction::Discard),
        None if value == "checks" => Some(GitAction::Checks),
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
    /// The record of the run that Commit or Revert names, if the bridge still has it.
    pub run: Option<&'a RunChanges>,
    /// The chat folder, for the checks of a chat with no own branch.
    pub folder: &'a Path,
    pub ci: &'a CiChecks,
    pub desk: Option<&'a MergeDesk>,
    pub control: &'a Control,
}

/// What an action changed in the state of the bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Nothing,
    Committed(MessageId),
    Reverted(MessageId),
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

pub fn perform(action: &GitAction, text: &str, context: &Context) -> Done {
    if let Err(e) = check_copy(context) {
        return done(Err(e), Effect::Nothing);
    }
    match action {
        GitAction::Commit(id) => {
            let reply = context
                .run
                .ok_or_else(|| TOO_OLD.to_owned())
                .and_then(|run| run_actions::commit(context.git, run, text));
            done(reply, Effect::Committed(*id))
        }
        GitAction::Revert(id) => {
            let reply = context
                .run
                .ok_or_else(|| TOO_OLD.to_owned())
                .and_then(|run| run_actions::revert(context.git, run));
            done(reply, Effect::Reverted(*id))
        }
        GitAction::Merge => done(merge(context), Effect::Nothing),
        GitAction::Discard => done(discard(context), Effect::Discarded),
        GitAction::Checks => done(checks(context), Effect::Nothing),
    }
}

fn check_copy(context: &Context) -> Result<(), String> {
    match context.worktree {
        Some(worktree) => chat_branch::check_link(context.git, worktree),
        None => Ok(()),
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
    let home = home_folder().map(|h| path_bytes(&h));
    shown(&path_bytes(Path::new(path)), home.as_deref())
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

fn checks(context: &Context) -> Result<String, String> {
    let CiChecks::On { program } = context.ci else {
        return Err(ci_checks::OFF.into());
    };
    let (folder, branch) = match context.worktree {
        Some(worktree) => (Path::new(&worktree.folder), Some(worktree.branch.clone())),
        None => (
            context.folder,
            chat_branch::current_branch(context.git, context.folder),
        ),
    };
    let branch = branch.ok_or_else(|| NOT_ON_A_BRANCH.to_owned())?;
    match ci_checks::checks(program, folder, &branch) {
        Ok(Some(counts)) => Ok(with_blocks("", &ci_block(&counts))),
        Ok(None) => Ok(ci_checks::no_pull_request(&branch)),
        Err(e) => Err(e.text()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_host::UserConfig;

    #[test]
    fn each_action_of_the_game_parses_and_nothing_else() {
        assert_eq!(
            git_action("commit:12"),
            Some(GitAction::Commit(MessageId(12)))
        );
        assert_eq!(
            git_action("revert:3"),
            Some(GitAction::Revert(MessageId(3)))
        );
        assert_eq!(git_action("merge"), Some(GitAction::Merge));
        assert_eq!(git_action("discard"), Some(GitAction::Discard));
        assert_eq!(git_action("checks"), Some(GitAction::Checks));
        for bad in [
            "commit",
            "commit:",
            "commit:-1",
            "revert:x",
            "push",
            "merge:main",
            "",
        ] {
            assert_eq!(git_action(bad), None, "{bad}");
        }
    }

    fn context<'a>(
        git: &'a GitHost,
        folder: &'a Path,
        ci: &'a CiChecks,
        control: &'a Control,
    ) -> Context<'a> {
        Context {
            git,
            worktree: None,
            run: None,
            folder,
            ci,
            desk: None,
            control,
        }
    }

    #[test]
    fn an_action_on_a_summary_that_the_bridge_forgot_changes_nothing() {
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        let (ci, control) = (CiChecks::Off, Control::default());
        let dir = tempfile::tempdir().unwrap();

        let done = perform(
            &GitAction::Revert(MessageId(4)),
            "",
            &context(&git, dir.path(), &ci, &control),
        );

        assert_eq!(done.reply, Err(TOO_OLD.into()));
        assert_eq!(done.effect, Effect::Nothing);
    }

    #[test]
    fn merge_and_discard_need_an_own_branch() {
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        let (ci, control) = (CiChecks::Off, Control::default());
        let dir = tempfile::tempdir().unwrap();
        let context = context(&git, dir.path(), &ci, &control);

        assert_eq!(
            perform(&GitAction::Merge, "", &context).reply,
            Err(NO_BRANCH.into())
        );
        assert_eq!(
            perform(&GitAction::Discard, "", &context).reply,
            Err(NO_BRANCH.into())
        );
    }

    #[test]
    fn checks_that_are_off_run_nothing_and_say_how_to_turn_them_on() {
        let git = GitHost::with_config(UserConfig::Skip).unwrap();
        let (ci, control) = (CiChecks::Off, Control::default());
        let dir = tempfile::tempdir().unwrap();

        let done = perform(
            &GitAction::Checks,
            "",
            &context(&git, dir.path(), &ci, &control),
        );

        assert_eq!(done.reply, Err(ci_checks::OFF.into()));
    }

    #[test]
    fn a_path_in_the_home_folder_shows_with_a_tilde() {
        let app = home_folder().unwrap().join("Code").join("app");

        assert_eq!(shown_path(&app.to_string_lossy()), "~/Code/app");
        assert_eq!(shown_path("/srv/app"), "/srv/app");
    }
}
