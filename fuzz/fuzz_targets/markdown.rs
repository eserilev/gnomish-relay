//! The Markdown renderer on the compiled code: it never panics, its output has the
//! block shape of `SPEC.md` 7.3.1, no agent byte can start a WoW code or HTML
//! markup, and the output stays within a fixed multiple of the input.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::inline::{BOLD, BOLD_ITALIC, CODE, ITALIC, LINK};
use protocol::markdown::{FIELD, MARKER, render_markdown};

/// Checks one text field. Colors never nest, and each one closes in its field.
fn check_text(text: &[u8], html: bool) {
    let colors = [BOLD, ITALIC, BOLD_ITALIC, CODE, LINK];
    let mut open = false;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with(b"||") {
            i += 2;
        } else if rest.starts_with(b"|r") {
            assert!(open, "a reset with no color: {text:?}");
            open = false;
            i += 2;
        } else if colors.iter().any(|c| rest.starts_with(c)) {
            assert!(!open, "a nested color: {text:?}");
            open = true;
            i += 10;
        } else {
            let b = text[i];
            assert_ne!(b, b'|', "a lone pipe: {text:?}");
            assert!(b >= b' ' && b != 0x7F, "a control byte: {text:?}");
            if html {
                assert!(b != b'<' && b != b'>', "raw markup: {text:?}");
                if b == b'&' {
                    assert!(
                        [&b"&lt;"[..], b"&gt;", b"&amp;"].iter().any(|e| rest.starts_with(e)),
                        "a raw ampersand: {text:?}"
                    );
                }
            }
            i += 1;
        }
    }
    assert!(!open, "a color that never closes: {text:?}");
}

fn check_digit(field: &[u8], max: u8) {
    assert_eq!(field.len(), 1);
    assert!(b'0' <= field[0] && field[0] <= max);
}

fn check_block(line: &[u8]) {
    let (&kind, rest) = line.split_first().expect("no empty block");
    let fields: Vec<&[u8]> = rest.split(|&b| b == FIELD).skip(1).collect();
    assert_eq!(rest.first().copied().unwrap_or(FIELD), FIELD);
    match kind {
        b'h' => {
            assert_eq!(fields.len(), 2);
            check_digit(fields[0], b'3');
            assert_ne!(fields[0][0], b'0');
            check_text(fields[1], true);
        }
        b'p' | b'q' => {
            assert_eq!(fields.len(), 1);
            check_text(fields[0], true);
        }
        b'l' => {
            assert_eq!(fields.len(), 3);
            check_digit(fields[0], b'4');
            assert!(fields[1].len() <= 9 && fields[1].iter().all(u8::is_ascii_digit));
            check_text(fields[2], true);
        }
        b'c' => {
            assert_eq!(fields.len(), 1);
            check_text(fields[0], false);
        }
        b't' => {
            assert!(!fields.is_empty());
            assert!(fields[0] == b"0" || fields[0] == b"1");
            fields[1..].iter().for_each(|cell| check_text(cell, false));
        }
        b'r' => assert!(rest.is_empty()),
        _ => panic!("unknown block kind {kind}"),
    }
}

fuzz_target!(|data: &[u8]| {
    let out = render_markdown(data);
    assert!(out.len() <= 10 * data.len() + 4, "{} bytes from {}", out.len(), data.len());
    assert!(out.starts_with(&MARKER));
    assert_eq!(out.last(), Some(&b'\n'));
    let body = &out[MARKER.len()..out.len() - 1];
    for line in body.split(|&b| b == b'\n').skip(1) {
        check_block(line);
    }
    assert!(body.is_empty() || body[0] == b'\n');
});
