//! One sandbox of commands for each run on Linux (SPEC.md 6.6.4, "One sandbox for each
//! run"). A holder process keeps its namespaces open, and each command joins them with
//! `nsenter`. So a server that one command starts in the background answers a later
//! command, and all of it ends with the run.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::command_sandbox::{ProxyEnd, WALLS_VAR, Walls, command_env, mount_args};
use crate::forward::{FORWARD_FLAG, ports_arg};
use crate::story_sandbox::Sandbox;

/// The flag of the forwarder that holds the sandbox open and runs nothing.
pub const HOLD_FLAG: &str = "--hold";
/// The info of `bwrap` is a few hundred bytes.
const MAX_INFO: usize = 4096;
const NAMESPACES: [&str; 3] = ["user", "mnt", "net"];

/// The mounts of the run, its own namespaces, and a holder that runs until the run ends.
/// `bwrap` writes the process id of the holder to its standard error, and the holder
/// writes `READY` to its standard output.
pub fn holder_args(walls: &Walls) -> Vec<OsString> {
    let mut args = mount_args(walls);
    for word in [
        "--chdir",
        "/",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--info-fd",
        "2",
        "--",
    ] {
        args.push(word.into());
    }
    match &walls.proxy {
        Some(ProxyEnd::Socket { socket, forwarder }) => args.extend([
            forwarder.into(),
            FORWARD_FLAG.into(),
            socket.into(),
            ports_arg(&walls.local_ports).into(),
            HOLD_FLAG.into(),
        ]),
        _ => args.extend(["sh".into(), "-c".into(), NO_PROXY_HOLDER.into()]),
    }
    args
}

/// `bwrap` writes its info before the mounts are done. The holder writes this line when
/// it runs, so the mounts are done then, and a command that joins sees the walls.
pub const READY: &[u8] = b"ready\n";
const NO_PROXY_HOLDER: &str = "echo ready && exec sleep infinity";

/// What names the holder for sure: a process id alone can come back for another process.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HolderId {
    pub pid: u32,
    /// Field 22 of `/proc/<pid>/stat`: the start time of the process.
    pub start: u64,
    /// The inodes of the user, mount, and network namespaces.
    pub namespaces: [u64; 3],
}

/// Next to the walls file, in the data folder, which no command sees.
pub fn holder_file(walls_file: &Path) -> PathBuf {
    walls_file.with_extension("holder")
}

/// The holder of one run. It ends, and its file goes away, when this drops.
pub struct Holder {
    child: Child,
    file: PathBuf,
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Starts the holder of the walls in `walls_file`, and notes who it is.
pub fn start_holder(walls: &Walls, walls_file: &Path) -> Result<Holder, String> {
    let Sandbox::Bwrap(bwrap) = &walls.tool else {
        return Err("Only bwrap has one sandbox for each run.".into());
    };
    let mut child = Command::new(bwrap)
        .args(holder_args(walls))
        .env_clear()
        .envs(command_env(walls, |name| std::env::var_os(name)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("The sandbox of the run did not start: {e}"))?;
    let pid = child
        .stderr
        .take()
        .ok_or(())
        .and_then(|mut e| read_info(&mut e));
    let ready = child
        .stdout
        .take()
        .ok_or(())
        .and_then(|mut o| read_ready(&mut o));
    let info = pid
        .and_then(|pid| ready.map(|()| pid))
        .map_err(|()| "The sandbox of the run did not start.");
    let file = holder_file(walls_file);
    let holder = Holder { child, file };
    let id = read_id(info?)?;
    let text = serde_json::to_string(&id).map_err(|e| format!("No holder file: {e}"))?;
    std::fs::write(&holder.file, text).map_err(|e| format!("No holder file: {e}"))?;
    Ok(holder)
}

/// The process id in the info of `bwrap`, which ends at its first `}`.
fn read_info(out: &mut impl Read) -> Result<u32, ()> {
    let mut info = Vec::new();
    let mut byte = [0u8; 1];
    while !info.ends_with(b"}") && info.len() < MAX_INFO {
        match out.read(&mut byte) {
            Ok(1) => info.push(byte[0]),
            _ => return Err(()),
        }
    }
    let value: serde_json::Value = serde_json::from_slice(&info).map_err(|_| ())?;
    let pid = value.get("child-pid").and_then(serde_json::Value::as_u64);
    pid.and_then(|p| u32::try_from(p).ok()).ok_or(())
}

/// The line of the holder after the info, which says that it runs.
fn read_ready(out: &mut impl Read) -> Result<(), ()> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while !line.ends_with(READY) && line.len() < MAX_INFO {
        match out.read(&mut byte) {
            Ok(1) => line.push(byte[0]),
            _ => return Err(()),
        }
    }
    if line.ends_with(READY) {
        Ok(())
    } else {
        Err(())
    }
}

/// The id of process `pid` now, from `/proc`.
pub fn read_id(pid: u32) -> Result<HolderId, String> {
    let gone = || format!("The sandbox of the run is gone (process {pid}).");
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|_| gone())?;
    let started = start_time(&stat).ok_or_else(gone)?;
    let mut namespaces = [0; 3];
    for (slot, name) in namespaces.iter_mut().zip(NAMESPACES) {
        *slot = namespace(&format!("/proc/{pid}/ns/{name}")).ok_or_else(gone)?;
    }
    Ok(HolderId {
        pid,
        start: started,
        namespaces,
    })
}

/// The name of a process can hold spaces and `)`, so the fields start after the last `)`.
fn start_time(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(unix)]
fn namespace(path: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

#[cfg(not(unix))]
fn namespace(_path: &str) -> Option<u64> {
    None
}

/// The holder of the run, only when it is the same process that the bridge noted, and
/// only when its namespaces are not the ones of this process: a stale or false file
/// never lets a command run with no walls.
pub fn check_holder(walls_file: &Path) -> Result<HolderId, String> {
    let file = holder_file(walls_file);
    let text = std::fs::read_to_string(&file)
        .map_err(|_| "The sandbox of the run is not running.".to_owned())?;
    let noted: HolderId =
        serde_json::from_str(&text).map_err(|e| format!("bad holder file: {e}"))?;
    let now = read_id(noted.pid)?;
    if now != noted {
        return Err("The sandbox of the run is gone: another process has its id.".into());
    }
    let own = own_namespaces()?;
    if now.namespaces.iter().zip(own).any(|(a, b)| *a == b) {
        return Err("The holder shares a namespace with the wrapper, so it is no sandbox.".into());
    }
    Ok(now)
}

fn own_namespaces() -> Result<[u64; 3], String> {
    let mut own = [0; 3];
    for (slot, name) in own.iter_mut().zip(NAMESPACES) {
        *slot = namespace(&format!("/proc/self/ns/{name}"))
            .ok_or("This process has no namespaces in /proc.")?;
    }
    Ok(own)
}

/// The shell changes to the working folder inside the sandbox, then runs the command:
/// `$0` is the folder, `$1` the shell, and `$2` the command.
const CD_THEN_RUN: &str = "cd -- \"$0\" || exit 126; exec \"$1\" -c \"$2\"";

/// `nsenter` puts `shell -c command` into the namespaces of the holder. It keeps the user
/// id, so the command has no capabilities there. No `--wd=<folder>`: nsenter opens that
/// folder before it joins, so the command would start in a folder of this computer, and
/// `cd ..` would leave the walls. The join puts the command at the root of the sandbox.
pub fn join_args(id: &HolderId, cwd: &Path, shell: &Path, command: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--target",
        &id.pid.to_string(),
        "--user",
        "--mount",
        "--net",
        "--pid",
        "--preserve-credentials",
        "--",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.extend([
        shell.into(),
        "-c".into(),
        CD_THEN_RUN.into(),
        cwd.into(),
        shell.into(),
        command.into(),
    ]);
    args
}

/// The forwarder in the wall of the agent starts the holder of the run of Claude, when
/// `GNOMISH_RELAY_SANDBOX` names walls of `bwrap`. It must start inside the wall: from
/// outside, the namespaces of the holder are out of reach for a command.
pub fn hold_for_agent() -> Option<Holder> {
    let file = std::env::var_os(WALLS_VAR)?;
    let text = std::fs::read_to_string(&file).ok()?;
    let walls: Walls = serde_json::from_str(&text).ok()?;
    if !matches!(walls.tool, Sandbox::Bwrap(_)) {
        return None;
    }
    match start_holder(&walls, Path::new(&file)) {
        Ok(holder) => Some(holder),
        Err(e) => {
            eprintln!("gnomish-relay sandbox: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_time_is_field_22_also_with_a_bracket_in_the_name() {
        let fields: Vec<String> = (3..=25).map(|n| n.to_string()).collect();
        let stat = format!("41 (a) b (c)) {}", fields.join(" "));

        assert_eq!(start_time(&stat), Some(22));
        assert_eq!(start_time("41 (x"), None);
    }

    #[test]
    fn the_info_of_bwrap_gives_the_process_id_of_the_holder() {
        let info = br#"{
    "child-pid": 6,
    "net-namespace": 4026535407
}and more"#;

        assert_eq!(read_info(&mut &info[..]), Ok(6));
        assert_eq!(read_info(&mut &b"{\"x\": 1}"[..]), Err(()));
        assert_eq!(read_info(&mut &b"{\"child-pid\": 6"[..]), Err(()));
    }

    #[test]
    fn the_holder_is_ready_only_after_its_line() {
        assert_eq!(read_ready(&mut &b"\nready\n"[..]), Ok(()));
        assert_eq!(read_ready(&mut &b"\nread"[..]), Err(()));
        assert_eq!(read_ready(&mut &b""[..]), Err(()));
    }

    #[test]
    fn nsenter_keeps_the_user_id_and_joins_every_namespace_of_the_holder() {
        let id = HolderId {
            pid: 42,
            start: 1,
            namespaces: [1, 2, 3],
        };

        let args = join_args(
            &id,
            Path::new("/home/x/app"),
            Path::new("/bin/bash"),
            "a; b",
        );

        let words: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            words,
            [
                "--target",
                "42",
                "--user",
                "--mount",
                "--net",
                "--pid",
                "--preserve-credentials",
                "--",
                "/bin/bash",
                "-c",
                CD_THEN_RUN,
                "/home/x/app",
                "/bin/bash",
                "a; b"
            ]
        );
        assert!(!words.iter().any(|w| w.starts_with("--wd")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_holder_file_of_a_process_of_this_computer_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let walls_file = dir.path().join("run.json");
        let own = read_id(std::process::id()).unwrap();
        std::fs::write(
            holder_file(&walls_file),
            serde_json::to_string(&own).unwrap(),
        )
        .unwrap();

        let error = check_holder(&walls_file).unwrap_err();

        assert!(error.contains("shares a namespace"), "{error}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_holder_file_with_another_start_time_or_no_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let walls_file = dir.path().join("run.json");
        let mut own = read_id(std::process::id()).unwrap();
        own.start += 1;

        let missing = check_holder(&walls_file).unwrap_err();
        std::fs::write(
            holder_file(&walls_file),
            serde_json::to_string(&own).unwrap(),
        )
        .unwrap();
        let other = check_holder(&walls_file).unwrap_err();

        assert!(missing.contains("not running"), "{missing}");
        assert!(other.contains("another process"), "{other}");
        assert!(read_id(u32::MAX).is_err());
    }
}
