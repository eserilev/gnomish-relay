//! String literals for the Seatbelt profile of the agent sandbox on macOS. A bad escape
//! lets a folder name end the literal and add a rule. See `SPEC.md` 6.6.4 and 14.1, S32.

use crate::search::has_byte;

const QUOTE: u8 = b'"';
const BACKSLASH: u8 = b'\\';

fn needs_backslash(b: u8) -> bool {
    b == QUOTE || b == BACKSLASH
}

/// Every other byte goes in as it is. The reader keeps a line break or a high byte.
fn push_escaped(out: &mut Vec<u8>, b: u8) {
    if needs_backslash(b) {
        out.push(BACKSLASH);
    }
    out.push(b);
}

fn quoted(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(QUOTE);
    let mut i = 0;
    while i < bytes.len() {
        push_escaped(&mut out, bytes[i]);
        i += 1;
    }
    out.push(QUOTE);
    out
}

/// A double-quoted SBPL literal that reads back as exactly `bytes`. `None` for a NUL
/// byte: the profile is a C string, and no path holds one.
#[must_use]
pub fn sbpl_string(bytes: &[u8]) -> Option<Vec<u8>> {
    if has_byte(bytes, 0) {
        return None;
    }
    Some(quoted(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal(bytes: &[u8]) -> Vec<u8> {
        sbpl_string(bytes).unwrap()
    }

    #[test]
    fn a_plain_path_is_quoted_as_is() {
        assert_eq!(literal(b"/Users/x/Code"), b"\"/Users/x/Code\"");
    }

    #[test]
    fn a_quote_in_a_path_cannot_end_the_literal() {
        assert_eq!(
            literal(b"/a\") (allow default) (\""),
            b"\"/a\\\") (allow default) (\\\"\""
        );
    }

    #[test]
    fn a_backslash_is_doubled() {
        assert_eq!(literal(b"/a\\b"), b"\"/a\\\\b\"");
    }

    #[test]
    fn line_breaks_and_high_bytes_stay_as_they_are() {
        assert_eq!(literal(b"/a\nb\xff"), b"\"/a\nb\xff\"");
    }

    #[test]
    fn a_path_with_a_nul_byte_has_no_literal() {
        assert_eq!(sbpl_string(b"/a\0b"), None);
    }
}
