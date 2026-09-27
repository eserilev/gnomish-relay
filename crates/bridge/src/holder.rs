//! One sandbox of commands for each run on Linux (SPEC.md 6.6.4, "One sandbox for each
//! run"). The holder is this program inside the walls of the run. It keeps the namespaces
//! open, and it starts each command that the wrapper sends it (`launch.rs`). So a server
//! that one command starts in the background answers a later command, and all of it ends
//! with the run.

use std::ffi::OsString;
use std::io::Read;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::command_sandbox::{ProxyEnd, Walls, command_env, mount_args};
use crate::forward::ports_arg;
use crate::story_sandbox::Sandbox;

/// The flag of this program that holds the sandbox of a run.
pub const HOLD_FLAG: &str = "--sandbox-hold";
/// The socket of the holder, in the temp folder of the run.
const LAUNCH_SOCKET: &str = ".gnomish-relay-launch";
/// The holder writes this line when it takes commands.
pub const READY: &[u8] = b"ready\n";

pub fn launch_socket(walls: &Walls) -> PathBuf {
    walls.temp.join(LAUNCH_SOCKET)
}

/// The mounts of the run, its own namespaces, and the holder, which runs until the run
/// ends: `<this program> --sandbox-hold <launch socket> <proxy socket> <local ports>`.
pub fn holder_args(walls: &Walls) -> Vec<OsString> {
    let mut args = mount_args(walls);
    for word in [
        "--chdir",
        "/",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--",
    ] {
        args.push(word.into());
    }
    let proxy = match &walls.proxy {
        Some(ProxyEnd::Socket { socket, .. }) => socket.clone().into_os_string(),
        _ => OsString::new(),
    };
    args.extend([
        walls.wrapper.clone().into(),
        HOLD_FLAG.into(),
        launch_socket(walls).into(),
        proxy,
        ports_arg(&walls.local_ports).into(),
    ]);
    args
}

/// The holder of one run. It ends, and every process in its sandbox with it, when this
/// drops.
pub struct Holder {
    child: Child,
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the holder of `walls`, and waits until it takes commands.
pub fn start_holder(walls: &Walls) -> Result<Holder, String> {
    let Sandbox::Bwrap(bwrap) = &walls.tool else {
        return Err("Only bwrap has one sandbox for each run.".into());
    };
    let mut child = Command::new(bwrap)
        .args(holder_args(walls))
        .env_clear()
        .envs(command_env(walls, |name| std::env::var_os(name)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("The sandbox of the run did not start: {e}"))?;
    let ready = child
        .stdout
        .take()
        .ok_or(())
        .and_then(|mut out| read_ready(&mut out));
    let holder = Holder { child };
    ready.map_err(|()| "The sandbox of the run did not start.".to_owned())?;
    Ok(holder)
}

fn read_ready(out: &mut impl Read) -> Result<(), ()> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while !line.ends_with(READY) && line.len() < 4096 {
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

/// The holder inside the sandbox: the forwarder of the proxy, and the socket that takes
/// the commands of the run. It ends only with the sandbox.
#[cfg(unix)]
pub fn run_holder(launch: &Path, proxy: &str, ports: &str) -> i32 {
    match hold(launch, proxy, ports) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gnomish-relay sandbox: {e}");
            126
        }
    }
}

#[cfg(unix)]
fn hold(launch: &Path, proxy: &str, ports: &str) -> Result<(), String> {
    use std::io::Write;
    let local_ports = crate::forward::parse_ports(ports)?;
    if !proxy.is_empty() {
        let forward = crate::forward::Forward {
            socket: Path::new(proxy),
            port: crate::forward::INNER_PORT,
            local_ports: &local_ports,
        };
        crate::forward::listen(&forward)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(launch)
        .map_err(|e| format!("no socket for the commands: {e}"))?;
    let mut out = std::io::stdout();
    out.write_all(READY)
        .and_then(|()| out.flush())
        .map_err(|e| format!("cannot say ready: {e}"))?;
    crate::launch::serve(&listener);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_holder_is_ready_only_after_its_line() {
        assert_eq!(read_ready(&mut &b"\nready\n"[..]), Ok(()));
        assert_eq!(read_ready(&mut &b"\nread"[..]), Err(()));
        assert_eq!(read_ready(&mut &b""[..]), Err(()));
    }
}
