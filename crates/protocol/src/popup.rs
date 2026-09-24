//! The permission popup. A malicious agent can label `rm -rf ~` as "run tests",
//! so the popup shows the raw command first and the label of the agent below it.
//! See `SPEC.md` 6.4.

/// Raw command bytes shown in full. A longer command shows its start and end.
pub const COMMAND_BUDGET: usize = 300;
pub const LABEL_BUDGET: usize = 120;

use crate::ascii::{push_bytes, push_decimal};

// Arrays, not `&[u8]`: Aeneas cannot translate a reference stored in a constant.
const CUT_START: [u8; 6] = *b" [... ";
const CUT_END: [u8; 16] = *b" bytes cut ...] ";
const AGENT_SAYS: [u8; 17] = *b"\nthe agent says: ";
const LABEL_CUT: [u8; 6] = *b" [...]";

#[allow(clippy::cast_possible_truncation)] // clamped first
fn clamp_to_u32(n: usize) -> u32 {
    if n > u32::MAX as usize {
        u32::MAX
    } else {
        n as u32
    }
}

fn hex_digit(n: u8) -> u8 {
    if n < 10 { b'0' + n } else { b'a' + (n - 10) }
}

fn push_shown(out: &mut Vec<u8>, b: u8) {
    if b == b'\\' {
        out.push(b'\\');
        out.push(b'\\');
    } else if b' ' <= b && b <= b'~' {
        out.push(b);
    } else {
        out.push(b'\\');
        out.push(b'x');
        out.push(hex_digit(b / 16));
        out.push(hex_digit(b % 16));
    }
}

fn push_shown_range(out: &mut Vec<u8>, bytes: &[u8], start: usize, end: usize) {
    let mut i = start;
    while i < end {
        push_shown(out, bytes[i]);
        i += 1;
    }
}

fn push_command(out: &mut Vec<u8>, command: &[u8]) {
    let n = command.len();
    if n <= COMMAND_BUDGET {
        push_shown_range(out, command, 0, n);
    } else {
        let half = COMMAND_BUDGET / 2;
        push_shown_range(out, command, 0, half);
        push_bytes(out, &CUT_START);
        push_decimal(out, clamp_to_u32(n - COMMAND_BUDGET));
        push_bytes(out, &CUT_END);
        push_shown_range(out, command, n - half, n);
    }
}

fn push_label(out: &mut Vec<u8>, label: &[u8]) {
    let n = label.len();
    if n <= LABEL_BUDGET {
        push_shown_range(out, label, 0, n);
    } else {
        push_shown_range(out, label, 0, LABEL_BUDGET);
        push_bytes(out, &LABEL_CUT);
    }
}

/// Every byte outside printable ASCII shows as `\xHH`. This also hides bidi
/// and zero-width tricks.
#[must_use]
pub fn popup_text(command: &[u8], label: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    push_command(&mut out, command);
    push_bytes(&mut out, &AGENT_SAYS);
    push_label(&mut out, label);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_command_shows_in_full_before_the_label() {
        assert_eq!(
            popup_text(b"cargo test", b"run tests"),
            b"cargo test\nthe agent says: run tests"
        );
    }

    #[test]
    fn control_and_unicode_bytes_show_as_hex() {
        // U+202E (right-to-left override) is E2 80 AE in UTF-8.
        let shown = popup_text(b"rm\t\xE2\x80\xAE", b"");
        assert_eq!(shown, b"rm\\x09\\xe2\\x80\\xae\nthe agent says: ");
    }

    #[test]
    fn a_backslash_shows_doubled() {
        assert_eq!(popup_text(b"a\\b", b""), b"a\\\\b\nthe agent says: ");
    }

    #[test]
    fn a_long_command_keeps_its_start_and_end() {
        let mut command = vec![b'a'; 400];
        command[399] = b'Z';
        let shown = popup_text(&command, b"");
        let expected_start = [&[b'a'; 150][..], b" [... 100 bytes cut ...] "].concat();
        assert!(shown.starts_with(&expected_start));
        assert!(shown.ends_with(b"aZ\nthe agent says: "));
    }

    #[test]
    fn a_long_label_is_cut() {
        let shown = popup_text(b"ls", &[b'x'; 200]);
        assert!(shown.ends_with(&[&[b'x'; 120][..], b" [...]"].concat()));
    }
}
