//! Replay protection: each `(token, id)` runs at most one time. See `SPEC.md` 8.3.

pub const SEEN_CAPACITY: usize = 1000;

pub struct Entry {
    pub token: Vec<u8>,
    pub id: u32,
}

/// The last `SEEN_CAPACITY` messages, oldest first.
pub struct Seen {
    pub entries: Vec<Entry>,
}

#[must_use]
pub fn new_seen() -> Seen {
    Seen {
        entries: Vec::new(),
    }
}

/// Returns `true` and remembers the message if it is new. Returns `false` for a repeat.
#[must_use]
pub fn admit(history: Seen, token: &[u8], id: u32) -> (bool, Seen) {
    todo!()
}
