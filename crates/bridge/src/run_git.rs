//! Git around one run, in the thread of the run (SPEC.md 9.10): the own branch before
//! the agent starts, and the branch after it.

use std::path::Path;
use std::sync::Arc;

use crate::chat_branch::{self, ChatWorktree};
use crate::folder_walk::Walk;
use crate::git_blocks::RunBlocks;
use crate::git_host::GitHost;
use crate::relay::{BranchPlan, Job};
use crate::run::log;

/// What the bridge needs for git in a run.
#[derive(Clone)]
pub struct RunGit {
    pub host: Arc<GitHost>,
    pub walk: Walk,
}

/// A change of the own branch of a chat, for the state of the bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WorktreeChange {
    #[default]
    Same,
    /// A new worktree, or `None` when the old one is gone and no new one came.
    Set(Option<ChatWorktree>),
}

/// The folder of a run, and its own branch.
pub struct Started {
    pub folder: String,
    pub worktree: Option<ChatWorktree>,
    pub change: WorktreeChange,
}

impl RunGit {
    /// The worktree of the chat, made at its first run. An error stops the run.
    fn folder(
        &self,
        job: &Job,
        plan: BranchPlan,
    ) -> Result<(Option<ChatWorktree>, WorktreeChange), String> {
        let name = match plan {
            BranchPlan::Plain => return Ok((None, WorktreeChange::Same)),
            BranchPlan::Use(worktree) if worktree.exists() => {
                return Ok((Some(worktree), WorktreeChange::Same));
            }
            BranchPlan::Use(gone) => gone
                .branch
                .trim_start_matches(chat_branch::BRANCH_PREFIX)
                .to_owned(),
            BranchPlan::Make { name } => name,
        };
        let made = chat_branch::make(
            &self.host,
            &self.walk,
            &job.chat,
            &name,
            Path::new(&job.cwd),
        )?;
        if let Some(worktree) = &made {
            log(&format!(
                "{} works on {} in {}",
                job.chat, worktree.branch, worktree.worktree
            ));
        }
        Ok((made.clone(), WorktreeChange::Set(made)))
    }

    pub fn start(&self, job: &Job, plan: BranchPlan) -> Result<Started, String> {
        let (worktree, change) = self.folder(job, plan)?;
        let folder = worktree
            .as_ref()
            .map_or_else(|| job.cwd.clone(), |w| w.folder.clone());
        Ok(Started {
            folder,
            worktree,
            change,
        })
    }

    /// The blocks of the bridge under the reply.
    pub fn end(&self, started: &Started) -> RunBlocks {
        let folder = Path::new(&started.folder);
        RunBlocks {
            branch: chat_branch::branch_info(&self.host, folder, started.worktree.as_ref()),
        }
    }
}
