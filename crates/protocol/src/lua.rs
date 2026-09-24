//! Lua string literals for the slot files. WoW runs a slot file as Lua code, so a
//! bad escape lets an agent reply run code in the game. See `SPEC.md` 14.1, S8.

pub(crate) fn is_plain(b: u8) -> bool {
    b' ' <= b && b <= b'~' && b != b'"' && b != b'\\'
}

/// Always three digits, so a digit after the escape can never join it.
fn push_escape(out: &mut Vec<u8>, b: u8) {
    out.push(b'\\');
    out.push(b'0' + b / 100);
    out.push(b'0' + b / 10 % 10);
    out.push(b'0' + b % 10);
}

fn push_escaped(out: &mut Vec<u8>, b: u8) {
    if is_plain(b) {
        out.push(b);
    } else {
        push_escape(out, b);
    }
}

/// A double-quoted Lua 5.1 literal that reads back as exactly `bytes`.
#[must_use]
pub fn lua_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'"');
    let mut i = 0;
    while i < bytes.len() {
        push_escaped(&mut out, bytes[i]);
        i += 1;
    }
    out.push(b'"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_quoted_as_is() {
        assert_eq!(lua_string(b"hello world"), b"\"hello world\"");
    }

    #[test]
    fn a_quote_cannot_end_the_literal() {
        assert_eq!(
            lua_string(b"a\" .. os.exit() .. \""),
            b"\"a\\034 .. os.exit() .. \\034\""
        );
    }

    #[test]
    fn a_backslash_is_escaped() {
        assert_eq!(lua_string(b"a\\b"), b"\"a\\092b\"");
    }

    #[test]
    fn newlines_and_nul_are_escaped() {
        assert_eq!(lua_string(b"a\nb\0"), b"\"a\\010b\\000\"");
    }

    #[test]
    fn a_digit_after_an_escape_stays_separate() {
        assert_eq!(lua_string(b"\n1"), b"\"\\0101\"");
    }

    #[test]
    fn high_bytes_are_escaped() {
        assert_eq!(lua_string(&[0xFF]), b"\"\\255\"");
    }
}
