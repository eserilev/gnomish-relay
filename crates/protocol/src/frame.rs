//! A frame: the bytes that one strip carries. See `SPEC.md` 7.1.
//!
//! ```text
//! [0x6E 0x52] [version] [time: 4] [frame id: 2] [len: 2] [payload] [fletcher16: 2] [tag: 8]
//! ```
//!
//! Numbers are big-endian. The tag is an HMAC that the bridge checks. This
//! crate has no crypto, so it only carries the tag and says which bytes it covers.

pub const MAGIC: [u8; 2] = [0x6E, 0x52];
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 11;
pub const CHECKSUM_LEN: usize = 2;
pub const TAG_LEN: usize = 8;
pub const MAX_PAYLOAD: usize = 3200;

/// How old a frame can be, and how far in the future, in seconds.
pub const MAX_AGE: u32 = 300;
pub const MAX_AHEAD: u32 = 60;

pub struct Frame {
    pub time: u32,
    pub frame_id: u16,
    pub payload: Vec<u8>,
    pub tag: [u8; 8],
}

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

/// Returns `None` if the payload is longer than `MAX_PAYLOAD`.
#[must_use]
pub fn encode_frame(time: u32, frame_id: u16, payload: &[u8], tag: [u8; 8]) -> Option<Vec<u8>> {
    todo!()
}

/// Bytes after the tag are ignored. They are the zero padding of the last cell group.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, FrameError> {
    todo!()
}

/// The tag covers everything before it.
#[must_use]
pub fn signed_len(f: &Frame) -> usize {
    todo!()
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
