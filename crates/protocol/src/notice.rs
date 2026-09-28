//! The text of a notification from a terminal session (SPEC.md 10.2). Any local
//! process of the user can write it, so the game gets only visible characters.

use crate::ascii::push_range;

pub const MAX_REPO: usize = 64;
pub const MAX_TEXT: usize = 600;

/// The length of the UTF-8 sequence that `lead` starts, or 0 for a byte that
/// starts none: a continuation byte, an overlong lead, or a lead past U+10FFFF.
fn sequence_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    } else if lead < 0xC2 {
        0
    } else if lead < 0xE0 {
        2
    } else if lead < 0xF0 {
        3
    } else if lead < 0xF5 {
        4
    } else {
        0
    }
}

fn is_continuation(b: u8) -> bool {
    0x80 <= b && b <= 0xBF
}

fn continues_at(text: &[u8], j: usize) -> bool {
    j < text.len() && is_continuation(text[j])
}

/// A sequence of 2 to 4 bytes from `i` that the text holds in full.
fn whole_at(text: &[u8], i: usize, n: usize) -> bool {
    n >= 2
        && continues_at(text, i + 1)
        && (n < 3 || continues_at(text, i + 2))
        && (n < 4 || continues_at(text, i + 3))
}

fn in_range(b: u8, low: u8, high: u8) -> bool {
    low <= b && b <= high
}

/// C1 controls, U+0080 to U+009F, and the Arabic letter mark U+061C.
fn is_hidden_two(b0: u8, b1: u8) -> bool {
    if b0 == 0xC2 {
        b1 < 0xA0
    } else {
        b0 == 0xD8 && b1 == 0x9C
    }
}

/// In U+2000 to U+207F: U+200B to U+200F, U+2028 to U+202E, and U+2060 to U+206F.
fn is_hidden_punctuation(b1: u8, b2: u8) -> bool {
    if b1 == 0x80 {
        in_range(b2, 0x8B, 0x8F) || in_range(b2, 0xA8, 0xAE)
    } else {
        b1 == 0x81 && in_range(b2, 0xA0, 0xAF)
    }
}

/// Zero-width and bidi marks: U+180E, the ones of `is_hidden_punctuation`, and U+FEFF.
fn is_hidden_three(b0: u8, b1: u8, b2: u8) -> bool {
    if b0 == 0xE1 {
        b1 == 0xA0 && b2 == 0x8E
    } else if b0 == 0xE2 {
        is_hidden_punctuation(b1, b2)
    } else {
        b0 == 0xEF && b1 == 0xBB && b2 == 0xBF
    }
}

/// Tag characters, U+E0000 to U+E007F: invisible, and a known way to hide a prompt.
fn is_hidden_four(b0: u8, b1: u8, b2: u8) -> bool {
    b0 == 0xF3 && b1 == 0xA0 && (b2 == 0x80 || b2 == 0x81)
}

/// Takes a whole sequence of `n` bytes from `i`.
fn is_hidden(text: &[u8], i: usize, n: usize) -> bool {
    if n == 2 {
        is_hidden_two(text[i], text[i + 1])
    } else if n == 3 {
        is_hidden_three(text[i], text[i + 1], text[i + 2])
    } else {
        is_hidden_four(text[i], text[i + 1], text[i + 2])
    }
}

/// Bytes that `push_ascii` writes for `b`.
fn ascii_width(b: u8) -> usize {
    if b == b'|' { 2 } else { 1 }
}

/// A control byte becomes a space, so the words of two lines stay apart.
fn push_ascii(out: &mut Vec<u8>, b: u8) {
    if b == b'|' {
        out.push(b'|');
        out.push(b'|');
    } else if b < 0x20 || b == 0x7F {
        out.push(b' ');
    } else {
        out.push(b);
    }
}

/// At most `max` bytes of visible text. Each `|` is doubled (S10), and the cut
/// falls between two characters. S40 proves it.
#[must_use]
pub fn notice_text(text: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let n = sequence_len(text[i]);
        if n == 1 {
            if out.len() + ascii_width(text[i]) <= max {
                push_ascii(&mut out, text[i]);
                i += 1;
            } else {
                i = text.len();
            }
        } else if whole_at(text, i, n) {
            if is_hidden(text, i, n) {
                i += n;
            } else if out.len() + n <= max {
                push_range(&mut out, text, i, i + n);
                i += n;
            } else {
                i = text.len();
            }
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_stays_the_same() {
        assert_eq!(notice_text(b"All tests pass.", 600), b"All tests pass.");
    }

    #[test]
    fn a_pipe_is_doubled() {
        assert_eq!(notice_text(b"|Hitem:1|h", 600), b"||Hitem:1||h");
    }

    #[test]
    fn a_control_byte_becomes_a_space() {
        assert_eq!(
            notice_text(b"one\ntwo\tthree\x7F\x1B", 600),
            b"one two three  "
        );
    }

    #[test]
    fn letters_of_other_scripts_stay() {
        let text = "Fertig: café, 東京, 🎉".as_bytes();
        assert_eq!(notice_text(text, 600), text);
    }

    #[test]
    fn bidi_zero_width_and_tag_characters_go() {
        let text =
            "a\u{202E}b\u{200B}c\u{2066}d\u{FEFF}e\u{061C}f\u{180E}g\u{0085}h\u{E0041}i\u{2028}j";
        assert_eq!(notice_text(text.as_bytes(), 600), b"abcdefghij");
    }

    #[test]
    fn broken_utf8_goes() {
        assert_eq!(
            notice_text(b"a\x80b\xC3c\xE2\x82d\xF8e\xC0\xAFf", 600),
            b"abcdef"
        );
    }

    #[test]
    fn a_sequence_cut_at_the_end_of_the_input_goes() {
        assert_eq!(notice_text(b"ab\xE2\x82", 600), b"ab");
    }

    #[test]
    fn the_cut_falls_between_characters() {
        let text = "aé€".as_bytes();
        assert_eq!(notice_text(text, 2), b"a");
        assert_eq!(notice_text(text, 3), "aé".as_bytes());
        assert_eq!(notice_text(text, 5), "aé".as_bytes());
        assert_eq!(notice_text(text, 6), text);
    }

    #[test]
    fn a_doubled_pipe_is_never_cut_in_half() {
        assert_eq!(notice_text(b"a|", 2), b"a");
    }

    #[test]
    fn the_output_never_passes_max() {
        let long = vec![b'|'; 1000];
        assert_eq!(notice_text(&long, MAX_TEXT).len(), MAX_TEXT);
        assert_eq!(notice_text(&long, 0), b"");
    }
}
