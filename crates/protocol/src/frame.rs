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

#[must_use]
pub fn is_fresh(frame_time: u32, now: u32) -> bool {
    todo!()
}

/// `tag_ok` comes from the bridge, which checks the HMAC over `signed_len` bytes.
pub fn check_frame(frame_time: u32, tag_ok: bool, now: u32) -> Result<(), Reject> {
    todo!()
}
