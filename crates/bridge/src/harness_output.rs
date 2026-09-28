//! The output of a `command` agent (SPEC.md 9.2): its lines become progress lines, and
//! all of it becomes the reply. A terminal program can print colors, cursor moves, and
//! progress bars, so the bridge keeps only the text.

use crate::process::cut;

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';
pub const LONG_OUTPUT: &str = "(The output was too long for the game. This is its end.)";

/// The text of `raw` with no escape sequence and no control character but a newline and a
/// tab. A line keeps only the text after its last CR, as a terminal shows a progress bar.
pub fn clean(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let plain = strip_escapes(&text);
    let lines: Vec<&str> = plain.split('\n').map(after_last_cr).collect();
    lines.join("\n")
}

/// A CR at the end, as in `\r\n`, is not a progress bar.
fn after_last_cr(line: &str) -> &str {
    let line = line.strip_suffix('\r').unwrap_or(line);
    match line.rfind('\r') {
        Some(at) => &line[at + 1..],
        None => line,
    }
}

/// What an escape sequence is, by its first character after ESC.
fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ESC => skip_escape(&mut chars),
            '\n' | '\t' | '\r' => out.push(ch),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// CSI (`ESC [`) ends at a byte from `@` to `~`. OSC (`ESC ]`) ends at BEL or `ESC \`.
/// Any other escape is two characters.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    return;
                }
            }
        }
        Some(']') => {
            while let Some(c) = chars.next() {
                if c == BEL {
                    return;
                }
                if c == ESC {
                    chars.next();
                    return;
                }
            }
        }
        _ => {}
    }
}

/// One progress line for the activity panel, with each run of white space as one space,
/// or `None` for a blank line.
pub fn progress_line(raw: &[u8], max: usize) -> Option<String> {
    let text = clean(raw);
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    Some(cut(&words.join(" "), max).to_owned())
}

/// The reply from all of the output. A harness prints its answer last, so a reply over
/// `max` bytes keeps its end, from the start of a line, after a note.
pub fn reply_text(output: &[u8], max: usize) -> String {
    let text = clean(output);
    let text = text.trim();
    if text.len() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(LONG_OUTPUT.len() + 2);
    let mut start = text.len() - keep;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let tail = &text[start..];
    let tail = match tail.find('\n') {
        Some(at) if at + 1 < tail.len() => &tail[at + 1..],
        _ => tail,
    };
    format!("{LONG_OUTPUT}\n\n{tail}")
}

/// The last bytes of an output: at most `max`, so a harness that prints for hours
/// never fills the memory of the bridge.
#[derive(Default)]
pub struct Tail {
    bytes: Vec<u8>,
    /// Every byte so far.
    pub total: u64,
}

impl Tail {
    pub fn push(&mut self, chunk: &[u8], max: usize) {
        self.total += chunk.len() as u64;
        self.bytes.extend_from_slice(chunk);
        // Twice the limit before a drain, so a drain is rare.
        if self.bytes.len() > max.saturating_mul(2) {
            let extra = self.bytes.len() - max;
            self.bytes.drain(..extra);
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Splits chunks of output into lines. A line longer than `max` bytes keeps its start.
#[derive(Default)]
pub struct Lines {
    open: Vec<u8>,
}

impl Lines {
    pub fn push(&mut self, chunk: &[u8], max: usize) -> Vec<Vec<u8>> {
        let mut done = Vec::new();
        for &byte in chunk {
            if byte == b'\n' {
                done.push(std::mem::take(&mut self.open));
            } else if self.open.len() < max {
                self.open.push(byte);
            }
        }
        done
    }

    /// The last line, which has no newline.
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        let last = std::mem::take(&mut self.open);
        (!last.is_empty()).then_some(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_cursor_moves_go_away() {
        assert_eq!(clean(b"\x1b[1;32mdone\x1b[0m \x1b[2K"), "done ");
    }

    #[test]
    fn a_title_sequence_goes_away_with_either_end() {
        assert_eq!(clean(b"\x1b]0;title\x07a\x1b]8;;http://x\x1b\\b"), "ab");
    }

    #[test]
    fn a_progress_bar_keeps_its_last_state() {
        assert_eq!(clean(b"10%\r50%\r100%\r\nnext"), "100%\nnext");
    }

    #[test]
    fn other_control_characters_go_away_and_tabs_stay() {
        assert_eq!(clean(b"a\x08\x00\tb\x7f\xc2\x9bc"), "a\tbc");
    }

    #[test]
    fn a_cut_escape_at_the_end_does_not_panic() {
        assert_eq!(clean(b"ok\x1b[12"), "ok");
        assert_eq!(clean(b"ok\x1b]0;t"), "ok");
        assert_eq!(clean(b"ok\x1b"), "ok");
    }

    #[test]
    fn bad_utf8_becomes_the_replacement_character() {
        assert_eq!(clean(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn a_blank_line_is_no_progress_and_a_long_line_is_cut() {
        assert_eq!(progress_line(b"  \x1b[0m \r\n", 200), None);
        assert_eq!(
            progress_line(b"  edit main.rs\n", 200).unwrap(),
            "edit main.rs"
        );
        let long = "é".repeat(300);
        let line = progress_line(long.as_bytes(), 201).unwrap();
        assert_eq!(line.len(), 200);
    }

    #[test]
    fn a_short_reply_is_all_of_the_output() {
        assert_eq!(
            reply_text(b"\nline one\nline two\n", 100),
            "line one\nline two"
        );
    }

    #[test]
    fn a_long_reply_keeps_its_end_from_a_line_start() {
        let lines: Vec<String> = (0..100).map(|n| format!("line {n}")).collect();
        let output = lines.join("\n");
        let reply = reply_text(output.as_bytes(), 120);
        assert!(reply.len() <= 120, "{}", reply.len());
        assert!(reply.starts_with(LONG_OUTPUT), "{reply}");
        assert!(reply.ends_with("line 99"), "{reply}");
        let first = reply.lines().nth(2).unwrap();
        assert!(first.starts_with("line "), "{first}");
    }

    #[test]
    fn the_tail_keeps_the_last_bytes_and_counts_all_of_them() {
        let mut tail = Tail::default();
        for _ in 0..10 {
            tail.push(b"0123456789", 15);
        }
        assert_eq!(tail.total, 100);
        assert!(tail.bytes().len() <= 30);
        assert!(tail.bytes().ends_with(b"0123456789"));
    }

    #[test]
    fn lines_across_chunks_come_whole_and_a_long_line_keeps_its_start() {
        let mut lines = Lines::default();
        assert!(lines.push(b"ab", 4).is_empty());
        assert_eq!(lines.push(b"c\nde", 4), [b"abc".to_vec()]);
        assert_eq!(lines.push(b"fghij\n", 4), [b"defg".to_vec()]);
        assert_eq!(lines.push(b"end", 4), Vec::<Vec<u8>>::new());
        assert_eq!(lines.finish(), Some(b"end".to_vec()));
        assert_eq!(lines.finish(), None);
    }
}
