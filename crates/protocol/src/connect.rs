//! The target of a `CONNECT` request to the proxy of the bridge. The agent or a command
//! writes the request, so it is untrusted, and every input gives a target or a defined
//! refusal. See `SPEC.md` 6.6.4 and 14.1, S35.

use crate::hosts::{good_host_name, host_allowed};
use crate::search::{equal_run, has_byte};

const SPACE: u8 = b' ';
const COLON: u8 = b':';
const DOT: u8 = b'.';
const CR: u8 = b'\r';
const LF: u8 = b'\n';
const CONNECT: [u8; 7] = *b"CONNECT";
const HTTP_1: [u8; 7] = *b"HTTP/1.";
const LOCALHOST: [u8; 9] = *b"localhost";
/// Docker over TCP, and the debug port of a browser: each one runs any code.
const DANGEROUS_PORTS: [u16; 3] = [2375, 2376, 9222];

/// Which hosts a remote target may name.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Mode {
    /// Only a host of the list: the commands, and the agent in `strict` mode.
    Listed,
    /// Any good host name. The bridge then connects only to a public address (S34).
    Public,
}

#[derive(PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Target {
    /// The host in lower case, and port 443 or 80.
    Remote { host: Vec<u8>, port: u16 },
    /// A port of `local_ports` on the loopback of this computer.
    Local { port: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Refusal {
    NotHttp,
    /// Any method but `CONNECT`, for example a plain `GET http://...`.
    NotConnect,
    NoPort,
    IpAddress,
    BadHost,
    NotListed,
    /// A remote port other than 443 and 80.
    BadPort,
    LocalPortClosed,
    LocalPortDangerous,
}

/// The index of the first CR LF, or `head.len()` with none.
fn line_end(head: &[u8]) -> usize {
    let mut at = 0;
    let mut found = false;
    while !found && at + 1 < head.len() {
        found = head[at] == CR && head[at + 1] == LF;
        if !found {
            at += 1;
        }
    }
    if found { at } else { head.len() }
}

/// The index of the first space in `line[from..end]`, or `end`.
fn next_space(line: &[u8], from: usize, end: usize) -> usize {
    let mut at = from;
    while at < end && line[at] != SPACE {
        at += 1;
    }
    at
}

/// `HTTP/1.` and one digit.
fn is_version(bytes: &[u8], start: usize, end: usize) -> bool {
    end - start == HTTP_1.len() + 1
        && equal_run(bytes, start, &HTTP_1, HTTP_1.len())
        && is_digit(bytes[end - 1])
}

fn equals_at(bytes: &[u8], start: usize, end: usize, word: &[u8]) -> bool {
    end - start == word.len() && equal_run(bytes, start, word, word.len())
}

fn copy_range(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn lower(b: u8) -> u8 {
    if b'A' <= b && b <= b'Z' { b + 32 } else { b }
}

fn lower_range(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        out.push(lower(bytes[i]));
        i += 1;
    }
    out
}

/// The index of the last `:` in `bytes[start..end]`, or `end` with none.
fn last_colon(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut found = end;
    let mut i = start;
    while i < end {
        if bytes[i] == COLON {
            found = i;
        }
        i += 1;
    }
    found
}

fn is_digit(b: u8) -> bool {
    b'0' <= b && b <= b'9'
}

/// 1 to 5 decimal digits, at most 65535. No sign, no space.
fn parse_port(bytes: &[u8], start: usize, end: usize) -> Option<u16> {
    if end <= start || end - start > 5 {
        return None;
    }
    let mut value: u32 = 0;
    let mut ok = true;
    let mut i = start;
    while ok && i < end {
        ok = is_digit(bytes[i]);
        if ok {
            value = value * 10 + (bytes[i] - b'0') as u32;
        }
        i += 1;
    }
    if !ok || value > 65535 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)] // at most 65535, checked above
    Some(value as u16)
}

/// A `:`, or a last label that starts with a digit, as in `127.1` or `0x7f000001`.
fn looks_like_ip(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut last = start;
    let mut colon = false;
    let mut i = start;
    while i < end {
        if bytes[i] == DOT {
            last = i + 1;
        }
        if bytes[i] == COLON {
            colon = true;
        }
        i += 1;
    }
    colon || (last < end && is_digit(bytes[last]))
}

fn is_listed_port(ports: &[u16], port: u16) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < ports.len() {
        found = ports[i] == port;
        i += 1;
    }
    found
}

fn local_target(ports: &[u16], port: u16) -> Result<Target, Refusal> {
    if is_listed_port(&DANGEROUS_PORTS, port) {
        return Err(Refusal::LocalPortDangerous);
    }
    if !is_listed_port(ports, port) {
        return Err(Refusal::LocalPortClosed);
    }
    Ok(Target::Local { port })
}

fn remote_target(mode: Mode, list: &[Vec<u8>], host: &[u8], port: u16) -> Result<Target, Refusal> {
    if !good_host_name(host) {
        return Err(Refusal::BadHost);
    }
    if port != 443 && port != 80 {
        return Err(Refusal::BadPort);
    }
    if mode == Mode::Listed && !host_allowed(list, host) {
        return Err(Refusal::NotListed);
    }
    Ok(Target::Remote {
        host: lower_range(host, 0, host.len()),
        port,
    })
}

/// `line[start..end]` is the target of the request, as in `example.com:443`.
fn check_host_port(
    mode: Mode,
    list: &[Vec<u8>],
    ports: &[u16],
    line: &[u8],
    start: usize,
    end: usize,
) -> Result<Target, Refusal> {
    if start < end && line[start] == b'[' {
        return Err(Refusal::IpAddress);
    }
    let colon = last_colon(line, start, end);
    if colon == end {
        return Err(Refusal::NoPort);
    }
    let Some(port) = parse_port(line, colon + 1, end) else {
        return Err(Refusal::NoPort);
    };
    if equals_at(
        &lower_range(line, start, colon),
        0,
        colon - start,
        &LOCALHOST,
    ) {
        return local_target(ports, port);
    }
    if looks_like_ip(line, start, colon) {
        return Err(Refusal::IpAddress);
    }
    remote_target(mode, list, &copy_range(line, start, colon), port)
}

/// The first line must be `CONNECT <host>:<port> HTTP/1.<digit>`, with one space between
/// the three parts.
///
/// # Errors
///
/// A `Refusal` for a head that is not such a line, or a target that `mode`, `list`, and
/// `ports` do not allow.
pub fn check_target(
    mode: Mode,
    list: &[Vec<u8>],
    ports: &[u16],
    head: &[u8],
) -> Result<Target, Refusal> {
    let end = line_end(head);
    if end == head.len() {
        return Err(Refusal::NotHttp);
    }
    let first = next_space(head, 0, end);
    if first == end {
        return Err(Refusal::NotHttp);
    }
    let second = next_space(head, first + 1, end);
    if second == end || has_byte(&copy_range(head, second + 1, end), SPACE) {
        return Err(Refusal::NotHttp);
    }
    if !is_version(head, second + 1, end) {
        return Err(Refusal::NotHttp);
    }
    if !equals_at(head, 0, first, &CONNECT) {
        return Err(Refusal::NotConnect);
    }
    check_host_port(mode, list, ports, head, first + 1, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(text: &str) -> Result<Target, Refusal> {
        let list = vec![b"index.crates.io".to_vec(), b"github.com".to_vec()];
        check_target(Mode::Listed, &list, &[5432], text.as_bytes())
    }

    fn public(text: &str) -> Result<Target, Refusal> {
        check_target(Mode::Public, &[], &[5432, 2375], text.as_bytes())
    }

    fn head(target: &str) -> String {
        format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n")
    }

    #[allow(clippy::unnecessary_wraps)] // compared with a result
    fn remote(host: &str, port: u16) -> Result<Target, Refusal> {
        Ok(Target::Remote {
            host: host.as_bytes().to_vec(),
            port,
        })
    }

    #[test]
    fn a_listed_host_gives_its_name_in_lower_case_and_its_port() {
        assert_eq!(
            listed(&head("Index.Crates.IO:443")),
            remote("index.crates.io", 443)
        );
        assert_eq!(listed(&head("github.com:80")), remote("github.com", 80));
    }

    #[test]
    fn a_host_that_is_not_listed_is_refused_only_in_the_listed_mode() {
        assert_eq!(listed(&head("example.com:443")), Err(Refusal::NotListed));
        assert_eq!(public(&head("Example.com:443")), remote("example.com", 443));
    }

    #[test]
    fn a_remote_port_other_than_443_and_80_is_refused() {
        for target in ["github.com:22", "github.com:8443", "github.com:0"] {
            assert_eq!(listed(&head(target)), Err(Refusal::BadPort), "{target}");
            assert_eq!(public(&head(target)), Err(Refusal::BadPort), "{target}");
        }
    }

    #[test]
    fn localhost_reaches_only_a_listed_local_port() {
        assert_eq!(
            listed(&head("localhost:5432")),
            Ok(Target::Local { port: 5432 })
        );
        assert_eq!(
            listed(&head("LocalHost:5432")),
            Ok(Target::Local { port: 5432 })
        );
        assert_eq!(
            listed(&head("localhost:443")),
            Err(Refusal::LocalPortClosed)
        );
        assert_eq!(
            public(&head("localhost:3000")),
            Err(Refusal::LocalPortClosed)
        );
    }

    #[test]
    fn a_dangerous_local_port_is_refused_even_when_listed() {
        for port in ["2375", "2376", "9222"] {
            let target = format!("localhost:{port}");
            assert_eq!(
                public(&head(&target)),
                Err(Refusal::LocalPortDangerous),
                "{port}"
            );
        }
    }

    #[test]
    fn a_plain_http_request_is_not_a_connect() {
        assert_eq!(
            public("GET http://example.com/ HTTP/1.1\r\n\r\n"),
            Err(Refusal::NotConnect)
        );
        assert_eq!(
            public("connect example.com:443 HTTP/1.1\r\n\r\n"),
            Err(Refusal::NotConnect)
        );
    }

    #[test]
    fn an_ip_address_is_refused_in_every_form() {
        for target in [
            "127.0.0.1:443",
            "[::1]:443",
            "[::ffff:127.0.0.1]:443",
            "::1:443",
            "127.1:443",
            "0x7f000001:443",
            "2130706433:443",
            "a.0x7f:443",
            "93.184.216.34:443",
        ] {
            assert_eq!(public(&head(target)), Err(Refusal::IpAddress), "{target}");
            assert_eq!(listed(&head(target)), Err(Refusal::IpAddress), "{target}");
        }
    }

    #[test]
    fn a_bad_host_or_port_is_refused() {
        for (target, error) in [
            ("a.localhost:443", Refusal::BadHost),
            ("ex ample.com:443", Refusal::NotHttp),
            ("example.com", Refusal::NoPort),
            ("example.com:", Refusal::NoPort),
            ("example.com:99999", Refusal::NoPort),
            ("example.com:65536", Refusal::NoPort),
            ("example.com:123456", Refusal::NoPort),
            ("example.com:+443", Refusal::NoPort),
            ("example.com:4a3", Refusal::NoPort),
            ("user@example.com:443", Refusal::BadHost),
            ("example.com.:443", Refusal::BadHost),
            (":443", Refusal::BadHost),
        ] {
            assert_eq!(public(&head(target)), Err(error), "{target}");
        }
    }

    #[test]
    fn a_head_that_is_not_http_is_refused() {
        for text in [
            "",
            "CONNECT",
            "CONNECT example.com:443\r\n\r\n",
            "CONNECT example.com:443 SPDY/3\r\n\r\n",
            "CONNECT example.com:443 HTTP/1.1",
            "CONNECT  example.com:443 HTTP/1.1\r\n",
            "CONNECT example.com:443 HTTP/1.1 x\r\n",
            "CONNECT example.com:443 HTTP/1.\r\n",
            "CONNECT example.com:443 HTTP/1.x\r\n",
            "CONNECT example.com:443 HTTP/1.11\r\n",
            "\u{16}\u{3}\u{1}",
        ] {
            assert_eq!(public(text), Err(Refusal::NotHttp), "{text:?}");
        }
        assert_eq!(
            check_target(Mode::Public, &[], &[], b"\xff\xfe x y\r\n"),
            Err(Refusal::NotHttp)
        );
    }

    #[test]
    fn the_first_line_ends_at_the_first_cr_lf() {
        assert_eq!(
            public("CONNECT example.com:443 HTTP/1.0\r\nCONNECT x HTTP/1.1\r\n\r\n"),
            remote("example.com", 443)
        );
        assert_eq!(
            public("CONNECT example.com:443 HTTP/1.1\rx\r\n"),
            Err(Refusal::NotHttp)
        );
    }
}
