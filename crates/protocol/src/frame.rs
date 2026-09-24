//! A frame: the bytes that one strip carries. See `SPEC.md` 7.1.
//!
//! ```text
//! [0x6E 0x52] [version] [time: 4] [frame id: 2] [len: 2] [payload] [fletcher16: 2] [tag: 8]
//! ```
//!
//! Numbers are big-endian. The tag is an HMAC that the bridge checks. This
//! crate has no crypto, so it only carries the tag and says which bytes it covers.

// Each narrowing cast takes one byte of a larger number, on purpose.
#![allow(clippy::cast_possible_truncation)]

pub const MAGIC: [u8; 2] = [0x6E, 0x52];
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 11;
pub const CHECKSUM_LEN: usize = 2;
pub const TAG_LEN: usize = 8;
pub const MAX_PAYLOAD: usize = 3200;

/// How old a frame can be, and how far in the future, in seconds.
pub const MAX_AGE: u32 = 300;
pub const MAX_AHEAD: u32 = 60;

use crate::ascii::{push_bytes, push_range};

pub struct Frame {
    pub time: u32,
    pub frame_id: u16,
    pub payload: Vec<u8>,
    pub tag: [u8; 8],
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub enum FrameError {
    TooShort,
    BadMagic,
    BadVersion,
    TooLong,
    Truncated,
    BadChecksum,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub enum Reject {
    BadTag,
    Stale,
    Future,
}

fn push_be16(out: &mut Vec<u8>, n: u16) {
    out.push((n >> 8) as u8);
    out.push(n as u8);
}

fn push_be32(out: &mut Vec<u8>, n: u32) {
    out.push((n >> 24) as u8);
    out.push((n >> 16) as u8);
    out.push((n >> 8) as u8);
    out.push(n as u8);
}

fn read_be16(bytes: &[u8], at: usize) -> u16 {
    (bytes[at] as u16) << 8 | bytes[at + 1] as u16
}

fn read_be32(bytes: &[u8], at: usize) -> u32 {
    (bytes[at] as u32) << 24
        | (bytes[at + 1] as u32) << 16
        | (bytes[at + 2] as u32) << 8
        | bytes[at + 3] as u32
}

/// Fletcher-16 over `bytes[start..end]`, low sum first.
fn fletcher16(bytes: &[u8], start: usize, end: usize) -> (u8, u8) {
    let mut s1: u16 = 0;
    let mut s2: u16 = 0;
    let mut i = start;
    while i < end {
        s1 = (s1 + bytes[i] as u16) % 255;
        s2 = (s2 + s1) % 255;
        i += 1;
    }
    (s1 as u8, s2 as u8)
}

/// Only called with a length of at most `MAX_PAYLOAD`.
fn payload_len(payload: &[u8]) -> u16 {
    payload.len() as u16
}

/// Returns `None` if the payload is longer than `MAX_PAYLOAD`.
#[must_use]
pub fn encode_frame(time: u32, frame_id: u16, payload: &[u8], tag: [u8; 8]) -> Option<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD {
        return None;
    }
    let mut out = Vec::new();
    push_bytes(&mut out, &MAGIC);
    out.push(VERSION);
    push_be32(&mut out, time);
    push_be16(&mut out, frame_id);
    push_be16(&mut out, payload_len(payload));
    push_bytes(&mut out, payload);
    let (s1, s2) = fletcher16(&out, 2, out.len());
    out.push(s1);
    out.push(s2);
    push_bytes(&mut out, &tag);
    Some(out)
}

/// Bytes after the tag are ignored. They are the zero padding of the last cell group.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, FrameError> {
    let n = bytes.len();
    if n < HEADER_LEN + CHECKSUM_LEN + TAG_LEN {
        return Err(FrameError::TooShort);
    }
    if bytes[0] != MAGIC[0] || bytes[1] != MAGIC[1] {
        return Err(FrameError::BadMagic);
    }
    if bytes[2] != VERSION {
        return Err(FrameError::BadVersion);
    }
    let len = read_be16(bytes, 9) as usize;
    if len > MAX_PAYLOAD {
        return Err(FrameError::TooLong);
    }
    let end = HEADER_LEN + len;
    if n < end + CHECKSUM_LEN + TAG_LEN {
        return Err(FrameError::Truncated);
    }
    let (s1, s2) = fletcher16(bytes, 2, end);
    if bytes[end] != s1 || bytes[end + 1] != s2 {
        return Err(FrameError::BadChecksum);
    }
    let mut payload = Vec::new();
    push_range(&mut payload, bytes, HEADER_LEN, end);
    let t = end + CHECKSUM_LEN;
    let tag = [
        bytes[t],
        bytes[t + 1],
        bytes[t + 2],
        bytes[t + 3],
        bytes[t + 4],
        bytes[t + 5],
        bytes[t + 6],
        bytes[t + 7],
    ];
    Ok(Frame {
        time: read_be32(bytes, 3),
        frame_id: read_be16(bytes, 7),
        payload,
        tag,
    })
}

/// The tag covers everything before it.
#[must_use]
pub fn signed_len(f: &Frame) -> usize {
    HEADER_LEN + f.payload.len() + CHECKSUM_LEN
}

// Widened to u64, so `+ MAX_AGE` cannot overflow near the end of u32 time.
fn is_stale(frame_time: u32, now: u32) -> bool {
    now as u64 > frame_time as u64 + MAX_AGE as u64
}

fn is_ahead(frame_time: u32, now: u32) -> bool {
    frame_time as u64 > now as u64 + MAX_AHEAD as u64
}

#[must_use]
pub fn is_fresh(frame_time: u32, now: u32) -> bool {
    !is_stale(frame_time, now) && !is_ahead(frame_time, now)
}

/// `tag_ok` comes from the bridge, which checks the HMAC over `signed_len` bytes.
pub fn check_frame(frame_time: u32, tag_ok: bool, now: u32) -> Result<(), Reject> {
    if !tag_ok {
        return Err(Reject::BadTag);
    }
    if is_stale(frame_time, now) {
        return Err(Reject::Stale);
    }
    if is_ahead(frame_time, now) {
        return Err(Reject::Future);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    fn frame(payload: &[u8]) -> Vec<u8> {
        encode_frame(1_790_000_000, 42, payload, TAG).unwrap()
    }

    fn decode(bytes: &[u8]) -> Result<Frame, FrameError> {
        decode_frame(bytes)
    }

    #[test]
    fn a_frame_decodes_to_its_own_fields() {
        let f = decode(&frame(b"hello")).unwrap();
        assert_eq!(f.time, 1_790_000_000);
        assert_eq!(f.frame_id, 42);
        assert_eq!(f.payload, b"hello");
        assert_eq!(f.tag, TAG);
    }

    #[test]
    fn padding_after_the_tag_is_ignored() {
        let mut bytes = frame(b"hi");
        bytes.extend_from_slice(&[0, 0]);
        assert_eq!(decode(&bytes).unwrap().payload, b"hi");
    }

    #[test]
    fn the_frame_layout_is_exact() {
        let bytes = frame(b"");
        assert_eq!(
            &bytes[..11],
            &[0x6E, 0x52, 1, 0x6A, 0xB1, 0x3B, 0x80, 0, 42, 0, 0]
        );
        assert_eq!(bytes.len(), 21);
        assert_eq!(signed_len(&decode(&bytes).unwrap()), 13);
    }

    #[test]
    fn a_payload_that_is_too_long_is_refused() {
        assert!(encode_frame(0, 0, &[0; MAX_PAYLOAD + 1], TAG).is_none());
        assert!(encode_frame(0, 0, &[0; MAX_PAYLOAD], TAG).is_some());
    }

    #[test]
    fn a_short_input_is_too_short() {
        assert_eq!(decode(&[0x6E; 20]).err(), Some(FrameError::TooShort));
    }

    #[test]
    fn a_wrong_magic_is_rejected() {
        let mut bytes = frame(b"x");
        bytes[1] = 0;
        assert_eq!(decode(&bytes).err(), Some(FrameError::BadMagic));
    }

    #[test]
    fn a_wrong_version_is_rejected() {
        let mut bytes = frame(b"x");
        bytes[2] = 2;
        assert_eq!(decode(&bytes).err(), Some(FrameError::BadVersion));
    }

    #[test]
    fn a_length_over_the_maximum_is_rejected() {
        let mut bytes = frame(b"x");
        bytes[9] = 0xFF;
        assert_eq!(decode(&bytes).err(), Some(FrameError::TooLong));
    }

    #[test]
    fn a_cut_frame_is_truncated() {
        let bytes = frame(b"hello");
        assert_eq!(
            decode(&bytes[..bytes.len() - 1]).err(),
            Some(FrameError::Truncated)
        );
    }

    #[test]
    fn a_changed_payload_byte_fails_the_checksum() {
        let mut bytes = frame(b"hello");
        bytes[12] ^= 1;
        assert_eq!(decode(&bytes).err(), Some(FrameError::BadChecksum));
    }

    #[test]
    fn a_frame_from_now_is_fresh() {
        assert!(is_fresh(1000, 1000));
    }

    #[test]
    fn a_frame_exactly_five_minutes_old_is_fresh() {
        assert!(is_fresh(1000, 1300));
    }

    #[test]
    fn a_frame_older_than_five_minutes_is_stale() {
        assert_eq!(check_frame(1000, true, 1301), Err(Reject::Stale));
    }

    #[test]
    fn a_frame_one_minute_ahead_is_fresh() {
        assert!(is_fresh(1060, 1000));
    }

    #[test]
    fn a_frame_more_than_one_minute_ahead_is_rejected() {
        assert_eq!(check_frame(1061, true, 1000), Err(Reject::Future));
    }

    #[test]
    fn a_bad_tag_wins_over_a_good_time() {
        assert_eq!(check_frame(1000, false, 1000), Err(Reject::BadTag));
    }

    #[test]
    fn times_near_the_end_of_u32_do_not_overflow() {
        assert!(is_fresh(u32::MAX, u32::MAX));
        assert_eq!(check_frame(0, true, u32::MAX), Err(Reject::Stale));
    }

    #[test]
    fn a_good_tag_and_time_pass() {
        assert_eq!(check_frame(1000, true, 1010), Ok(()));
    }
}
