//! The forwarder inside `bwrap` (SPEC.md 6.6.4). The sandbox has its own network with
//! only a loopback, so a command reaches the proxy of the bridge through a port there,
//! which this program relays to the Unix socket of the proxy.

#[cfg(unix)]
use std::net::{Ipv4Addr, TcpListener};
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::ExitStatus;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use crate::pipe::relay;

/// The flag of this program that runs the forwarder and then one command.
pub const FORWARD_FLAG: &str = "--sandbox-forward";
/// Each command has a network of its own, so no other program holds this port.
pub const INNER_PORT: u16 = 3128;
/// The proxy ends a quiet connection itself, so this one only stops a stuck relay.
#[cfg(unix)]
const IDLE: Duration = Duration::from_mins(10);

/// Relays `port` to `socket`, runs `shell -c command`, and gives its exit status.
#[cfg(unix)]
pub fn run_forwarder(socket: &Path, port: u16, shell: &Path, command: &str) -> i32 {
    match forward(socket, port, shell, command) {
        Ok(status) => exit_code(status),
        Err(e) => {
            eprintln!("gnomish-relay sandbox: {e}");
            126
        }
    }
}

#[cfg(unix)]
fn forward(socket: &Path, port: u16, shell: &Path, command: &str) -> Result<ExitStatus, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("no port for the proxy: {e}"))?;
    let socket = socket.to_owned();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let socket = socket.clone();
            std::thread::spawn(move || {
                if let Ok(proxy) = std::os::unix::net::UnixStream::connect(&socket) {
                    let _ = relay(client, proxy, IDLE);
                }
            });
        }
    });
    std::process::Command::new(shell)
        .args(["-c", command])
        .status()
        .map_err(|e| format!("cannot start {}: {e}", shell.display()))
}

/// A shell gives 128 plus the signal for a command that a signal stopped.
#[cfg(unix)]
fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn the_command_reaches_the_socket_through_the_port_and_keeps_its_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy");
        let proxy = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = proxy.accept().unwrap();
            let mut got = [0u8; 4];
            stream.read_exact(&mut got).unwrap();
            stream.write_all(b"pong").unwrap();
        });
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let out = dir.path().join("out");
        let command = format!(
            "exec 3<>/dev/tcp/127.0.0.1/{port}; printf ping >&3; head -c 4 <&3 > '{}'; exit 7",
            out.display()
        );

        let code = run_forwarder(&path, port, Path::new("/bin/bash"), &command);

        assert_eq!(code, 7);
        assert_eq!(std::fs::read_to_string(out).unwrap(), "pong");
    }

    #[test]
    fn a_command_that_a_signal_stops_gives_128_plus_the_signal() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();

        let code = run_forwarder(
            Path::new("/nowhere"),
            port,
            Path::new("/bin/sh"),
            "kill -9 $$",
        );

        assert_eq!(code, 137);
    }

    #[test]
    fn a_port_that_is_taken_gives_126() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();

        let code = run_forwarder(Path::new("/nowhere"), port, Path::new("/bin/sh"), "true");

        assert_eq!(code, 126);
    }
}
