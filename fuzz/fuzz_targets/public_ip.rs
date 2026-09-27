//! S34 on the compiled code: the public address rule of the proxy (SPEC.md 6.6.4)
//! agrees with a table of ranges, written here as prefixes and masks.
#![no_main]

use std::net::{Ipv4Addr, Ipv6Addr};

use libfuzzer_sys::fuzz_target;
use protocol::ip::{is_public_v4, is_public_v6};

const V4_NOT_PUBLIC: [(u32, u32); 14] = [
    (0x0000_0000, 8),
    (0x0a00_0000, 8),
    (0x6440_0000, 10),
    (0x7f00_0000, 8),
    (0xa9fe_0000, 16),
    (0xac10_0000, 12),
    (0xc000_0000, 24),
    (0xc000_0200, 24),
    (0xc058_6300, 24),
    (0xc0a8_0000, 16),
    (0xc612_0000, 15),
    (0xc633_6400, 24),
    (0xcb00_7100, 24),
    (0xe000_0000, 3),
];

const V6_NOT_PUBLIC: [(u128, u32); 9] = [
    (0, 16),
    (0x0100 << 112, 16),
    (0x2001 << 112, 23),
    ((0x2001 << 112) | (0x0db8 << 96), 32),
    ((0x0064 << 112) | (0xff9b << 96), 32),
    (0xfc00 << 112, 7),
    (0xfe80 << 112, 10),
    (0xfec0 << 112, 10),
    (0xff00 << 112, 8),
];

fn in_v4(ip: u32, (net, bits): (u32, u32)) -> bool {
    let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
    ip & mask == net
}

fn in_v6(ip: u128, (net, bits): (u128, u32)) -> bool {
    let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
    ip & mask == net
}

fn model_v4(ip: u32) -> bool {
    !V4_NOT_PUBLIC.iter().any(|&range| in_v4(ip, range))
}

fn model_v6(ip: u128) -> bool {
    let mapped = in_v6(ip, (0xffff << 32, 96));
    let nat64 = in_v6(ip, ((0x0064 << 112) | (0xff9b << 96), 96));
    if mapped || nat64 {
        return model_v4(ip as u32);
    }
    if in_v6(ip, (0x2002 << 112, 16)) {
        return model_v4((ip >> 80) as u32);
    }
    !V6_NOT_PUBLIC.iter().any(|&range| in_v6(ip, range))
}

fuzz_target!(|data: &[u8]| {
    if let Ok(bytes) = <[u8; 4]>::try_from(&data[..data.len().min(4)]) {
        let ip = u32::from(Ipv4Addr::from(bytes));
        assert_eq!(is_public_v4(bytes), model_v4(ip), "{}", Ipv4Addr::from(bytes));
    }
    if let Ok(bytes) = <[u8; 16]>::try_from(&data[..data.len().min(16)]) {
        let ip = Ipv6Addr::from(bytes);
        assert_eq!(is_public_v6(ip.segments()), model_v6(u128::from(ip)), "{ip}");
    }
});
