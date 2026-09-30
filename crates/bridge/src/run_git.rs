//! Git around one run, in the thread of the run (SPEC.md 9.11): the own branch before
//! the agent starts, and the snapshot, the branch, and the checks after it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::active_folders::{ActiveFolders, ActiveGuard};
use crate::chat_branch::{self, ChatWorktree};
use crate::ci_checks::{self, CiChecks};
use crate::folder_walk::Walk;
use crate::git_blocks::RunBlocks;
use crate::git_host::GitHost;
use crate::relay::{BranchPlan, Job};
use crate::run::log;
use crate::run_changes::{Outcome, RunChanges, Snapshot, changes, snapshot};

/// What the bridge needs for git in a run.
#[derive(Clone)]
pub struct RunGit {
    pub host: Arc<GitHost>,
    pub ci: CiChecks,
    pub walk: Walk,
    pub active: ActiveFolders,
}

/// A change of the own branch of a chat, for the state of the bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WorktreeChange {
    #[default]
    Same,
    /// A new worktree, or `None` when the old one is gone and no new one came.
    Set(Option<ChatWorktree>),
}

/// The folder of a run, and the state of the repository when the run started.
pub struct Started {
    pub folder: String,
    pub worktree: Option<ChatWorktree>,
    pub change: WorktreeChange,
    start: Option<(PathBuf, Snapshot)>,
    active: ActiveGuard,
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
        let real = Path::new(&folder).canonicalize();
        let active = self
            .active
            .begin(real.as_deref().unwrap_or(Path::new(&folder)));
        let start = self.snapshot_of(Path::new(&folder));
        Ok(Started {
            folder,
            worktree,
            change,
            start,
            active,
        })
    }

    /// `None` outside a repository, and when git fails: the run goes on with no summary.
    fn snapshot_of(&self, folder: &Path) -> Option<(PathBuf, Snapshot)> {
        let top = chat_branch::repo_top(&self.host, folder)?;
        match snapshot(&self.host, &top) {
            Ok(snap) => Some((top, snap)),
            Err(e) => {
                log(&format!("no change summary in {}: {e}", top.display()));
                None
            }
        }
    }

    /// The blocks of the bridge under the reply. The test line comes from the events.
    pub fn end(&self, job: &Job, started: &Started) -> RunBlocks {
        let folder = Path::new(&started.folder);
        let branch = chat_branch::branch_info(&self.host, folder, started.worktree.as_ref());
        let ci = branch
            .as_ref()
            .filter(|b| !b.branch.is_empty())
            .and_then(|b| self.checks(folder, &b.branch));
        RunBlocks {
            branch,
            changes: self.changes(job, started),
            tests: None,
            ci,
        }
    }

    /// Only the files in the chat folder count: another chat can work in another
    /// folder of the same repository at the same time.
    fn changes(&self, job: &Job, started: &Started) -> Option<RunChanges> {
        let (top, start) = started.start.as_ref()?;
        let end = snapshot(&self.host, top).ok()?;
        let within = within_top(top, Path::new(&started.folder))?;
        let (files, odd_names) = changes(&self.host, top, &start.tree, &end.tree, &within).ok()?;
        if files.is_empty() {
            return None;
        }
        Some(RunChanges {
            chat: job.chat.clone(),
            id: job.id,
            top: top.to_string_lossy().into_owned(),
            start: start.clone(),
            end,
            files,
            odd_names,
            shared: started.active.shared(),
            outcome: Outcome::Open,
        })
    }

    fn checks(&self, folder: &Path, branch: &str) -> Option<ci_checks::CiCounts> {
        let CiChecks::On { program } = &self.ci else {
            return None;
        };
        match ci_checks::checks(program, folder, branch) {
            Ok(counts) => counts,
            Err(e) => {
                log(&format!("no CI checks for {branch}: {}", e.text()));
                None
            }
        }
    }
}

/// The chat folder relative to the top of its repository, with `/` as git writes it.
/// A folder name that is not UTF-8 gets no summary.
fn within_top(top: &Path, folder: &Path) -> Option<String> {
    let real = folder.canonicalize().ok()?;
    let relative = real.strip_prefix(top).ok()?;
    let parts: Option<Vec<&str>> = relative.iter().map(|p| p.to_str()).collect();
    Some(parts?.join("/"))
}
