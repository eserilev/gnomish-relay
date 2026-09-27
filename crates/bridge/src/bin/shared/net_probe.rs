//! The network probes of the fake agents, for the tests of the wall of the agent
//! (SPEC.md 6.6.4). A prompt `net direct=127.0.0.1:4000 proxy=allowed.test:443` runs
//! each probe and gives one word for each, in the same order. It is test code.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

/// `None` for a prompt that is not a probe.
pub fn answer(prompt: &str) -> Option<String> {
    let probes = prompt.strip_prefix("net ")?;
    let words: Vec<String> = probes.split_whitespace().map(probe).collect();
    Some(words.join(" "))
}

fn probe(word: &str) -> String {
    let (kind, target) = word.split_once('=').unwrap_or((word, ""));
    let result = match kind {
        "direct" => Some(direct(target)),
        "proxy" => proxy(target),
        "local" => local(target),
        "serve" => serve(target),
        "proc" => Some(yes_no(
            std::path::Path::new(&format!("/proc/{target}")).exists(),
        )),
        "write" => Some(ok_fail(
            std::fs::write(target, "changed by the agent").is_ok(),
        )),
        "sock" => Some(ok_fail(unix_connect(target))),
        _ => None,
    };
    format!("{kind}={}", result.unwrap_or_else(|| "bad".into()))
}

fn yes_no(yes: bool) -> String {
    if yes { "yes" } else { "no" }.into()
}

fn ok_fail(ok: bool) -> String {
    if ok { "ok" } else { "fail" }.into()
}

#[cfg(unix)]
fn unix_connect(path: &str) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
fn unix_connect(_path: &str) -> bool {
    false
}

fn connect(addr: &str) -> Option<TcpStream> {
    let addr: SocketAddr = addr.parse().ok()?;
    let stream = TcpStream::connect_timeout(&addr, WAIT).ok()?;
    stream.set_read_timeout(Some(WAIT)).ok()?;
    Some(stream)
}

fn direct(addr: &str) -> String {
    ok_fail(connect(addr).is_some())
}

/// The status of a `CONNECT` through `HTTPS_PROXY`, and with a 200 the first line of
/// the answer of the host to a GET.
fn proxy(target: &str) -> Option<String> {
    let url = std::env::var("HTTPS_PROXY").ok()?;
    let proxy = url.strip_prefix("http://")?;
    let Some(mut stream) = connect(proxy) else {
        return Some("noproxy".into());
    };
    write!(stream, "CONNECT {target} HTTP/1.1\r\n\r\n").ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut status = String::new();
    reader.read_line(&mut status).ok()?;
    let code = status.split_whitespace().nth(1)?.to_owned();
    if code != "200" {
        return Some(code);
    }
    let mut blank = String::new();
    reader.read_line(&mut blank).ok()?;
    write!(stream, "GET /agent HTTP/1.1\r\n\r\n").ok()?;
    let mut body = String::new();
    let _ = reader.read_to_string(&mut body);
    let last = body.lines().last().unwrap_or("").replace(' ', "_");
    Some(format!("200:{last}"))
}

/// A line from a server on a port of `local_ports`, as `localhost`.
fn local(port: &str) -> Option<String> {
    let Some(mut stream) = connect(&format!("127.0.0.1:{port}")) else {
        return Some("fail".into());
    };
    writeln!(stream, "ping").ok()?;
    let mut line = String::new();
    let _ = BufReader::new(stream).read_line(&mut line);
    if line.is_empty() {
        return Some("closed".into());
    }
    Some(line.trim().replace(' ', "_"))
}

/// A server of the agent on the loopback of its wall, and a client of it.
fn serve(port: &str) -> Option<String> {
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).ok()?;
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.write_all(b"served\n");
        }
    });
    let stream = connect(&format!("127.0.0.1:{port}"))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    Some(line.trim().to_owned())
}
