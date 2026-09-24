//! Replay protection: each `(token, id)` runs at most one time. See `SPEC.md` 8.3.

use crate::ascii::{bytes_equal, copy_bytes};

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

fn contains(entries: &[Entry], token: &[u8], id: u32) -> bool {
    let mut i = 0;
    while i < entries.len() {
        if entries[i].id == id && bytes_equal(&entries[i].token, token) {
            return true;
        }
        i += 1;
    }
    false
}

/// A copy of `entries[from..]`.
fn copy_entries(entries: &[Entry], from: usize) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut i = from;
    while i < entries.len() {
        out.push(Entry {
            token: copy_bytes(&entries[i].token),
            id: entries[i].id,
        });
        i += 1;
    }
    out
}

/// When full, the oldest entry goes.
fn first_kept(len: usize) -> usize {
    if len >= SEEN_CAPACITY { 1 } else { 0 }
}

/// Returns `true` and remembers the message if it is new. Returns `false` for a repeat.
#[must_use]
pub fn admit(history: Seen, token: &[u8], id: u32) -> (bool, Seen) {
    if contains(&history.entries, token, id) {
        return (false, history);
    }
    let mut entries = copy_entries(&history.entries, first_kept(history.entries.len()));
    entries.push(Entry {
        token: copy_bytes(token),
        id,
    });
    (true, Seen { entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_message_is_admitted_and_a_repeat_is_not() {
        let (first, seen) = admit(new_seen(), b"tok", 7);
        let (again, _) = admit(seen, b"tok", 7);
        assert!(first);
        assert!(!again);
    }

    #[test]
    fn the_same_id_with_another_token_is_new() {
        let (_, seen) = admit(new_seen(), b"tok", 7);
        assert!(admit(seen, b"other", 7).0);
    }

    #[test]
    fn the_oldest_message_goes_when_full() {
        let mut seen = new_seen();
        for id in 0..1000 {
            seen = admit(seen, b"t", id).1;
        }
        seen = admit(seen, b"t", 1000).1;
        assert_eq!(seen.entries.len(), 1000);
        assert_eq!(seen.entries[0].id, 1);
        assert!(admit(seen, b"t", 0).0);
    }
}
