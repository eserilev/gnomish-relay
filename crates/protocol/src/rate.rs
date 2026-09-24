//! Limits against strip spam. See `SPEC.md` 6.2.

pub const MAX_MESSAGES: usize = 10;
pub const WINDOW_SECONDS: u32 = 60;
pub const MAX_QUEUE: usize = 20;

/// The times of the messages admitted in the last `WINDOW_SECONDS`.
pub struct RateLimiter {
    pub times: Vec<u32>,
}

pub struct ChatQueue {
    pub ids: Vec<u32>,
}

#[must_use]
pub fn admit_message(limiter: RateLimiter, now: u32) -> (bool, RateLimiter) {
    todo!()
}

/// Returns `None` if the queue is full.
#[must_use]
pub fn enqueue(queue: ChatQueue, id: u32) -> Option<ChatQueue> {
    todo!()
}
