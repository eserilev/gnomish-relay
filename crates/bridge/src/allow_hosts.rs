//! The hosts that a command of a game run reaches through the proxy (SPEC.md 6.6.4 and
//! 12). A host matches only by its exact name, without ASCII case.

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

const MAX_NAME: usize = 253;
const MAX_LABEL: usize = 63;

/// Whether the list starts with `DEFAULT_HOSTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defaults {
    Keep,
    Off,
}

/// Each name is in lower case and passed `check_host_name`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostList {
    names: Vec<String>,
}

impl HostList {
    pub fn new(defaults: Defaults, more: &[String]) -> Result<HostList, String> {
        let mut names: Vec<String> = match defaults {
            Defaults::Keep => DEFAULT_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
            Defaults::Off => Vec::new(),
        };
        for host in more {
            check_host_name(host)?;
            names.push(host.to_ascii_lowercase());
        }
        names.sort();
        names.dedup();
        Ok(HostList { names })
    }

    pub fn allows(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.names.contains(&host)
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// A DNS name with at least two labels. The last label starts with a letter, so no IP
/// address passes, also none in a form such as `127.1` or `0x7f000001`.
pub fn check_host_name(host: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("host {host:?} {why}"));
    if host.is_empty() || host.len() > MAX_NAME {
        return bad("has no name or a name that is too long");
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return bad("needs a dot, as in example.com");
    }
    if !labels.iter().all(|label| is_label(label)) {
        return bad("is not a host name: only letters, digits, `-`, and `.`");
    }
    let last = labels[labels.len() - 1];
    if !last.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return bad("is an IP address, not a host name");
    }
    if last.eq_ignore_ascii_case("localhost") {
        return bad("is this computer");
    }
    Ok(())
}

fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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
