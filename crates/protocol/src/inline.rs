//! Inline Markdown spans: bold, italic, inline code, and links. WoW has no bold
//! inside a line, so each span becomes a color code. See `SPEC.md` 7.3.1.
//!
//! The color codes here are the only `|` that reach WoW unescaped. Every `|` of
//! the agent text is doubled.

use crate::ascii::push_bytes;

/// Where a text goes. A `SimpleHTML` frame reads `<`, `>`, and `&` as markup.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    Html,
    Wow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Off,
    On,
}

#[derive(Clone, Copy)]
struct Style {
    bold: Mark,
    italic: Mark,
}

// Arrays, not `&[u8]`: Aeneas cannot translate a reference stored in a constant.
pub const BOLD: [u8; 10] = *b"|cffffd100";
pub const ITALIC: [u8; 10] = *b"|cffc0c8ff";
pub const BOLD_ITALIC: [u8; 10] = *b"|cffffe680";
pub const CODE: [u8; 10] = *b"|cffb8e0b8";
pub const LINK: [u8; 10] = *b"|cff69b4ff";
pub const RESET: [u8; 2] = *b"|r";
const LT: [u8; 4] = *b"&lt;";
const GT: [u8; 4] = *b"&gt;";
const AMP: [u8; 5] = *b"&amp;";
const TAB: [u8; 4] = *b"    ";

const PLAIN: Style = Style {
    bold: Mark::Off,
    italic: Mark::Off,
};

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_alnum(b: u8) -> bool {
    (b'a' <= b && b <= b'z') || (b'A' <= b && b <= b'Z') || (b'0' <= b && b <= b'9')
}

pub(crate) fn is_punct(b: u8) -> bool {
    (b'!' <= b && b <= b'~') && !is_alnum(b)
}

fn is_control(b: u8) -> bool {
    b < b' ' || b == 0x7F
}

fn push_visible(out: &mut Vec<u8>, b: u8, escape: Escape) {
    if escape == Escape::Html {
        push_html(out, b);
    } else {
        out.push(b);
    }
}

fn push_html(out: &mut Vec<u8>, b: u8) {
    if b == b'<' {
        push_bytes(out, &LT);
    } else if b == b'>' {
        push_bytes(out, &GT);
    } else if b == b'&' {
        push_bytes(out, &AMP);
    } else {
        out.push(b);
    }
}

/// A tab becomes a space, and other control bytes go: they would split a block.
pub fn push_text_byte(out: &mut Vec<u8>, b: u8, escape: Escape) {
    if b == b'|' {
        out.push(b'|');
        out.push(b'|');
    } else if b == b'\t' {
        out.push(b' ');
    } else if !is_control(b) {
        push_visible(out, b, escape);
    }
}

/// A code line keeps its tab as four spaces, so indentation lines up.
pub fn push_code_byte(out: &mut Vec<u8>, b: u8) {
    if b == b'\t' {
        push_bytes(out, &TAB);
    } else {
        push_text_byte(out, b, Escape::Wow);
    }
}

fn push_text_range(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize, escape: Escape) {
    let mut i = start;
    while i < end {
        push_text_byte(out, md[i], escape);
        i += 1;
    }
}

fn open_color(out: &mut Vec<u8>, style: Style) {
    match (style.bold, style.italic) {
        (Mark::On, Mark::On) => push_bytes(out, &BOLD_ITALIC),
        (Mark::On, Mark::Off) => push_bytes(out, &BOLD),
        (Mark::Off, Mark::On) => push_bytes(out, &ITALIC),
        (Mark::Off, Mark::Off) => {}
    }
}

fn close_color(out: &mut Vec<u8>, style: Style) {
    if style.bold == Mark::On || style.italic == Mark::On {
        push_bytes(out, &RESET);
    }
}

/// The count of `b` bytes from `i` on, up to `end`.
fn run_len(md: &[u8], i: usize, end: usize, b: u8) -> usize {
    let mut j = i;
    while j < end && md[j] == b {
        j += 1;
    }
    j - i
}

/// The start of the next run of exactly `n` backticks, or `end` if none.
fn find_tick_run(md: &[u8], from: usize, end: usize, n: usize) -> usize {
    let mut j = from;
    while j < end {
        let len = run_len(md, j, end, b'`');
        if len == n {
            return j;
        }
        j += if len == 0 { 1 } else { len };
    }
    end
}

/// The next `b` at or after `from`, or `end` if none.
pub(crate) fn find_byte(md: &[u8], from: usize, end: usize, b: u8) -> usize {
    let mut j = from;
    while j < end && md[j] != b {
        j += 1;
    }
    j
}

fn flip(mark: Mark) -> Mark {
    match mark {
        Mark::On => Mark::Off,
        Mark::Off => Mark::On,
    }
}

/// The marks that a run of `n` delimiters toggles: 1 italic, 2 bold, 3 both.
fn toggled(style: Style, n: usize) -> Style {
    if n == 1 {
        Style {
            bold: style.bold,
            italic: flip(style.italic),
        }
    } else if n == 2 {
        Style {
            bold: flip(style.bold),
            italic: style.italic,
        }
    } else {
        Style {
            bold: flip(style.bold),
            italic: flip(style.italic),
        }
    }
}

/// The marks that a run of `n` delimiters touches are all off.
fn opens(style: Style, n: usize) -> bool {
    if n == 1 {
        style.italic == Mark::Off
    } else if n == 2 {
        style.bold == Mark::Off
    } else {
        style.bold == Mark::Off && style.italic == Mark::Off
    }
}

/// The marks that a run of `n` delimiters touches are all on.
fn closes(style: Style, n: usize) -> bool {
    if n == 1 {
        style.italic == Mark::On
    } else if n == 2 {
        style.bold == Mark::On
    } else {
        style.bold == Mark::On && style.italic == Mark::On
    }
}

fn byte_before(md: &[u8], start: usize, i: usize) -> u8 {
    if i > start { md[i - 1] } else { b' ' }
}

fn byte_at(md: &[u8], end: usize, i: usize) -> u8 {
    if i < end { md[i] } else { b' ' }
}

/// A `_` inside a word, as in `snake_case`, is a letter, not a mark.
fn can_open(md: &[u8], start: usize, end: usize, i: usize, n: usize) -> bool {
    let after = byte_at(md, end, i + n);
    let word = md[i] == b'_' && is_alnum(byte_before(md, start, i));
    !is_space(after) && !word
}

fn can_close(md: &[u8], start: usize, end: usize, i: usize, n: usize) -> bool {
    let word = md[i] == b'_' && is_alnum(byte_at(md, end, i + n));
    !is_space(byte_before(md, start, i)) && !word
}

/// A later run of `n` delimiters that can close the run at `i`. An opener with
/// none is text.
fn has_closer(md: &[u8], start: usize, end: usize, i: usize, n: usize) -> bool {
    let d = md[i];
    let mut j = i + n;
    while j < end {
        if run_len(md, j, end, d) >= n && can_close(md, start, end, j, n) {
            return true;
        }
        j += 1;
    }
    false
}

fn toggles(md: &[u8], start: usize, end: usize, i: usize, n: usize, style: Style) -> bool {
    if opens(style, n) {
        can_open(md, start, end, i, n) && has_closer(md, start, end, i, n)
    } else if closes(style, n) {
        can_close(md, start, end, i, n)
    } else {
        false
    }
}

fn min3(n: usize) -> usize {
    if n > 3 { 3 } else { n }
}

/// A run of `*` or `_`. Returns the index after it and the new style.
fn emphasis(
    out: &mut Vec<u8>,
    md: &[u8],
    span: (usize, usize),
    i: usize,
    style: Style,
    escape: Escape,
) -> (usize, Style) {
    let (start, end) = span;
    let n = min3(run_len(md, i, end, md[i]));
    if !toggles(md, start, end, i, n, style) {
        push_text_range(out, md, i, i + n, escape);
        return (i + n, style);
    }
    let next = toggled(style, n);
    close_color(out, style);
    open_color(out, next);
    (i + n, next)
}

/// Writes `md[start..end]` in `color`, then goes back to the colors of `style`.
fn push_colored(
    out: &mut Vec<u8>,
    md: &[u8],
    range: (usize, usize),
    color: &[u8; 10],
    style: Style,
    escape: Escape,
) {
    close_color(out, style);
    push_bytes(out, color);
    push_text_range(out, md, range.0, range.1, escape);
    push_bytes(out, &RESET);
    open_color(out, style);
}

/// Inline code shows its bytes with no marks. A run with no closing run is text.
fn code_span(
    out: &mut Vec<u8>,
    md: &[u8],
    end: usize,
    i: usize,
    style: Style,
    escape: Escape,
) -> usize {
    let n = run_len(md, i, end, b'`');
    let close = find_tick_run(md, i + n, end, n);
    if close == end {
        push_text_range(out, md, i, i + n, escape);
        return i + n;
    }
    push_colored(out, md, (i + n, close), &CODE, style, escape);
    close + n
}

/// `[text](target)` shows its text. WoW cannot open a browser, so the target goes.
fn link(out: &mut Vec<u8>, md: &[u8], end: usize, i: usize, style: Style, escape: Escape) -> usize {
    let close = find_byte(md, i + 1, end, b']');
    let target = close + 1;
    let open = target < end && md[target] == b'(';
    let paren = find_byte(md, target, end, b')');
    if !open || paren == end {
        push_text_byte(out, b'[', escape);
        return i + 1;
    }
    push_colored(out, md, (i + 1, close), &LINK, style, escape);
    paren + 1
}

/// A backslash before a punctuation byte makes it text.
fn escaped(out: &mut Vec<u8>, md: &[u8], end: usize, i: usize, escape: Escape) -> usize {
    if i + 1 < end && is_punct(md[i + 1]) {
        push_text_byte(out, md[i + 1], escape);
        return i + 2;
    }
    push_text_byte(out, b'\\', escape);
    i + 1
}

fn step(
    out: &mut Vec<u8>,
    md: &[u8],
    span: (usize, usize),
    i: usize,
    style: Style,
    escape: Escape,
) -> (usize, Style) {
    let end = span.1;
    let b = md[i];
    if b == b'\\' {
        (escaped(out, md, end, i, escape), style)
    } else if b == b'`' {
        (code_span(out, md, end, i, style, escape), style)
    } else if b == b'*' || b == b'_' {
        emphasis(out, md, span, i, style, escape)
    } else if b == b'[' {
        (link(out, md, end, i, style, escape), style)
    } else {
        push_text_byte(out, b, escape);
        (i + 1, style)
    }
}

/// Writes `md[start..end]` with its spans as color codes. Every color it opens,
/// it closes, so no color leaks into the next text.
pub fn push_inline(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize, escape: Escape) {
    let mut style = PLAIN;
    let mut i = start;
    while i < end {
        let (next, next_style) = step(out, md, (start, end), i, style, escape);
        i = next;
        style = next_style;
    }
    close_color(out, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(md: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_inline(&mut out, md, 0, md.len(), Escape::Html);
        out
    }

    fn wow(md: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_inline(&mut out, md, 0, md.len(), Escape::Wow);
        out
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(html(b"all tests pass"), b"all tests pass");
    }

    #[test]
    fn bold_becomes_a_gold_color() {
        assert_eq!(html(b"a **b** c"), b"a |cffffd100b|r c");
    }

    #[test]
    fn underscores_make_bold_too() {
        assert_eq!(html(b"__b__"), b"|cffffd100b|r");
    }

    #[test]
    fn italic_becomes_a_light_color() {
        assert_eq!(html(b"*i*"), b"|cffc0c8ffi|r");
    }

    #[test]
    fn three_stars_make_bold_italic() {
        assert_eq!(html(b"***x***"), b"|cffffe680x|r");
    }

    #[test]
    fn italic_inside_bold_switches_the_color_and_back() {
        assert_eq!(
            html(b"**a *b* c**"),
            b"|cffffd100a |r|cffffe680b|r|cffffd100 c|r"
        );
    }

    #[test]
    fn an_unclosed_mark_is_text() {
        assert_eq!(html(b"**never closed"), b"**never closed");
    }

    #[test]
    fn a_star_between_spaces_is_text() {
        assert_eq!(html(b"2 * 3 * 4"), b"2 * 3 * 4");
    }

    #[test]
    fn an_underscore_inside_a_word_is_text() {
        assert_eq!(html(b"my_long_name"), b"my_long_name");
    }

    #[test]
    fn a_closer_after_a_space_is_text() {
        assert_eq!(html(b"*a *b"), b"*a *b");
    }

    #[test]
    fn a_mark_with_no_closer_left_is_text() {
        assert_eq!(html(b"**a**b**"), b"|cffffd100a|rb**");
    }

    #[test]
    fn a_color_open_at_the_end_is_closed() {
        assert_eq!(html(b"*a**"), b"|cffc0c8ffa**|r");
    }

    #[test]
    fn inline_code_shows_its_bytes_with_no_marks() {
        assert_eq!(html(b"run `a **b**`"), b"run |cffb8e0b8a **b**|r");
    }

    #[test]
    fn inline_code_with_two_ticks_can_hold_one() {
        assert_eq!(html(b"``a`b``"), b"|cffb8e0b8a`b|r");
    }

    #[test]
    fn inline_code_inside_bold_comes_back_to_bold() {
        assert_eq!(
            html(b"**a `c` b**"),
            b"|cffffd100a |r|cffb8e0b8c|r|cffffd100 b|r"
        );
    }

    #[test]
    fn an_unclosed_tick_is_text() {
        assert_eq!(html(b"a ` b"), b"a ` b");
    }

    #[test]
    fn a_link_shows_its_text_and_drops_the_target() {
        assert_eq!(
            html(b"see [the docs](https://x.y/z)."),
            b"see |cff69b4ffthe docs|r."
        );
    }

    #[test]
    fn a_bracket_with_no_target_is_text() {
        assert_eq!(html(b"[x] done"), b"[x] done");
        assert_eq!(html(b"[x](no end"), b"[x](no end");
    }

    #[test]
    fn a_backslash_makes_a_mark_text() {
        assert_eq!(html(b"\\*a\\*"), b"*a*");
        assert_eq!(html(b"a\\b"), b"a\\b");
    }

    #[test]
    fn a_pipe_is_doubled_everywhere() {
        assert_eq!(html(b"a|b"), b"a||b");
        assert_eq!(html(b"`|cff`"), b"|cffb8e0b8||cff|r");
        assert_eq!(html(b"[|H](x)"), b"|cff69b4ff||H|r");
    }

    #[test]
    fn html_text_escapes_markup() {
        assert_eq!(html(b"<b>&"), b"&lt;b&gt;&amp;");
        assert_eq!(html(b"`<i>`"), b"|cffb8e0b8&lt;i&gt;|r");
    }

    #[test]
    fn wow_text_keeps_markup_bytes() {
        assert_eq!(wow(b"<b>&"), b"<b>&");
    }

    #[test]
    fn a_tab_becomes_a_space_and_control_bytes_go() {
        assert_eq!(html(b"a\tb\x1b\x1f\x00\x7fc"), b"a bc");
    }

    #[test]
    fn a_code_byte_keeps_a_tab_as_four_spaces() {
        let mut out = Vec::new();
        push_code_byte(&mut out, b'\t');
        push_code_byte(&mut out, b'|');
        push_code_byte(&mut out, b'<');
        assert_eq!(out, b"    ||<");
    }

    #[test]
    fn invalid_utf8_passes_through() {
        assert_eq!(html(&[0xFF, b'a', 0xC3]), [0xFF, b'a', 0xC3]);
    }
}
