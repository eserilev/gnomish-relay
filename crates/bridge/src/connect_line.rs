//! The head of an HTTP `CONNECT` request, as a command sends it to the proxy of the
//! sandbox (SPEC.md 6.6.4). The command is untrusted, so every input gives a target or
//! a defined refusal.

use std::net::IpAddr;

use crate::allow_hosts::check_host_name;

/// A longer head is refused. A real one is a few hundred bytes.
pub const MAX_HEAD: usize = 8192;

#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    /// In lower case, and a good name by `check_host_name`.
    pub host: String,
    pub port: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BadRequest {
    /// Any method but `CONNECT`, for example a plain `GET http://...`.
    NotConnect,
    NotHttp,
    NoPort,
    IpAddress,
    BadHost,
}

impl BadRequest {
    pub fn reason(&self) -> &'static str {
        match self {
            BadRequest::NotConnect => "the proxy takes only CONNECT, so only https works",
            BadRequest::NotHttp => "not an HTTP request",
            BadRequest::NoPort => "the target has no port",
            BadRequest::IpAddress => "an IP address, not a host name",
            BadRequest::BadHost => "not a host name that the proxy allows",
        }
    }
}

/// The end of the head: the index just after the first empty line.
pub fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
}

pub fn parse_connect(head: &[u8]) -> Result<Target, BadRequest> {
    let line_end = head
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or(BadRequest::NotHttp)?;
    let line = std::str::from_utf8(&head[..line_end]).map_err(|_| BadRequest::NotHttp)?;
    let parts: Vec<&str> = line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(BadRequest::NotHttp);
    };
    if !version.starts_with("HTTP/1.") {
        return Err(BadRequest::NotHttp);
    }
    if method != "CONNECT" {
        return Err(BadRequest::NotConnect);
    }
    parse_target(target)
}

fn parse_target(target: &str) -> Result<Target, BadRequest> {
    if target.starts_with('[') {
        return Err(BadRequest::IpAddress);
    }
    let (host, port) = target.rsplit_once(':').ok_or(BadRequest::NoPort)?;
    let port: u16 = port.parse().map_err(|_| BadRequest::NoPort)?;
    if looks_like_ip(host) {
        return Err(BadRequest::IpAddress);
    }
    check_host_name(host).map_err(|_| BadRequest::BadHost)?;
    Ok(Target {
        host: host.to_ascii_lowercase(),
        port,
    })
}

/// Also the short and hex forms that the resolver of the OS takes, such as `127.1`.
fn looks_like_ip(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or_default();
    host.parse::<IpAddr>().is_ok() || last.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Target, BadRequest> {
        parse_connect(text.as_bytes())
    }

    #[test]
    fn a_connect_request_gives_its_host_in_lower_case_and_its_port() {
        let head = "CONNECT Index.Crates.IO:443 HTTP/1.1\r\nHost: index.crates.io:443\r\n\r\n";

        assert_eq!(
            parse(head),
            Ok(Target {
                host: "index.crates.io".into(),
                port: 443
            })
        );
    }

    #[test]
    fn a_plain_http_request_is_not_a_connect() {
        let head = "GET http://example.com/ HTTP/1.1\r\n\r\n";

        assert_eq!(parse(head), Err(BadRequest::NotConnect));
    }

    #[test]
    fn an_ip_address_is_refused_in_every_form() {
        for target in [
            "127.0.0.1:443",
            "[::1]:443",
            "[::ffff:127.0.0.1]:443",
            "127.1:443",
            "0x7f000001:443",
            "2130706433:443",
            "a.0x7f:443",
        ] {
            let head = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
            assert_eq!(parse(&head), Err(BadRequest::IpAddress), "{target}");
        }
    }

    #[test]
    fn a_bad_host_or_port_is_refused() {
        for (target, error) in [
            ("localhost:443", BadRequest::BadHost),
            ("a.localhost:443", BadRequest::BadHost),
            ("ex ample.com:443", BadRequest::NotHttp),
            ("example.com", BadRequest::NoPort),
            ("example.com:", BadRequest::NoPort),
            ("example.com:99999", BadRequest::NoPort),
            ("user@example.com:443", BadRequest::BadHost),
            ("example.com.:443", BadRequest::BadHost),
        ] {
            let head = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
            assert_eq!(parse(&head), Err(error), "{target}");
        }
    }

    #[test]
    fn a_head_that_is_not_http_is_refused() {
        for head in [
            "",
            "CONNECT example.com:443\r\n\r\n",
            "CONNECT example.com:443 SPDY/3\r\n\r\n",
            "CONNECT example.com:443 HTTP/1.1",
            "\u{16}\u{3}\u{1}",
        ] {
            assert_eq!(parse(head), Err(BadRequest::NotHttp), "{head:?}");
        }
        assert_eq!(parse_connect(b"\xff\xfe x y\r\n"), Err(BadRequest::NotHttp));
    }

    #[test]
    fn the_head_ends_after_the_first_empty_line() {
        assert_eq!(head_end(b"CONNECT a.b:443 HTTP/1.1\r\n\r\nTLS"), Some(28));
        assert_eq!(head_end(b"CONNECT a.b:443 HTTP/1.1\r\n"), None);
    }

    #[test]
    fn every_refusal_has_a_reason() {
        for error in [
            BadRequest::NotConnect,
            BadRequest::NotHttp,
            BadRequest::NoPort,
            BadRequest::IpAddress,
            BadRequest::BadHost,
        ] {
            assert!(!error.reason().is_empty());
        }
    }
}
