//! S35 on the compiled code: the target of a `CONNECT` request to the proxy (SPEC.md
//! 6.6.4). The agent or a command writes it, so it is untrusted.
//!
//! - No input panics the check, in either mode.
//! - A target comes only from a first line `CONNECT <host>:<port> HTTP/1.<digit>`.
//! - A remote target is a good host name in lower case, never an IP address, on port 443
//!   or 80. In the listed mode it is on the list.
//! - A local target is `localhost` with a listed port that is not 2375, 2376, or 9222.
#![no_main]

use bridge::allow_hosts::check_host_name;
use bridge::connect_line::head_end;
use libfuzzer_sys::fuzz_target;
use protocol::connect::{Mode, Target, check_target};

const LIST: [&str; 2] = ["github.com", "index.crates.io"];
const PORTS: [u16; 3] = [5432, 3000, 2375];

/// The host and the port of the first line, as the model reads it.
fn first_line(data: &[u8]) -> Option<(String, u16)> {
    let end = data.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&data[..end]).ok()?;
    let parts: Vec<&str> = line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return None;
    };
    let digit = version.strip_prefix("HTTP/1.")?;
    assert_eq!(method, "CONNECT");
    assert!(digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit(), "{version}");
    let (host, port) = target.rsplit_once(':')?;
    assert!(!port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()));
    Some((host.to_owned(), port.parse().ok()?))
}

fn check_remote(mode: Mode, data: &[u8], host: &[u8], port: u16) {
    let host = std::str::from_utf8(host).unwrap();
    let (line_host, line_port) = first_line(data).expect("a target needs a good first line");
    assert_eq!(host, line_host.to_ascii_lowercase());
    assert_eq!(port, line_port);
    assert!(port == 443 || port == 80, "{port}");
    assert!(check_host_name(host).is_ok(), "{host}");
    assert!(host.parse::<std::net::IpAddr>().is_err(), "{host}");
    let last = host.rsplit('.').next().unwrap_or_default();
    assert!(last.starts_with(|c: char| c.is_ascii_alphabetic()), "{host}");
    if mode == Mode::Listed {
        assert!(LIST.contains(&host), "{host}");
    }
}

fn check_local(data: &[u8], port: u16) {
    let (line_host, line_port) = first_line(data).expect("a target needs a good first line");
    assert!(line_host.eq_ignore_ascii_case("localhost"), "{line_host}");
    assert_eq!(port, line_port);
    assert!(PORTS.contains(&port), "{port}");
    assert!(![2375, 2376, 9222].contains(&port), "{port}");
}

fuzz_target!(|data: &[u8]| {
    let _ = head_end(data);
    let list: Vec<Vec<u8>> = LIST.iter().map(|h| h.as_bytes().to_vec()).collect();
    for mode in [Mode::Listed, Mode::Public] {
        match check_target(mode, &list, &PORTS, data) {
            Ok(Target::Remote { host, port }) => check_remote(mode, data, &host, port),
            Ok(Target::Local { port }) => check_local(data, port),
            Err(_) => {}
        }
    }
});
