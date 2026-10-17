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

fn in_range(b: u8, low: u8, high: u8) -> bool {
    low <= b && b <= high
}

/// After these leads, the second byte has a smaller range: the other bytes give an
/// overlong form, a surrogate, or a code point past U+10FFFF.
fn second_byte_fits(lead: u8, b1: u8) -> bool {
    if lead == 0xE0 {
        b1 >= 0xA0
    } else if lead == 0xED {
        b1 < 0xA0
    } else if lead == 0xF0 {
        b1 >= 0x90
    } else if lead == 0xF4 {
        b1 < 0x90
    } else {
        true
    }
}

fn second_byte_fits_at(text: &[u8], i: usize) -> bool {
    i + 1 < text.len() && second_byte_fits(text[i], text[i + 1])
}

/// A valid sequence of 2 to 4 bytes from `i` that the text holds in full.
fn whole_at(text: &[u8], i: usize, n: usize) -> bool {
    n >= 2
        && continues_at(text, i + 1)
        && second_byte_fits_at(text, i)
        && (n < 3 || continues_at(text, i + 2))
        && (n < 4 || continues_at(text, i + 3))
}

/// C1 controls U+0080 to U+009F, the soft hyphen U+00AD, the combining grapheme
/// joiner U+034F, and the Arabic letter mark U+061C.
fn is_hidden_two(b0: u8, b1: u8) -> bool {
    if b0 == 0xC2 {
        b1 < 0xA0 || b1 == 0xAD
    } else if b0 == 0xCD {
        b1 == 0x8F
    } else {
        b0 == 0xD8 && b1 == 0x9C
    }
}

/// In U+1000 to U+1FFF: the Hangul fillers U+115F and U+1160, and the Mongolian
/// selectors and vowel separator U+180B to U+180F.
fn is_hidden_e1(b1: u8, b2: u8) -> bool {
    if b1 == 0x85 {
        b2 == 0x9F || b2 == 0xA0
    } else {
        b1 == 0xA0 && in_range(b2, 0x8B, 0x8F)
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

/// In U+F000 to U+FFFF: the variation selectors U+FE00 to U+FE0F, U+FEFF, the
/// half-width Hangul filler U+FFA0, and the interlinear marks U+FFF9 to U+FFFB.
fn is_hidden_ef(b1: u8, b2: u8) -> bool {
    if b1 == 0xB8 {
        in_range(b2, 0x80, 0x8F)
    } else if b1 == 0xBB {
        b2 == 0xBF
    } else if b1 == 0xBE {
        b2 == 0xA0
    } else {
        b1 == 0xBF && in_range(b2, 0xB9, 0xBB)
    }
}

/// Zero-width, bidi, and filler marks of 3 bytes. U+3164 is the Hangul filler.
fn is_hidden_three(b0: u8, b1: u8, b2: u8) -> bool {
    if b0 == 0xE1 {
        is_hidden_e1(b1, b2)
    } else if b0 == 0xE2 {
        is_hidden_punctuation(b1, b2)
    } else if b0 == 0xE3 {
        b1 == 0x85 && b2 == 0xA4
    } else {
        b0 == 0xEF && is_hidden_ef(b1, b2)
    }
}

/// After F3 A0: the tags U+E0000 to U+E007F, a known way to hide a prompt, and the
/// variation selectors U+E0100 to U+E01EF.
fn is_hidden_plane_14(b2: u8, b3: u8) -> bool {
    b2 == 0x80 || b2 == 0x81 || in_range(b2, 0x84, 0x86) || (b2 == 0x87 && b3 <= 0xAF)
}

/// The plane-14 marks, and the musical format marks U+1D173 to U+1D17A.
fn is_hidden_four(b0: u8, b1: u8, b2: u8, b3: u8) -> bool {
    if b0 == 0xF3 {
        b1 == 0xA0 && is_hidden_plane_14(b2, b3)
    } else {
        b0 == 0xF0 && b1 == 0x9D && b2 == 0x85 && in_range(b3, 0xB3, 0xBA)
    }
}

/// Takes a whole sequence of `n` bytes from `i`.
fn is_hidden(text: &[u8], i: usize, n: usize) -> bool {
    if n == 2 {
        is_hidden_two(text[i], text[i + 1])
    } else if n == 3 {
        is_hidden_three(text[i], text[i + 1], text[i + 2])
    } else {
        is_hidden_four(text[i], text[i + 1], text[i + 2], text[i + 3])
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
    fn fillers_and_variation_selectors_go() {
        let text = "a\u{00AD}b\u{034F}c\u{115F}d\u{1160}e\u{180B}f\u{180F}g\u{3164}h\u{FFA0}i\u{FE00}j\u{FE0F}k\u{FFF9}l\u{FFFB}m\u{E0100}n\u{E01EF}o\u{2029}p\u{1D173}q\u{1D17A}r";
        assert_eq!(notice_text(text.as_bytes(), 600), b"abcdefghijklmnopqr");
    }

    #[test]
    fn neighbors_of_hidden_characters_stay() {
        let text = "\u{00AC}\u{034E}\u{180A}\u{3165}\u{FE10}\u{FFFC}\u{E01F0}\u{1D172}\u{1D17B}";
        assert_eq!(notice_text(text.as_bytes(), 600), text.as_bytes());
    }

    #[test]
    fn overlong_forms_surrogates_and_too_big_code_points_go() {
        assert_eq!(notice_text(b"a\xE0\x80\xAFb", 600), b"ab");
        assert_eq!(notice_text(b"a\xF0\x80\x80\xAFb", 600), b"ab");
        assert_eq!(notice_text(b"a\xED\xA0\x80b", 600), b"ab");
        assert_eq!(notice_text(b"a\xF4\x90\x80\x80b", 600), b"ab");
    }

    #[test]
    fn the_edges_of_the_valid_ranges_stay() {
        let text = "\u{0800}\u{D7FF}\u{E000}\u{10000}\u{10FFFF}";
        assert_eq!(notice_text(text.as_bytes(), 600), text.as_bytes());
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
