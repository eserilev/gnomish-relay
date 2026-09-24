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

/// Widened to u64, so `+ WINDOW_SECONDS` cannot overflow.
fn in_window(t: u32, now: u32) -> bool {
    t as u64 <= now as u64 && (now as u64) < t as u64 + WINDOW_SECONDS as u64
}

/// The admitted times that still count at `now`, in order.
fn still_counting(times: &[u32], now: u32) -> Vec<u32> {
    let mut kept = Vec::new();
    let mut i = 0;
    while i < times.len() {
        if in_window(times[i], now) {
            kept.push(times[i]);
        }
        i += 1;
    }
    kept
}

#[must_use]
pub fn admit_message(limiter: RateLimiter, now: u32) -> (bool, RateLimiter) {
    let mut kept = still_counting(&limiter.times, now);
    if kept.len() < MAX_MESSAGES {
        kept.push(now);
        (true, RateLimiter { times: kept })
    } else {
        (false, RateLimiter { times: kept })
    }
}

/// Returns `None` if the queue is full.
#[must_use]
pub fn enqueue(queue: ChatQueue, id: u32) -> Option<ChatQueue> {
    if queue.ids.len() >= MAX_QUEUE {
        return None;
    }
    let mut ids = queue.ids;
    ids.push(id);
    Some(ChatQueue { ids })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(times: &[u32]) -> Vec<bool> {
        let mut limiter = RateLimiter { times: Vec::new() };
        let mut out = Vec::new();
        for &t in times {
            let (ok, next) = admit_message(limiter, t);
            limiter = next;
            out.push(ok);
        }
        out
    }

    #[test]
    fn ten_messages_in_a_minute_pass_and_the_eleventh_does_not() {
        let decisions = run(&[0; 11]);
        assert!(decisions[..10].iter().all(|&ok| ok));
        assert!(!decisions[10]);
    }

    #[test]
    fn a_minute_later_messages_pass_again() {
        let mut times = vec![0; 10];
        times.push(60);
        assert!(run(&times)[10]);
    }

    #[test]
    fn at_59_seconds_the_old_messages_still_count() {
        let mut times = vec![0; 10];
        times.push(59);
        assert!(!run(&times)[10]);
    }

    #[test]
    fn a_full_queue_refuses() {
        let mut queue = ChatQueue { ids: Vec::new() };
        for id in 0..20 {
            queue = enqueue(queue, id).unwrap();
        }
        assert!(enqueue(queue, 20).is_none());
    }
}
