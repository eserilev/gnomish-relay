//! Who connects to the serve socket (SPEC.md 11.7.8). A command of a run can reach the
//! socket, for example a Codex command, and must never answer its own request.

use crate::hook::JOB_VAR;

/// Why `serve` refuses a peer: it is one of its own runs.
pub const A_RUN: &str = "a run of an agent on this computer can't take the link";

/// True when `pid` has `ancestor` above it. `parent_of` gives the parent of a process.
pub fn descends_from(pid: u32, ancestor: u32, parent_of: impl Fn(u32) -> Option<u32>) -> bool {
    let mut current = pid;
    // A chain of processes is short. The bound stops a loop in a changing process table.
    for _ in 0..4096 {
        let Some(parent) = parent_of(current) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        if parent <= 1 || parent == current {
            return false;
        }
        current = parent;
    }
    false
}

/// The environment of a process as `/proc/<pid>/environ` holds it: `NAME=value` with a
/// zero byte after each.
pub fn has_job_marker(environ: &[u8]) -> bool {
    let marker = format!("{JOB_VAR}=1");
    environ
        .split(|b| *b == 0)
        .any(|entry| entry == marker.as_bytes())
}

/// The parent field of `/proc/<pid>/stat`. The name in parentheses can hold spaces and
/// parentheses, so the fields start after the last `)`.
pub fn parent_in_stat(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(target_os = "linux")]
pub use linux::refusal;

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::net::UnixStream;

    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};

    use super::{A_RUN, descends_from, has_job_marker, parent_in_stat};

    fn parent_of(pid: u32) -> Option<u32> {
        parent_in_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }

    /// `None` lets the peer in. A peer that the checks cannot read gets in too: then
    /// `/proc` is hidden, and the socket is the only wall, as on macOS.
    pub fn refusal_of(pid: u32, serve: u32) -> Option<&'static str> {
        if descends_from(pid, serve, parent_of) {
            return Some(A_RUN);
        }
        let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        has_job_marker(&environ).then_some(A_RUN)
    }

    pub fn refusal(stream: &UnixStream) -> Option<&'static str> {
        let credentials = getsockopt(stream, PeerCredentials).ok()?;
        let pid = u32::try_from(credentials.pid()).ok()?;
        refusal_of(pid, std::process::id())
    }

    #[cfg(test)]
    mod tests {
        use std::process::{Command, Stdio};

        use super::*;

        fn sleeper(job: bool) -> std::process::Child {
            let mut command = Command::new("sleep");
            command.arg("5").stdout(Stdio::null());
            if job {
                command.env("GNOMISH_RELAY_JOB", "1");
            } else {
                command.env_remove("GNOMISH_RELAY_JOB");
            }
            command.spawn().unwrap()
        }

        #[test]
        fn a_child_of_serve_is_refused() {
            let mut child = sleeper(false);

            let why = refusal_of(child.id(), std::process::id());

            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(why, Some(A_RUN));
        }

        #[test]
        fn a_process_with_the_marker_of_a_run_is_refused() {
            let mut child = sleeper(true);

            let why = refusal_of(child.id(), u32::MAX);

            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(why, Some(A_RUN));
        }

        /// No real process has the largest id, so it stands for an unrelated `serve`.
        #[test]
        fn another_process_of_the_user_gets_in() {
            let mut child = sleeper(false);

            let why = refusal_of(child.id(), u32::MAX);

            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(why, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn a_grandchild_descends_from_its_grandparent() {
        let parents: BTreeMap<u32, u32> = [(30, 20), (20, 10), (10, 1)].into();
        let parent_of = |pid| parents.get(&pid).copied();

        assert!(descends_from(30, 10, parent_of));
        assert!(!descends_from(10, 30, parent_of));
        assert!(!descends_from(30, 99, parent_of));
    }

    #[test]
    fn a_loop_in_the_process_table_ends() {
        let parents: BTreeMap<u32, u32> = [(30, 20), (20, 30)].into();

        assert!(!descends_from(30, 99, |pid| parents.get(&pid).copied()));
    }

    #[test]
    fn the_marker_must_be_a_whole_entry_with_the_value_1() {
        assert!(has_job_marker(b"HOME=/h\0GNOMISH_RELAY_JOB=1\0"));
        assert!(!has_job_marker(b"GNOMISH_RELAY_JOB=0\0"));
        assert!(!has_job_marker(b"XGNOMISH_RELAY_JOB=1\0"));
    }

    #[test]
    fn the_parent_comes_after_a_name_with_spaces_and_parentheses() {
        assert_eq!(parent_in_stat("42 (a) b)) S 7 42 42"), Some(7));
        assert_eq!(parent_in_stat("garbage"), None);
    }
}
