//! The host names that the proxy of the bridge takes, and its allow lists. See
//! `SPEC.md` 6.6.4 and 14.1, S33.

use crate::ascii::{bytes_equal, to_lower};

const MAX_NAME: usize = 253;
const MAX_LABEL: usize = 63;
const DOT: u8 = b'.';
const DASH: u8 = b'-';
const LOCALHOST: [u8; 9] = *b"localhost";

fn is_letter(b: u8) -> bool {
    (b'a' <= b && b <= b'z') || (b'A' <= b && b <= b'Z')
}

fn is_label_byte(b: u8) -> bool {
    is_letter(b) || (b'0' <= b && b <= b'9') || b == DASH
}

fn all_label_bytes(host: &[u8], start: usize, end: usize) -> bool {
    let mut ok = true;
    let mut i = start;
    while ok && i < end {
        ok = is_label_byte(host[i]);
        i += 1;
    }
    ok
}

/// `host[start..end]`, with `start <= end <= host.len()`.
fn label_ok(host: &[u8], start: usize, end: usize) -> bool {
    if end <= start || end - start > MAX_LABEL {
        return false;
    }
    host[start] != DASH && host[end - 1] != DASH && all_label_bytes(host, start, end)
}

/// Every label is good, and there are at least two.
fn labels_ok(host: &[u8]) -> bool {
    let mut ok = true;
    let mut labels: usize = 0;
    let mut start = 0;
    let mut i = 0;
    while ok && i <= host.len() {
        if i == host.len() || host[i] == DOT {
            ok = label_ok(host, start, i);
            labels += 1;
            start = i + 1;
        }
        i += 1;
    }
    ok && labels >= 2
}

fn last_label_start(host: &[u8]) -> usize {
    let mut start = 0;
    let mut i = 0;
    while i < host.len() {
        if host[i] == DOT {
            start = i + 1;
        }
        i += 1;
    }
    start
}

fn lower_copy(bytes: &[u8], start: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = start;
    while i < bytes.len() {
        out.push(to_lower(bytes[i]));
        i += 1;
    }
    out
}

/// A last label that starts with a letter is never an IP address, also none in a form
/// such as `127.1` or `0x7f000001`.
fn last_label_ok(host: &[u8]) -> bool {
    let start = last_label_start(host);
    if start >= host.len() {
        return false;
    }
    is_letter(host[start]) && !bytes_equal(&lower_copy(host, start), &LOCALHOST)
}

/// A DNS name with at least two labels, and never an IP address or `localhost`.
#[must_use]
pub fn good_host_name(host: &[u8]) -> bool {
    if host.len() == 0 || host.len() > MAX_NAME {
        return false;
    }
    labels_ok(host) && last_label_ok(host)
}

/// A good host name that equals a name of the list, without ASCII case.
#[must_use]
pub fn host_allowed(list: &[Vec<u8>], host: &[u8]) -> bool {
    if !good_host_name(host) {
        return false;
    }
    let lower = lower_copy(host, 0);
    let mut found = false;
    let mut i = 0;
    while !found && i < list.len() {
        found = bytes_equal(&lower_copy(&list[i], 0), &lower);
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<Vec<u8>> {
        names.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    #[test]
    fn a_dns_name_with_two_labels_is_good() {
        for host in [
            "github.com",
            "index.crates.io",
            "a-b.c0.example",
            "x.y",
            "API.Anthropic.COM",
        ] {
            assert!(good_host_name(host.as_bytes()), "{host}");
        }
    }

    #[test]
    fn an_ip_address_localhost_or_a_bad_name_is_not_good() {
        let long_label = format!("{}.com", "a".repeat(64));
        let long_name = format!("{}com", "a.".repeat(127));
        for host in [
            "",
            "localhost",
            "a.localhost",
            "a.LocalHost",
            "127.0.0.1",
            "127.1",
            "0x7f.1",
            "[::1]",
            "::1",
            "*.github.com",
            "github.com:443",
            "https://github.com",
            "git hub.com",
            "-a.com",
            "a-.com",
            "a.-com",
            "a..com",
            ".a.com",
            "a.com.",
            "intranet",
            "a.1com",
            &long_label,
            &long_name,
        ] {
            assert!(!good_host_name(host.as_bytes()), "{host:?}");
        }
    }

    #[test]
    fn a_label_of_63_bytes_and_a_name_of_253_bytes_are_good() {
        let label = format!("{}.com", "a".repeat(63));
        let name = format!("{}.com", "a.".repeat(124) + "a");
        assert_eq!(name.len(), 253);
        assert!(good_host_name(label.as_bytes()));
        assert!(good_host_name(name.as_bytes()));
    }

    #[test]
    fn a_host_matches_without_case_and_only_by_its_exact_name() {
        let hosts = list(&["Nodejs.org", "github.com"]);

        assert!(host_allowed(&hosts, b"nodejs.org"));
        assert!(host_allowed(&hosts, b"GITHUB.com"));
        assert!(!host_allowed(&hosts, b"evil.github.com"));
        assert!(!host_allowed(&hosts, b"github.com.evil.net"));
        assert!(!host_allowed(&hosts, b"xgithub.com"));
        assert!(!host_allowed(&[], b"github.com"));
    }

    #[test]
    fn a_bad_name_never_matches_even_when_the_list_holds_it() {
        let hosts = list(&["localhost", "127.0.0.1", ""]);

        assert!(!host_allowed(&hosts, b"localhost"));
        assert!(!host_allowed(&hosts, b"127.0.0.1"));
        assert!(!host_allowed(&hosts, b""));
    }
}
