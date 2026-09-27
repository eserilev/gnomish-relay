//! Whether an address is on the public internet. The proxy of the sandbox connects only
//! to such an address, so an allowed name cannot lead to this computer or its network.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let not_public = a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224;
    !not_public
}

/// A form that holds an IPv4 address counts as that address.
fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_v4(ip) {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    let not_public = ip.is_unspecified()
        || ip.is_loopback()
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] & 0xff00) == 0xff00
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        // The local NAT64 range (RFC 8215) holds its IPv4 address at other bits.
        || (s[0] == 0x0064 && s[1] == 0xff9b)
        || (s[0] == 0x2001 && s[1] < 0x0200)
        || s[0] == 0x0100
        || s[0] == 0;
    !not_public
}

/// IPv4-mapped (`::ffff:a.b.c.d`), 6to4 (`2002:ab:cd::`), and NAT64 (`64:ff9b::a.b.c.d`).
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let tail = v4_of(s[6], s[7]);
    if s[..5] == [0; 5] && s[5] == 0xffff {
        return Some(tail);
    }
    if s[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        return Some(tail);
    }
    if s[0] == 0x2002 {
        return Some(v4_of(s[1], s[2]));
    }
    None
}

fn v4_of(high: u16, low: u16) -> Ipv4Addr {
    let [a, b] = high.to_be_bytes();
    let [c, d] = low.to_be_bytes();
    Ipv4Addr::new(a, b, c, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn a_public_address_is_public() {
        for ip in [
            "93.184.216.34",
            "140.82.112.3",
            "1.1.1.1",
            "2606:4700::1111",
            "2a04:4e42::644",
        ] {
            assert!(public(ip), "{ip}");
        }
    }

    #[test]
    fn this_computer_its_network_and_the_reserved_ranges_are_not_public() {
        for ip in [
            "0.0.0.0",
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "198.18.0.1",
            "192.88.99.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "100::1",
            "::127.0.0.1",
        ] {
            assert!(!public(ip), "{ip}");
        }
    }

    #[test]
    fn the_edges_of_the_private_ranges_stay_public() {
        for ip in [
            "172.15.255.255",
            "172.32.0.0",
            "100.63.255.255",
            "100.128.0.0",
        ] {
            assert!(public(ip), "{ip}");
        }
    }

    #[test]
    fn an_ipv6_form_of_a_private_ipv4_address_is_not_public() {
        for ip in [
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::192.168.0.1",
            "2002:7f00:1::",
            "2002:a9fe:a9fe::",
        ] {
            assert!(!public(ip), "{ip}");
        }
        assert!(public("::ffff:93.184.216.34"));
        assert!(public("2002:5db8:d822::"));
    }

    #[test]
    fn the_local_nat64_range_and_the_rest_of_its_block_are_not_public() {
        for ip in [
            "64:ff9b:1::5db8:d822",
            "64:ff9b:1:ffff::5db8:d822",
            "64:ff9b:0:0:1::5db8:d822",
            "64:ff9b:ffff::1",
        ] {
            assert!(!public(ip), "{ip}");
        }
        assert!(public("64:ff9b::5db8:d822"));
    }
}
