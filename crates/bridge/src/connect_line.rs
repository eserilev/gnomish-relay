//! The head of an HTTP `CONNECT` request to the proxy (SPEC.md 6.6.4): where it ends,
//! and the answer to each refusal. The target check itself is `protocol::connect` (S35).

use protocol::connect::Refusal;

/// A longer head is refused. A real one is a few hundred bytes.
pub const MAX_HEAD: usize = 8192;

/// The end of the head: the index just after the first empty line.
pub fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
}

pub fn status(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NotConnect => "405 Method Not Allowed",
        Refusal::NotHttp | Refusal::NoPort => "400 Bad Request",
        Refusal::IpAddress
        | Refusal::BadHost
        | Refusal::NotListed
        | Refusal::BadPort
        | Refusal::LocalPortClosed
        | Refusal::LocalPortDangerous => "403 Forbidden",
    }
}

pub fn reason(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NotConnect => "the proxy takes only CONNECT, so only https works",
        Refusal::NotHttp => "not an HTTP request",
        Refusal::NoPort => "the target has no port",
        Refusal::IpAddress => "an IP address, not a host name",
        Refusal::BadHost => "not a host name that the proxy allows",
        Refusal::NotListed => {
            "not on the allow list; add it to config.toml: allow_hosts in [sandbox] for commands, agent_hosts in the agent entry for the agent"
        }
        Refusal::BadPort => "the proxy takes only ports 443 and 80",
        Refusal::LocalPortClosed => {
            "a port of this computer that is not open; add it to local_ports in [sandbox] of config.toml"
        }
        Refusal::LocalPortDangerous => {
            "a port that runs any code (Docker or the debugger of a browser), which the proxy never opens"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_ends_after_the_first_empty_line() {
        assert_eq!(head_end(b"CONNECT a.b:443 HTTP/1.1\r\n\r\nTLS"), Some(28));
        assert_eq!(head_end(b"CONNECT a.b:443 HTTP/1.1\r\n"), None);
    }

    #[test]
    fn every_refusal_has_a_status_and_a_reason() {
        for refusal in [
            Refusal::NotConnect,
            Refusal::NotHttp,
            Refusal::NoPort,
            Refusal::IpAddress,
            Refusal::BadHost,
            Refusal::NotListed,
            Refusal::BadPort,
            Refusal::LocalPortClosed,
            Refusal::LocalPortDangerous,
        ] {
            assert!(!reason(refusal).is_empty());
            assert!(status(refusal).starts_with('4'));
        }
        assert_eq!(status(Refusal::NotConnect), "405 Method Not Allowed");
        assert_eq!(status(Refusal::NoPort), "400 Bad Request");
        assert_eq!(status(Refusal::LocalPortClosed), "403 Forbidden");
    }
}
