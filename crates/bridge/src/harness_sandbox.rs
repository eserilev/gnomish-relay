//! The whole harness of a `command` agent runs inside the walls of its run (SPEC.md 9.2,
//! 6.6.4): the bridge cannot see its tool calls, so the harness and every program that it
//! starts share one sandbox.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::agent_wall::EXEC_FLAG;
use crate::command_sandbox::{ProxyEnd, Walls, bytes_arg, mount_args, seatbelt_profile};
use crate::forward::{FORWARD_FLAG, ports_arg};
use crate::story_sandbox::Sandbox;

pub const NO_SANDBOX: &str = "This agent runs its own tools, and this computer has no sandbox for them, so the bridge does not start it. Use an agent with kind acp, claude, or codex here, or run the bridge under WSL2 on Windows.";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The program and the arguments that start `program` with `args` inside `walls`, with
/// no shell. On Linux the forwarder inside the sandbox is the way to the proxy.
pub fn launch(
    walls: &Walls,
    cwd: &Path,
    program: &Path,
    args: &[String],
) -> Result<(PathBuf, Vec<OsString>), String> {
    match &walls.tool {
        Sandbox::Bwrap(bwrap) => Ok((bwrap.clone(), bwrap_args(walls, cwd, program, args)?)),
        Sandbox::Seatbelt => {
            let mut all = vec![OsString::from("-p"), bytes_arg(seatbelt_profile(walls)?)];
            all.extend(["--".into(), program.into()]);
            all.extend(args.iter().map(OsString::from));
            Ok((PathBuf::from(SANDBOX_EXEC), all))
        }
        Sandbox::None => Err(NO_SANDBOX.into()),
    }
}

/// The mounts of the run, its own namespaces, then the forwarder, which starts the
/// harness and its arguments, each one as it is.
pub fn bwrap_args(
    walls: &Walls,
    cwd: &Path,
    program: &Path,
    args: &[String],
) -> Result<Vec<OsString>, String> {
    let Some(ProxyEnd::Socket { socket, forwarder }) = &walls.proxy else {
        return Err("The proxy of the agent did not start.".into());
    };
    let mut out = mount_args(walls);
    out.extend(["--chdir".into(), cwd.into()]);
    for word in ["--unshare-all", "--die-with-parent", "--new-session", "--"] {
        out.push(word.into());
    }
    out.extend([
        forwarder.into(),
        FORWARD_FLAG.into(),
        socket.into(),
        ports_arg(&walls.local_ports).into(),
        EXEC_FLAG.into(),
        program.into(),
    ]);
    out.extend(args.iter().map(OsString::from));
    Ok(out)
}
