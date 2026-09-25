//! Byte search for the action classifier: a run of bytes, and a name in a list.

use crate::ascii::push_bytes;

const SPACE: u8 = b' ';

/// `a[at..at + n] == b[..n]`. The caller keeps `at + n <= a.len()` and `n <= b.len()`.
pub(crate) fn equal_run(a: &[u8], at: usize, b: &[u8], n: usize) -> bool {
    let mut same = true;
    let mut k = 0;
    while same && k < n {
        same = a[at + k] == b[k];
        k += 1;
    }
    same
}

pub(crate) fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let last = hay.len() - needle.len();
    let mut found = false;
    let mut at = 0;
    while !found && at <= last {
        found = equal_run(hay, at, needle, needle.len());
        at += 1;
    }
    found
}

/// A list is one byte string of names, each with a space before and after it,
/// for example `b" curl wget "`.
pub(crate) fn listed(names: &[u8], name: &[u8]) -> bool {
    // A name this long cannot be in the list, and the check keeps the pushes small.
    if name.len() >= names.len() {
        return false;
    }
    let mut needle = Vec::new();
    needle.push(SPACE);
    push_bytes(&mut needle, name);
    needle.push(SPACE);
    contains(names, &needle)
}

pub(crate) fn has_byte(bytes: &[u8], b: u8) -> bool {
    let mut found = false;
    let mut i = 0;
    while !found && i < bytes.len() {
        found = bytes[i] == b;
        i += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_finds_a_run_at_the_start_the_middle_and_the_end() {
        assert!(contains(b"abcdef", b"abc"));
        assert!(contains(b"abcdef", b"cd"));
        assert!(contains(b"abcdef", b"ef"));
        assert!(!contains(b"abcdef", b"fa"));
    }

    #[test]
    fn the_empty_needle_is_everywhere() {
        assert!(contains(b"", b""));
        assert!(contains(b"ab", b""));
    }

    #[test]
    fn a_needle_longer_than_the_hay_is_not_found() {
        assert!(!contains(b"ab", b"abc"));
    }

    #[test]
    fn listed_matches_whole_names_only() {
        let names = b" curl wget ";
        assert!(listed(names, b"curl"));
        assert!(listed(names, b"wget"));
        assert!(!listed(names, b"url"));
        assert!(!listed(names, b"cur"));
        assert!(!listed(names, b""));
    }

    #[test]
    fn a_name_as_long_as_the_list_is_not_listed() {
        assert!(!listed(b" a ", b" a "));
    }

    #[test]
    fn has_byte_finds_any_position() {
        assert!(has_byte(b"a=b", b'='));
        assert!(!has_byte(b"ab", b'='));
        assert!(!has_byte(b"", b'='));
    }
}
