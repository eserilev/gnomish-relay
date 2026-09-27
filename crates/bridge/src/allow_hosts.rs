//! The hosts that a command of a game run reaches through the proxy (SPEC.md 6.6.4 and
//! 12). A host matches only by its exact name, without ASCII case.

use protocol::hosts::{good_host_name, host_allowed};

/// The package hosts that `cargo`, `rustup`, `git`, `npm`, and `pip` fetch from.
pub const DEFAULT_HOSTS: &[&str] = &[
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    "github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "raw.githubusercontent.com",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
];

/// Whether the list starts with `DEFAULT_HOSTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defaults {
    Keep,
    Off,
}

/// Each name passed `check_host_name`. The match rule lives in `protocol::hosts` (S33).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostList {
    names: Vec<Vec<u8>>,
}

impl HostList {
    pub fn new(defaults: Defaults, more: &[String]) -> Result<HostList, String> {
        let mut names: Vec<Vec<u8>> = match defaults {
            Defaults::Keep => DEFAULT_HOSTS
                .iter()
                .map(|h| h.as_bytes().to_vec())
                .collect(),
            Defaults::Off => Vec::new(),
        };
        for host in more {
            check_host_name(host)?;
            names.push(host.to_ascii_lowercase().into_bytes());
        }
        names.sort();
        names.dedup();
        Ok(HostList { names })
    }

    pub fn allows(&self, host: &str) -> bool {
        host_allowed(&self.names, host.as_bytes())
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// A DNS name with at least two labels, by `protocol::hosts::good_host_name`. Only the
/// error text is here: it says why a name is bad.
pub fn check_host_name(host: &str) -> Result<(), String> {
    if good_host_name(host.as_bytes()) {
        return Ok(());
    }
    Err(format!("host {host:?} {}", why_bad(host)))
}

fn why_bad(host: &str) -> &'static str {
    let last = host.rsplit('.').next().unwrap_or_default();
    if host.is_empty() || host.len() > 253 {
        "has no name or a name that is too long"
    } else if last.eq_ignore_ascii_case("localhost") {
        "is this computer"
    } else if !host.contains('.') {
        "needs a dot, as in example.com"
    } else if last.starts_with(|c: char| c.is_ascii_digit()) {
        "is an IP address, not a host name"
    } else {
        "is not a host name: only letters, digits, `-`, and `.`"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(more: &[&str]) -> HostList {
        let more: Vec<String> = more.iter().map(|h| (*h).to_owned()).collect();
        HostList::new(Defaults::Keep, &more).unwrap()
    }

    #[test]
    fn every_default_host_is_a_good_name() {
        for host in DEFAULT_HOSTS {
            assert_eq!(check_host_name(host), Ok(()), "{host}");
        }
    }

    #[test]
    fn the_defaults_allow_the_package_hosts() {
        let hosts = list(&[]);

        assert!(hosts.allows("index.crates.io"));
        assert!(hosts.allows("registry.npmjs.org"));
        assert!(!hosts.allows("crates.io"));
        assert!(!hosts.allows("example.com"));
    }

    #[test]
    fn a_host_matches_without_case_and_only_by_its_exact_name() {
        let hosts = list(&["Nodejs.org"]);

        assert!(hosts.allows("nodejs.org"));
        assert!(hosts.allows("GITHUB.com"));
        assert!(!hosts.allows("evil.github.com"));
        assert!(!hosts.allows("github.com.evil.net"));
        assert!(!hosts.allows("xgithub.com"));
    }

    #[test]
    fn with_the_defaults_off_only_the_hosts_of_the_config_match() {
        let hosts = HostList::new(Defaults::Off, &["nodejs.org".to_owned()]).unwrap();

        assert!(hosts.allows("nodejs.org"));
        assert!(!hosts.allows("github.com"));
        assert!(HostList::new(Defaults::Off, &[]).unwrap().is_empty());
    }

    #[test]
    fn an_ip_address_localhost_or_a_bad_name_is_refused() {
        for bad in [
            "",
            "localhost",
            "a.localhost",
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
            "a..com",
            "a.com.",
            "intranet",
            &format!("{}.com", "a".repeat(64)),
            &format!("{}com", "a.".repeat(127)),
        ] {
            assert!(check_host_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_bad_host_in_the_config_is_an_error() {
        let error = HostList::new(Defaults::Keep, &["10.0.0.1".to_owned()]).unwrap_err();

        assert!(error.contains("IP address"), "{error}");
    }
}
