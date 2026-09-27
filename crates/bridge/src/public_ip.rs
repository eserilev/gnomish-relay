//! Whether an address is on the public internet. The proxy connects only to such an
//! address, so an allowed name cannot lead to this computer or its network. The rules
//! live in `protocol::ip` (SPEC.md 14.1, S34).

use std::net::IpAddr;

use protocol::ip::{is_public_v4, is_public_v6};

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4.octets()),
        IpAddr::V6(v6) => is_public_v6(v6.segments()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn both_kinds_of_address_reach_their_rule() {
        assert!(public("93.184.216.34"));
        assert!(!public("192.168.1.1"));
        assert!(public("2606:4700::1111"));
        assert!(!public("::1"));
        assert!(!public("64:ff9b:1::5db8:d822"));
    }
}
