//! Whether an address is on the public internet. The proxy of the bridge connects only
//! to such an address, so an allowed name cannot lead to this computer or its network.
//! See `SPEC.md` 6.6.4 and 14.1, S34.

fn v4_low_ranges(a: u8, b: u8) -> bool {
    a == 0
        || a == 10
        || a == 127
        || (a == 100 && b >= 64 && b < 128)
        || (a == 169 && b == 254)
        || (a == 172 && b >= 16 && b < 32)
}

fn v4_high_ranges(a: u8, b: u8, c: u8) -> bool {
    (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
}

#[must_use]
pub fn is_public_v4(octets: [u8; 4]) -> bool {
    !(v4_low_ranges(octets[0], octets[1]) || v4_high_ranges(octets[0], octets[1], octets[2]))
}

fn high_byte(segment: u16) -> u8 {
    (segment >> 8) as u8
}

fn low_byte(segment: u16) -> u8 {
    (segment & 0xff) as u8
}

fn v4_of(high: u16, low: u16) -> [u8; 4] {
    [
        high_byte(high),
        low_byte(high),
        high_byte(low),
        low_byte(low),
    ]
}

/// `::ffff:a.b.c.d`.
fn is_mapped(s: [u16; 8]) -> bool {
    s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xffff
}

/// `64:ff9b::a.b.c.d`, the well-known prefix of NAT64. Only this `/96` holds its IPv4
/// address in the last 32 bits.
fn is_nat64(s: [u16; 8]) -> bool {
    s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0
}

/// `2002:abcd:efgh::`, 6to4.
fn is_6to4(s: [u16; 8]) -> bool {
    s[0] == 0x2002
}

/// `::/16` holds `::` and `::1`. `64:ff9b::/32` holds the local NAT64 range
/// `64:ff9b:1::/48` (RFC 8215).
fn v6_ranges(s: [u16; 8]) -> bool {
    s[0] == 0
        || s[0] == 0x0100
        || (s[0] == 0x2001 && s[1] < 0x0200)
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || (s[0] == 0x0064 && s[1] == 0xff9b)
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] & 0xff00) == 0xff00
}

/// A form that holds an IPv4 address counts as that address.
#[must_use]
pub fn is_public_v6(segments: [u16; 8]) -> bool {
    if is_mapped(segments) || is_nat64(segments) {
        return is_public_v4(v4_of(segments[6], segments[7]));
    }
    if is_6to4(segments) {
        return is_public_v4(v4_of(segments[1], segments[2]));
    }
    !v6_ranges(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v6(text: &str) -> [u16; 8] {
        text.parse::<std::net::Ipv6Addr>().unwrap().segments()
    }

    fn v4(text: &str) -> [u8; 4] {
        text.parse::<std::net::Ipv4Addr>().unwrap().octets()
    }

    #[test]
    fn a_public_address_is_public() {
        for ip in ["93.184.216.34", "140.82.112.3", "1.1.1.1"] {
            assert!(is_public_v4(v4(ip)), "{ip}");
        }
        for ip in ["2606:4700::1111", "2a04:4e42::644"] {
            assert!(is_public_v6(v6(ip)), "{ip}");
        }
    }

    #[test]
    fn this_computer_its_network_and_the_reserved_v4_ranges_are_not_public() {
        for ip in [
            "0.0.0.0",
            "127.0.0.1",
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
            "198.19.0.1",
            "192.88.99.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public_v4(v4(ip)), "{ip}");
        }
    }

    #[test]
    fn the_edges_of_the_v4_ranges_stay_public() {
        for ip in [
            "172.15.255.255",
            "172.32.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "192.0.1.1",
            "192.88.98.1",
            "198.17.0.1",
            "198.20.0.1",
            "198.51.101.1",
            "203.0.114.1",
            "223.255.255.255",
        ] {
            assert!(is_public_v4(v4(ip)), "{ip}");
        }
    }

    #[test]
    fn this_computer_its_network_and_the_reserved_v6_ranges_are_not_public() {
        for ip in [
            "::",
            "::1",
            "::127.0.0.1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "2001:1ff::1",
            "100::1",
        ] {
            assert!(!is_public_v6(v6(ip)), "{ip}");
        }
        assert!(is_public_v6(v6("2001:200::1")));
    }

    #[test]
    fn an_ipv6_form_of_a_v4_address_counts_as_that_address() {
        for ip in [
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::192.168.0.1",
            "2002:7f00:1::",
            "2002:a9fe:a9fe::",
        ] {
            assert!(!is_public_v6(v6(ip)), "{ip}");
        }
        for ip in [
            "::ffff:93.184.216.34",
            "64:ff9b::5db8:d822",
            "2002:5db8:d822::",
        ] {
            assert!(is_public_v6(v6(ip)), "{ip}");
        }
    }

    #[test]
    fn the_local_nat64_range_and_the_rest_of_its_block_are_not_public() {
        for ip in [
            "64:ff9b:1::5db8:d822",
            "64:ff9b:0:0:1::5db8:d822",
            "64:ff9b:ffff::1",
        ] {
            assert!(!is_public_v6(v6(ip)), "{ip}");
        }
    }
}
