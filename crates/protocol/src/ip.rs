//! Whether an address is on the public internet. The proxy of the bridge connects only
//! to such an address, so an allowed name cannot lead to this computer or its network.
//! See `SPEC.md` 6.6.4 and 14.1, S34.

/// The ranges of IPv4 that are not public, as pairs of the first and the last address.
const V4_NOT_PUBLIC: [u32; 28] = [
    0x0000_0000,
    0x00ff_ffff, // 0.0.0.0/8
    0x0a00_0000,
    0x0aff_ffff, // 10.0.0.0/8
    0x6440_0000,
    0x647f_ffff, // 100.64.0.0/10
    0x7f00_0000,
    0x7fff_ffff, // 127.0.0.0/8
    0xa9fe_0000,
    0xa9fe_ffff, // 169.254.0.0/16
    0xac10_0000,
    0xac1f_ffff, // 172.16.0.0/12
    0xc000_0000,
    0xc000_00ff, // 192.0.0.0/24
    0xc000_0200,
    0xc000_02ff, // 192.0.2.0/24
    0xc058_6300,
    0xc058_63ff, // 192.88.99.0/24
    0xc0a8_0000,
    0xc0a8_ffff, // 192.168.0.0/16
    0xc612_0000,
    0xc613_ffff, // 198.18.0.0/15
    0xc633_6400,
    0xc633_64ff, // 198.51.100.0/24
    0xcb00_7100,
    0xcb00_71ff, // 203.0.113.0/24
    0xe000_0000,
    0xffff_ffff, // 224.0.0.0/3
];

/// The ranges of IPv6 that are not public, by the first 32 bits, as pairs of the first
/// and the last value. Each range is a prefix of at most 32 bits, so these bits decide.
const V6_NOT_PUBLIC: [u32; 18] = [
    0x0000_0000,
    0x0000_ffff, // ::/16, with :: and ::1
    0x0100_0000,
    0x0100_ffff, // 100::/16
    0x2001_0000,
    0x2001_01ff, // 2001::/23
    0x2001_0db8,
    0x2001_0db8, // 2001:db8::/32
    0x0064_ff9b,
    0x0064_ff9b, // 64:ff9b::/32, with the local range 64:ff9b:1::/48
    0xfc00_0000,
    0xfdff_ffff, // fc00::/7
    0xfe80_0000,
    0xfebf_ffff, // fe80::/10
    0xfec0_0000,
    0xfeff_ffff, // fec0::/10
    0xff00_0000,
    0xffff_ffff, // ff00::/8
];

/// Whether `x` is in a range of `table`, a list of pairs of the first and the last value.
fn in_ranges(table: &[u32], x: u32) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i + 1 < table.len() {
        found = table[i] <= x && x <= table[i + 1];
        i += 2;
    }
    found
}

fn v4_value(octets: [u8; 4]) -> u32 {
    octets[0] as u32 * 0x100_0000
        + octets[1] as u32 * 0x1_0000
        + octets[2] as u32 * 0x100
        + octets[3] as u32
}

/// Two segments of IPv6 as one 32-bit value.
fn pair_value(high: u16, low: u16) -> u32 {
    high as u32 * 0x1_0000 + low as u32
}

fn is_public_value(v4: u32) -> bool {
    !in_ranges(&V4_NOT_PUBLIC, v4)
}

#[must_use]
pub fn is_public_v4(octets: [u8; 4]) -> bool {
    is_public_value(v4_value(octets))
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

/// A form that holds an IPv4 address counts as that address: IPv4-mapped, NAT64, and
/// 6to4 (`2002:abcd:efgh::`).
#[must_use]
pub fn is_public_v6(segments: [u16; 8]) -> bool {
    if is_mapped(segments) || is_nat64(segments) {
        return is_public_value(pair_value(segments[6], segments[7]));
    }
    if segments[0] == 0x2002 {
        return is_public_value(pair_value(segments[1], segments[2]));
    }
    !in_ranges(&V6_NOT_PUBLIC, pair_value(segments[0], segments[1]))
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

    #[test]
    fn an_odd_table_length_leaves_out_the_last_value() {
        assert!(in_ranges(&[1, 3], 2));
        assert!(!in_ranges(&[1, 3, 5], 5));
        assert!(!in_ranges(&[], 0));
    }
}
