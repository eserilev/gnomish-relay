//! Text for the WoW chat frame. WoW reads `|` as the start of an escape code, for
//! links (`|H`), colors (`|c`), and textures (`|T`). `||` shows one `|`.

fn push_safe(out: &mut Vec<u8>, b: u8) {
    if b == b'|' {
        out.push(b'|');
    }
    out.push(b);
}

/// Doubles every `|`, so agent text shows as plain text.
#[must_use]
pub fn chat_safe(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        push_safe(&mut out, text[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(chat_safe(b"all tests pass"), b"all tests pass");
    }

    #[test]
    fn a_fake_link_shows_as_plain_text() {
        assert_eq!(chat_safe(b"|Hitem:1|h[Sword]|h"), b"||Hitem:1||h[Sword]||h");
    }

    #[test]
    fn a_pipe_at_the_end_is_doubled() {
        assert_eq!(chat_safe(b"a|"), b"a||");
    }
}
