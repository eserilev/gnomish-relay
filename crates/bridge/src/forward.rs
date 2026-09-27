//! The forwarder inside `bwrap` (SPEC.md 6.6.4). The sandbox has its own network with
//! only a loopback, so a command reaches the proxy of the bridge through a port there,
//! which this program relays to the Unix socket of the proxy. Each port of `local_ports`
//! is a port there too: a plain TCP client such as `psql` connects to it, and the
//! forwarder asks the proxy for `localhost:<port>`.

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::{Path, PathBuf};
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
/// The answer of the proxy to a request for a local port is a few dozen bytes.
#[cfg(unix)]
const MAX_ANSWER: usize = 8192;

/// Where the forwarder listens, and where it relays to.
#[cfg(unix)]
pub struct Forward<'a> {
    pub socket: &'a Path,
    /// The port of the proxy inside the sandbox.
    pub port: u16,
    pub local_ports: &'a [u16],
}

/// The ports of `local_ports` as one argument: `5432,3000`, or empty.
pub fn ports_arg(ports: &[u16]) -> String {
    let words: Vec<String> = ports.iter().map(u16::to_string).collect();
    words.join(",")
}

pub fn parse_ports(arg: &str) -> Result<Vec<u16>, String> {
    if arg.is_empty() {
        return Ok(Vec::new());
    }
    arg.split(',')
        .map(|p| p.parse().map_err(|_| format!("bad port {p:?}")))
        .collect()
}

/// Relays the ports, runs `shell -c command`, and gives its exit status.
#[cfg(unix)]
pub fn run_forwarder(forward: &Forward, shell: &Path, command: &str) -> i32 {
    let ran = listen(forward).and_then(|()| {
        std::process::Command::new(shell)
            .args(["-c", command])
            .status()
            .map_err(|e| format!("cannot start {}: {e}", shell.display()))
    });
    match ran {
        Ok(status) => exit_code(status),
        Err(e) => {
            eprintln!("gnomish-relay sandbox: {e}");
            126
        }
    }
}

/// Starts a thread for the port of the proxy and one for each local port.
#[cfg(unix)]
fn listen(forward: &Forward) -> Result<(), String> {
    let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, forward.port))
        .map_err(|e| format!("no port for the proxy: {e}"))?;
    let socket = forward.socket.to_owned();
    std::thread::spawn(move || relay_all(&proxy, &socket, None));
    for &port in forward.local_ports {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|e| format!("local port {port} is taken in the sandbox: {e}"))?;
        let socket = forward.socket.to_owned();
        std::thread::spawn(move || relay_all(&v4, &socket, Some(port)));
        // A computer with IPv6 off has no `::1`. Clients of `localhost` then use IPv4.
        if let Ok(v6) = TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))) {
            let socket = forward.socket.to_owned();
            std::thread::spawn(move || relay_all(&v6, &socket, Some(port)));
        }
    }
    Ok(())
}

/// Relays each client to the proxy. With a local port, it asks for it first.
#[cfg(unix)]
fn relay_all(listener: &TcpListener, socket: &Path, local: Option<u16>) {
    for client in listener.incoming().flatten() {
        let socket: PathBuf = socket.to_owned();
        std::thread::spawn(move || {
            let Ok(mut proxy) = UnixStream::connect(&socket) else {
                return;
            };
            if let Some(port) = local
                && !ask_for_local(&mut proxy, port)
            {
                return;
            }
            let _ = relay(client, proxy, IDLE);
        });
    }
}

/// Sends `CONNECT localhost:<port>` and reads the head of the answer, byte by byte, so
/// no byte of the tunnel leaves the stream. The proxy checks the port, never this code.
#[cfg(unix)]
fn ask_for_local(proxy: &mut UnixStream, port: u16) -> bool {
    if write!(proxy, "CONNECT localhost:{port} HTTP/1.1\r\n\r\n").is_err() {
        return false;
    }
    let _ = proxy.set_read_timeout(Some(Duration::from_secs(20)));
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < MAX_ANSWER {
        match proxy.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return false,
        }
    }
    let _ = proxy.set_read_timeout(None);
    head.starts_with(b"HTTP/1.1 200 ")
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
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// A test that runs at the same time can take the free port before the forwarder
    /// binds it, which gives 126. Then the run goes again on a new port.
    fn on_a_free_port(run: impl Fn(u16) -> i32) -> i32 {
        (0..5)
            .map(|_| run(free_port()))
            .find(|code| *code != 126)
            .unwrap_or(126)
    }

    fn only_proxy(socket: &Path, port: u16) -> Forward<'_> {
        Forward {
            socket,
            port,
            local_ports: &[],
        }
    }

    /// A proxy that answers the first line of each request with `answer`, and then
    /// sends back `got <first line>`.
    fn fake_proxy(path: &Path, answer: &'static str) {
        let proxy = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            for stream in proxy.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let mut blank = String::new();
                    reader.read_line(&mut blank).unwrap();
                    let mut stream = stream;
                    write!(stream, "{answer}got {line}").unwrap();
                });
            }
        });
    }

    #[test]
    fn the_command_reaches_the_socket_through_the_port_and_keeps_its_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy");
        let proxy = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = proxy.accept().unwrap();
            let mut got = [0u8; 4];
            stream.read_exact(&mut got).unwrap();
            stream.write_all(b"pong").unwrap();
        });
        let out = dir.path().join("out");

        let code = on_a_free_port(|port| {
            let command = format!(
                "exec 3<>/dev/tcp/127.0.0.1/{port}; printf ping >&3; head -c 4 <&3 > '{}'; exit 7",
                out.display()
            );
            run_forwarder(&only_proxy(&path, port), Path::new("/bin/bash"), &command)
        });

        assert_eq!(code, 7);
        assert_eq!(std::fs::read_to_string(out).unwrap(), "pong");
    }

    #[test]
    fn a_local_port_asks_the_proxy_for_localhost_and_then_relays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy");
        fake_proxy(&path, "HTTP/1.1 200 Connection established\r\n\r\n");
        let out = dir.path().join("out");
        let local = free_port();

        let code = on_a_free_port(|port| {
            let forward = Forward {
                socket: &path,
                port,
                local_ports: &[local],
            };
            let command = format!(
                "exec 3<>/dev/tcp/127.0.0.1/{local}; head -n 1 <&3 > '{}'",
                out.display()
            );
            run_forwarder(&forward, Path::new("/bin/bash"), &command)
        });

        assert_eq!(code, 0);
        let got = std::fs::read_to_string(out).unwrap();
        assert_eq!(got, format!("got CONNECT localhost:{local} HTTP/1.1\r\n"));
    }

    #[test]
    fn a_local_port_that_the_proxy_refuses_closes_the_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy");
        fake_proxy(&path, "HTTP/1.1 403 Forbidden\r\n\r\n");
        let out = dir.path().join("out");
        let local = free_port();

        let code = on_a_free_port(|port| {
            let forward = Forward {
                socket: &path,
                port,
                local_ports: &[local],
            };
            let command = format!(
                "exec 3<>/dev/tcp/127.0.0.1/{local}; cat <&3 > '{}'",
                out.display()
            );
            run_forwarder(&forward, Path::new("/bin/bash"), &command)
        });

        assert_eq!(code, 0);
        assert_eq!(std::fs::read_to_string(out).unwrap(), "");
    }

    #[test]
    fn a_command_that_a_signal_stops_gives_128_plus_the_signal() {
        let code = on_a_free_port(|port| {
            let forward = only_proxy(Path::new("/nowhere"), port);
            run_forwarder(&forward, Path::new("/bin/sh"), "kill -9 $$")
        });

        assert_eq!(code, 137);
    }

    #[test]
    fn a_port_that_is_taken_gives_126() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let forward = Forward {
            socket: Path::new("/nowhere"),
            port: free_port(),
            local_ports: &[port],
        };

        assert_eq!(
            run_forwarder(
                &only_proxy(Path::new("/nowhere"), port),
                Path::new("/bin/sh"),
                "true"
            ),
            126
        );
        assert_eq!(run_forwarder(&forward, Path::new("/bin/sh"), "true"), 126);
    }

    #[test]
    fn the_local_ports_go_in_one_argument_and_come_back() {
        assert_eq!(ports_arg(&[5432, 3000]), "5432,3000");
        assert_eq!(ports_arg(&[]), "");
        assert_eq!(parse_ports("5432,3000"), Ok(vec![5432, 3000]));
        assert_eq!(parse_ports(""), Ok(vec![]));
        assert!(parse_ports("5432,x").is_err());
        assert!(parse_ports("70000").is_err());
    }
}
