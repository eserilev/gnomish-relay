//! The children of the desktop app. Each one leads its own process group, so a kill also
//! ends the programs that it started (SPEC.md 9.4). A signal to the app never reaches
//! them, so the app keeps a list of the groups, and `end_every_group` ends them at its exit.

use std::collections::BTreeSet;
use std::io;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The groups that run, and whether the app is closing.
#[derive(Default)]
pub struct Groups {
    /// The process ids of the group leaders.
    leaders: BTreeSet<u32>,
    /// Set by `end_every_group`. After it, no new child starts.
    closing: bool,
}

/// The groups of the desktop app.
static GROUPS: Mutex<Groups> = Mutex::new(Groups {
    leaders: BTreeSet::new(),
    closing: false,
});

pub const CLOSING: &str = "The desktop app is stopping.";

fn lock(groups: &Mutex<Groups>) -> MutexGuard<'_, Groups> {
    groups.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts `command` as the leader of a new process group. End it with `end`.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    spawn_in(&GROUPS, command)
}

/// The lock covers the start, so `end_every_group` never misses a new child.
fn spawn_in(groups: &Mutex<Groups>, command: &mut Command) -> io::Result<Child> {
    let mut groups = lock(groups);
    if groups.closing {
        return Err(io::Error::other(CLOSING));
    }
    own_group(command);
    let child = command.spawn()?;
    groups.leaders.insert(child.id());
    Ok(child)
}

/// Kills the group of `child`, then reaps the child. The status of a child that exited
/// by itself stays its own.
pub fn end(child: &mut Child) -> io::Result<ExitStatus> {
    end_in(&GROUPS, child)
}

/// The id leaves the list before the reap: after it, the id can name a new process.
fn end_in(groups: &Mutex<Groups>, child: &mut Child) -> io::Result<ExitStatus> {
    kill_group(child.id());
    let _ = child.kill();
    lock(groups).leaders.remove(&child.id());
    child.wait()
}

/// Kills every group that `spawn` started and `end` did not end yet. After it, `spawn`
/// refuses.
pub fn end_every_group() {
    end_every_group_in(&GROUPS);
}

fn end_every_group_in(groups: &Mutex<Groups>) {
    let mut groups = lock(groups);
    groups.closing = true;
    for id in std::mem::take(&mut groups.leaders) {
        kill_group(id);
    }
}

/// True once the child exited.
// On Linux the child stays unreaped, so its id cannot name another group before `end`.
#[cfg(target_os = "linux")]
pub fn has_exited(child: &mut Child) -> bool {
    use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid};
    use nix::unistd::Pid;
    let Ok(id) = i32::try_from(child.id()) else {
        return true;
    };
    let flags = WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT;
    match waitid(Id::Pid(Pid::from_raw(id)), flags) {
        Ok(WaitStatus::StillAlive) => false,
        Ok(_) => true,
        // The child is reaped already, for example by `end`.
        Err(_) => !matches!(child.try_wait(), Ok(None)),
    }
}

/// True once the child exited.
// TODO: peek with no reap when nix gives `waitid` on macOS. Until then `end` can, rarely,
// kill a new group that took the id of an empty group.
#[cfg(not(target_os = "linux"))]
pub fn has_exited(child: &mut Child) -> bool {
    !matches!(child.try_wait(), Ok(None))
}

#[cfg(unix)]
fn own_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

// TODO: put the child in a Job object with KILL_ON_JOB_CLOSE when a crate gives one with
// no `unsafe` in this workspace. Until then `kill_group` stops the tree with `taskkill /T`.
#[cfg(not(unix))]
fn own_group(_command: &mut Command) {}

/// While a member of the group or its unreaped leader lives, `id` names only this group.
#[cfg(unix)]
fn kill_group(id: u32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    let Ok(id) = i32::try_from(id) else {
        return;
    };
    let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
}

/// `taskkill /T` misses a grandchild whose parent already ended. Windows reuses no id
/// while a `Child` holds the process handle.
#[cfg(not(unix))]
fn kill_group(id: u32) {
    use std::process::Stdio;
    let _ = Command::new("taskkill")
        .args(["/PID", &id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn exit_3() -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 3"]);
        command
    }

    #[test]
    fn has_exited_leaves_the_child_unreaped_and_end_gives_its_status() {
        let groups = Mutex::new(Groups::default());
        let mut child = spawn_in(&groups, &mut exit_3()).unwrap();
        let start = Instant::now();
        while !has_exited(&mut child) {
            assert!(start.elapsed() < Duration::from_secs(10), "never exited");
            std::thread::sleep(Duration::from_millis(10));
        }

        let zombie = std::path::Path::new("/proc").join(child.id().to_string());
        assert!(zombie.exists(), "the leader is not reaped yet");
        let status = end_in(&groups, &mut child).unwrap();

        assert_eq!(status.code(), Some(3));
        assert!(lock(&groups).leaders.is_empty());
    }

    #[test]
    fn no_child_starts_after_the_end_of_every_group() {
        let groups = Mutex::new(Groups::default());
        let mut running =
            spawn_in(&groups, Command::new("/bin/sh").args(["-c", "sleep 30"])).unwrap();

        end_every_group_in(&groups);

        let refused = spawn_in(&groups, &mut exit_3()).unwrap_err();
        assert_eq!(refused.to_string(), CLOSING);
        assert!(running.wait().is_ok_and(|status| !status.success()));
    }
}
