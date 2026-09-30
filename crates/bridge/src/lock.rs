//! Only one bridge runs at a time (SPEC.md 8.4).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::fs_safe::write_atomic;

const LOCK_FILE: &str = "bridge.lock";
// Windows does not let another process read a locked file, so the id has its own file.
const PID_FILE: &str = "bridge.pid";

/// The bridge holds this while it runs. The OS releases it when the process stops,
/// also after a crash.
pub struct BridgeLock {
    _file: File,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Bridge {
    Stopped,
    /// The process id is `None` when `bridge.pid` is missing or damaged.
    Runs(Option<u32>),
}

fn open(dir: &Path) -> Result<File> {
    let path = dir.join(LOCK_FILE);
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))
}

fn pid(dir: &Path) -> Option<u32> {
    std::fs::read_to_string(dir.join(PID_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// A status check holds the lock for a moment, so a start tries again for this long.
const STATUS_CHECK_PATIENCE: Duration = Duration::from_millis(500);

fn try_lock_with_patience(file: &File) -> Result<(), TryLockError> {
    let end = Instant::now() + STATUS_CHECK_PATIENCE;
    loop {
        match file.try_lock() {
            Err(TryLockError::WouldBlock) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

pub fn take(dir: &Path) -> Result<BridgeLock> {
    let file = open(dir)?;
    match try_lock_with_patience(&file) {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let id = pid(dir).map_or(String::new(), |id| format!(" (process {id})"));
            bail!(
                "the desktop app is already running{id}. To restart it, run gnomish-relay restart"
            );
        }
        Err(TryLockError::Error(e)) => return Err(e).context("cannot lock the bridge"),
    }
    write_atomic(dir, PID_FILE, std::process::id().to_string().as_bytes())?;
    Ok(BridgeLock { _file: file })
}

pub fn status(dir: &Path) -> Result<Bridge> {
    let file = open(dir)?;
    match file.try_lock() {
        Ok(()) => Ok(Bridge::Stopped),
        Err(TryLockError::WouldBlock) => Ok(Bridge::Runs(pid(dir))),
        Err(TryLockError::Error(e)) => Err(e).context("cannot read the bridge lock"),
    }
}

/// Returns `false` when the bridge still runs at the end of `timeout`.
pub fn wait_until_stopped(dir: &Path, timeout: Duration) -> Result<bool> {
    let end = Instant::now() + timeout;
    while status(dir)? != Bridge::Stopped {
        if Instant::now() >= end {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(true)
}

/// Returns `false` when no bridge runs at the end of `timeout`.
pub fn wait_until_runs(dir: &Path, timeout: Duration) -> Result<bool> {
    let end = Instant::now() + timeout;
    while status(dir)? == Bridge::Stopped {
        if Instant::now() >= end {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_bridge_cannot_take_the_lock_and_learns_the_first_process() {
        let dir = tempfile::tempdir().unwrap();
        let _first = take(dir.path()).unwrap();
        let error = take(dir.path()).err().unwrap().to_string();
        assert!(
            error.contains(&format!("process {}", std::process::id())),
            "{error}"
        );
    }

    #[test]
    fn the_status_gives_the_process_of_the_running_bridge() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(status(dir.path()).unwrap(), Bridge::Stopped);
        let lock = take(dir.path()).unwrap();
        assert_eq!(
            status(dir.path()).unwrap(),
            Bridge::Runs(Some(std::process::id()))
        );
        drop(lock);
        assert_eq!(status(dir.path()).unwrap(), Bridge::Stopped);
    }

    #[test]
    fn a_lock_with_no_pid_file_runs_with_no_process_id() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = take(dir.path()).unwrap();
        std::fs::remove_file(dir.path().join(PID_FILE)).unwrap();
        assert_eq!(status(dir.path()).unwrap(), Bridge::Runs(None));
    }

    #[test]
    fn the_wait_ends_when_the_bridge_stops_or_the_time_is_up() {
        let dir = tempfile::tempdir().unwrap();
        let lock = take(dir.path()).unwrap();
        assert!(!wait_until_stopped(dir.path(), Duration::from_millis(200)).unwrap());
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(lock);
        });
        assert!(wait_until_stopped(dir.path(), Duration::from_secs(10)).unwrap());
        release.join().unwrap();
    }

    /// `status` holds the lock for a moment to test it, so a start at that moment
    /// would see another bridge.
    #[test]
    fn a_status_check_at_the_same_moment_does_not_stop_a_start() {
        let dir = tempfile::tempdir().unwrap();
        let probe = open(dir.path()).unwrap();
        probe.try_lock().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(probe);
        });
        assert!(take(dir.path()).is_ok());
        release.join().unwrap();
    }

    #[test]
    fn the_wait_for_a_start_ends_when_the_bridge_runs_or_the_time_is_up() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!wait_until_runs(dir.path(), Duration::from_millis(200)).unwrap());
        let path = dir.path().to_owned();
        let start = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let lock = take(&path).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(lock);
        });
        assert!(wait_until_runs(dir.path(), Duration::from_secs(10)).unwrap());
        start.join().unwrap();
    }
}
