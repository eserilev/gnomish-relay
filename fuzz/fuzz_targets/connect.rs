//! The head of a `CONNECT` request to the proxy of the sandbox (SPEC.md 6.6.4). A command
//! of a game run writes it, so it is untrusted.
//!
//! - No input panics the parser.
//! - A target that passes is a host name in lower case, never an IP address in any form.
//! - The allow list matches only its exact names.
#![no_main]

use bridge::allow_hosts::{Defaults, HostList, check_host_name};
use bridge::connect_line::{head_end, parse_connect};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = head_end(data);
    let Ok(target) = parse_connect(data) else {
        return;
    };
    assert!(check_host_name(&target.host).is_ok(), "{target:?}");
    assert_eq!(target.host, target.host.to_ascii_lowercase());
    assert!(target.host.parse::<std::net::IpAddr>().is_err());
    let last = target.host.rsplit('.').next().unwrap_or_default();
    assert!(last.starts_with(|c: char| c.is_ascii_alphabetic()), "{target:?}");
    let hosts = HostList::new(Defaults::Off, &["github.com".to_owned()]).unwrap();
    assert_eq!(hosts.allows(&target.host), target.host == "github.com");
});
