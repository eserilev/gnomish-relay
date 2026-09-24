//! The body of a slot file. See `SPEC.md` 7.3.

pub const MAX_REPLIES: usize = 30;
pub const MAX_TEXT: usize = 32 * 1024;
pub const SLOT_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub enum Status {
    Working,
    Done,
    Error,
}

pub struct Reply {
    pub chat: Vec<u8>,
    pub id: u32,
    pub status: Status,
    pub text: Vec<u8>,
}

/// Keeps the last `MAX_REPLIES` replies. Cuts each text so that its Lua literal
/// fits in `MAX_TEXT` bytes, and each chat id to `MAX_ID_LEN` bytes.
#[must_use]
pub fn prepare_replies(replies: &[Reply]) -> Vec<Reply> {
    todo!()
}

#[must_use]
pub fn slot_body(now: u32, replies: &[Reply]) -> Vec<u8> {
    todo!()
}
