//! The chat folders with a run in progress (SPEC.md 9.11). Two runs at once in one
//! folder mix their changes, so a summary cannot tell them apart.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub struct ActiveFolders {
    runs: Arc<Mutex<Vec<ActiveRun>>>,
}

struct ActiveRun {
    id: u64,
    folder: PathBuf,
    shared: bool,
}

/// One run in `ActiveFolders`. The run leaves the list when this goes, even after a panic.
pub struct ActiveGuard {
    folders: ActiveFolders,
    id: u64,
}

/// One folder inside the other: a chat at the top of a repository sees the changes of
/// a chat in a subfolder of it.
fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

impl ActiveFolders {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<ActiveRun>> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks this run and every run that overlaps it as shared.
    pub fn begin(&self, folder: &Path) -> ActiveGuard {
        let mut runs = self.lock();
        let id = runs.iter().map(|r| r.id + 1).max().unwrap_or(0);
        let mut shared = false;
        for run in runs.iter_mut().filter(|r| overlap(&r.folder, folder)) {
            run.shared = true;
            shared = true;
        }
        runs.push(ActiveRun {
            id,
            folder: folder.to_owned(),
            shared,
        });
        ActiveGuard {
            folders: self.clone(),
            id,
        }
    }
}

impl ActiveGuard {
    /// True once another run worked in an overlapping folder during this run.
    pub fn shared(&self) -> bool {
        self.folders
            .lock()
            .iter()
            .any(|r| r.id == self.id && r.shared)
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.folders.lock().retain(|r| r.id != self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_runs_at_once_in_one_folder_are_both_shared() {
        let folders = ActiveFolders::default();

        let first = folders.begin(Path::new("/code/app"));
        let second = folders.begin(Path::new("/code/app"));

        assert!(first.shared());
        assert!(second.shared());
    }

    #[test]
    fn a_run_in_a_subfolder_shares_the_folder_above() {
        let folders = ActiveFolders::default();

        let top = folders.begin(Path::new("/code/app"));
        let sub = folders.begin(Path::new("/code/app/web"));

        assert!(top.shared());
        assert!(sub.shared());
    }

    #[test]
    fn runs_in_two_sibling_folders_are_not_shared() {
        let folders = ActiveFolders::default();

        let web = folders.begin(Path::new("/code/app/web"));
        let api = folders.begin(Path::new("/code/app/api"));

        assert!(!web.shared());
        assert!(!api.shared());
    }

    #[test]
    fn a_run_after_another_one_ended_is_not_shared() {
        let folders = ActiveFolders::default();
        drop(folders.begin(Path::new("/code/app")));

        let later = folders.begin(Path::new("/code/app"));

        assert!(!later.shared());
    }

    #[test]
    fn a_run_that_ended_keeps_no_mark_on_the_next_one() {
        let folders = ActiveFolders::default();
        let first = folders.begin(Path::new("/code/app"));
        let second = folders.begin(Path::new("/code/app"));
        drop(first);
        drop(second);

        let third = folders.begin(Path::new("/code/app"));

        assert!(!third.shared());
    }
}
