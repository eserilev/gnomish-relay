//! Checks a frame from the strip and returns its records (SPEC.md 6.3, S2, S11).

use std::path::Path;

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use protocol::frame::{Reject, check_frame, decode_frame, signed_len};
use protocol::record::{Record, parse_records};
use sha2::Sha256;

pub struct StripKey(Vec<u8>);

impl StripKey {
    pub fn from_hex(hex: &str) -> Result<StripKey> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            bail!("the strip key is not 64 hex digits");
        }
        let key = (0..32)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .context("the strip key is not hex")?;
        Ok(StripKey(key))
    }

    pub fn load(path: &Path) -> Result<StripKey> {
        let hex = std::fs::read_to_string(path).with_context(|| {
            format!(
                "cannot read the strip key at {}. Run scripts/dev-link.sh",
                path.display()
            )
        })?;
        StripKey::from_hex(&hex)
    }

    /// `verify_truncated_left` compares in constant time.
    fn verify(&self, signed: &[u8], tag: &[u8]) -> bool {
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&self.0) else {
            return false;
        };
        mac.update(signed);
        mac.verify_truncated_left(tag).is_ok()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rejected {
    NotAFrame,
    BadTag,
    Stale,
    Future,
    BadRecords,
}

/// `bytes` can run past the end of the frame, as they do when read off a strip.
pub fn receive(bytes: &[u8], key: &StripKey, now: u32) -> Result<Vec<Record>, Rejected> {
    let Ok(frame) = decode_frame(bytes) else {
        return Err(Rejected::NotAFrame);
    };
    let tag_ok = key.verify(&bytes[..signed_len(&frame)], &frame.tag);
    match check_frame(frame.time, tag_ok, now) {
        Ok(()) => {}
        Err(Reject::BadTag) => return Err(Rejected::BadTag),
        Err(Reject::Stale) => return Err(Rejected::Stale),
        Err(Reject::Future) => return Err(Rejected::Future),
    }
    parse_records(&frame.payload).map_err(|_| Rejected::BadRecords)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::frame::encode_frame;

    const NOW: u32 = 1_790_211_079;

    fn key() -> StripKey {
        StripKey::from_hex(&"ab".repeat(32)).unwrap()
    }

    fn signed(time: u32, payload: &[u8], key: &StripKey) -> Vec<u8> {
        let mut wire = encode_frame(time, 1, payload, [0; 8]).unwrap();
        let body = wire.len() - 8;
        let mut mac = Hmac::<Sha256>::new_from_slice(&key.0).unwrap();
        mac.update(&wire[..body]);
        wire[body..].copy_from_slice(&mac.finalize().into_bytes()[..8]);
        wire
    }

    const RECORD: &[u8] = b"tok\x1fc1\x1f3\x1f\x1f\x1f\x1fhi";

    #[test]
    fn a_signed_fresh_frame_gives_its_records() {
        let mut wire = signed(NOW, RECORD, &key());
        wire.extend([7; 40]);
        let records = receive(&wire, &key(), NOW).unwrap();
        assert_eq!(records[0].text, b"hi");
    }

    #[test]
    fn a_frame_signed_with_another_key_is_rejected() {
        let other = StripKey::from_hex(&"cd".repeat(32)).unwrap();
        assert_eq!(
            receive(&signed(NOW, RECORD, &other), &key(), NOW).err(),
            Some(Rejected::BadTag)
        );
    }

    #[test]
    fn an_old_frame_is_rejected() {
        assert_eq!(
            receive(&signed(NOW - 301, RECORD, &key()), &key(), NOW).err(),
            Some(Rejected::Stale)
        );
    }

    #[test]
    fn a_frame_from_the_future_is_rejected() {
        assert_eq!(
            receive(&signed(NOW + 61, RECORD, &key()), &key(), NOW).err(),
            Some(Rejected::Future)
        );
    }

    #[test]
    fn bytes_that_are_not_a_frame_are_rejected() {
        assert_eq!(
            receive(b"not a frame", &key(), NOW).err(),
            Some(Rejected::NotAFrame)
        );
    }

    #[test]
    fn a_signed_frame_with_broken_records_is_rejected() {
        let wire = signed(NOW, b"only\x1ftwo fields", &key());
        assert_eq!(
            receive(&wire, &key(), NOW).err(),
            Some(Rejected::BadRecords)
        );
    }

    #[test]
    fn a_key_must_be_32_hex_bytes() {
        assert!(StripKey::from_hex("abcd").is_err());
        assert!(StripKey::from_hex(&"zz".repeat(32)).is_err());
        assert!(StripKey::from_hex(&"é".repeat(32)).is_err());
    }
}
