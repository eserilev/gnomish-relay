//! The `bwrap` arguments of the wall of the agent (SPEC.md 6.6.4). The input is
//! `bind NUL read-only NUL socket NUL arg NUL arg ...`.
//!
//! The target reads the arguments as `bwrap` does, each option with its own number of
//! values, so a path such as `--proc` or `--` stays a value.
//!
//! - The wall always has its own network and processes, a new `/proc`, and no shared
//!   network.
//! - Each private folder comes before every bind back into it.
//! - Each startup file is bound read-only onto itself, and each socket is covered.
//! - The agent and its arguments come last, each one as it is, after the forwarder.
#![no_main]

use std::ffi::OsString;
use std::path::PathBuf;

use bridge::agent_wall::{EXEC_FLAG, INNER_SOCKET, WallSpec, wall_args};
use bridge::forward::FORWARD_FLAG;
use libfuzzer_sys::fuzz_target;

#[cfg(unix)]
fn path_of(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// The number of values of each option that the wall uses.
fn values(option: &OsString) -> usize {
    match option.to_str() {
        Some("--dev-bind" | "--bind" | "--ro-bind") => 2,
        Some("--tmpfs" | "--proc") => 1,
        Some("--unshare-net" | "--unshare-pid" | "--die-with-parent" | "--new-session") => 0,
        other => panic!("an option that the wall never uses: {other:?}"),
    }
}

/// The options with their values, and the command after `--`.
fn read(args: &[OsString]) -> (Vec<Vec<OsString>>, Vec<OsString>) {
    let mut options = Vec::new();
    let mut i = 0;
    while args[i] != "--" {
        let n = values(&args[i]);
        options.push(args[i..=i + n].to_vec());
        i += n + 1;
    }
    (options, args[i + 1..].to_vec())
}

fn os(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn at(options: &[Vec<OsString>], option: &[OsString]) -> Option<usize> {
    options.iter().position(|o| o[..] == option[..])
}

fuzz_target!(|data: &[u8]| {
    let mut fields = data.split(|&b| b == 0);
    let bind = path_of(fields.next().unwrap_or_default());
    let read_only = path_of(fields.next().unwrap_or_default());
    let socket = path_of(fields.next().unwrap_or_default());
    let agent_args: Vec<String> = fields
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    let spec = WallSpec {
        bwrap: PathBuf::from("/usr/bin/bwrap"),
        forwarder: PathBuf::from("/usr/bin/gnomish-relay"),
        socket: PathBuf::from("/data/sandbox/agent-1.sock"),
        binds: vec![bind, PathBuf::from("/tmp/gnomish-relay-run-1")],
        read_only: vec![read_only.clone()],
        sockets: vec![socket.clone()],
        local_ports: vec![5432],
        windows_drives: Vec::new(),
    };

    let (options, command) = read(&wall_args(
        &spec,
        &PathBuf::from("/usr/bin/agent"),
        &agent_args,
    ));

    for flag in [
        "--unshare-net",
        "--unshare-pid",
        "--die-with-parent",
        "--new-session",
    ] {
        assert!(at(&options, &os(&[flag])).is_some(), "{flag}");
    }
    assert!(at(&options, &os(&["--proc", "/proc"])).is_some());
    for option in options.iter().filter(|o| o[0] == "--bind") {
        let from = std::path::Path::new(&option[1]);
        let whole = ["/run", "/tmp", "/var/tmp", "/dev/shm"]
            .iter()
            .any(|f| from == std::path::Path::new(f));
        assert!(!whole, "a bind back of a whole private folder: {option:?}");
    }
    let tmp = at(&options, &os(&["--tmpfs", "/tmp"])).unwrap();
    let run = "/tmp/gnomish-relay-run-1";
    let back = at(&options, &os(&["--bind", run, run])).unwrap();
    assert!(tmp < back);
    let socket_bind = at(
        &options,
        &os(&["--bind", "/data/sandbox/agent-1.sock", INNER_SOCKET]),
    );
    assert!(tmp < socket_bind.unwrap());
    let ro: Vec<OsString> = vec![
        "--ro-bind".into(),
        read_only.clone().into(),
        read_only.into(),
    ];
    assert!(at(&options, &ro).is_some());
    let covered: Vec<OsString> = vec!["--ro-bind".into(), "/dev/null".into(), socket.into()];
    assert!(at(&options, &covered).is_some());
    let mut expected = os(&[
        "/usr/bin/gnomish-relay",
        FORWARD_FLAG,
        INNER_SOCKET,
        "5432",
        EXEC_FLAG,
        "/usr/bin/agent",
    ]);
    expected.extend(agent_args.iter().map(OsString::from));
    assert_eq!(command, expected);
});
