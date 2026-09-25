//! An agent reply in Markdown, as blocks for the game window. See `SPEC.md` 7.3.1.
//!
//! The output starts with `MARKER`. Each block is one line: `\n`, a kind byte,
//! and fields that each start with `FIELD`. A last `\n` ends the text, so the
//! addon can tell a complete last block from one that a size limit cut.

use crate::ascii::push_bytes;
use crate::inline::{Escape, find_byte, is_punct, push_code_byte, push_inline};

/// ESC cannot come from agent text: the renderer drops control bytes.
pub const MARKER: [u8; 3] = [0x1B, b'M', b'1'];
pub const FIELD: u8 = 0x1F;

pub const HEADING: u8 = b'h';
pub const PARAGRAPH: u8 = b'p';
pub const ITEM: u8 = b'l';
pub const QUOTE: u8 = b'q';
pub const CODE: u8 = b'c';
pub const ROW: u8 = b't';
pub const RULE: u8 = b'r';

const MAX_HEADING: usize = 3;
const MAX_LEVEL: usize = 4;
/// `CommonMark` allows at most 9 digits in a list number.
const MAX_DIGITS: usize = 9;

/// The block that a plain text line continues.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Nothing,
    Paragraph,
    Item,
    Quote,
}

/// Inside a code fence, every line is code until the closing fence.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Fence(u8),
}

#[derive(Clone, Copy)]
struct State {
    mode: Mode,
    open: Open,
}

fn line_end(md: &[u8], start: usize) -> usize {
    find_byte(md, start, md.len(), b'\n')
}

fn skip_spaces(md: &[u8], from: usize, end: usize) -> usize {
    let mut i = from;
    while i < end && (md[i] == b' ' || md[i] == b'\t') {
        i += 1;
    }
    i
}

/// The end of `md[start..end]` without its trailing spaces and control bytes.
fn trim_end(md: &[u8], start: usize, end: usize) -> usize {
    let mut e = end;
    while e > start && md[e - 1] <= b' ' {
        e -= 1;
    }
    e
}

fn run_len(md: &[u8], i: usize, end: usize, b: u8) -> usize {
    let mut j = i;
    while j < end && md[j] == b {
        j += 1;
    }
    j - i
}

fn is_digit(b: u8) -> bool {
    b'0' <= b && b <= b'9'
}

fn start_block(out: &mut Vec<u8>, kind: u8) {
    out.push(b'\n');
    out.push(kind);
}

fn push_level(out: &mut Vec<u8>, level: usize) {
    out.push(FIELD);
    // Both callers clamp the level to one digit.
    #[allow(clippy::cast_possible_truncation)]
    out.push(b'0' + level as u8);
}

fn push_text_field(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize, escape: Escape) {
    out.push(FIELD);
    push_inline(out, md, start, trim_end(md, start, end), escape);
}

fn min_usize(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

// Code fences

fn is_fence(md: &[u8], i: usize, end: usize) -> bool {
    i < end && (md[i] == b'`' || md[i] == b'~') && run_len(md, i, end, md[i]) >= 3
}

fn closes_fence(md: &[u8], i: usize, end: usize, fence: u8) -> bool {
    i < end && md[i] == fence && run_len(md, i, end, fence) >= 3
}

/// A code line keeps its spaces and every byte. Only `|` and control bytes change.
fn code_line(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize) {
    start_block(out, CODE);
    out.push(FIELD);
    let mut i = start;
    while i < end {
        push_code_byte(out, md[i]);
        i += 1;
    }
}

fn fence_line(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize, fence: u8) -> State {
    let i = skip_spaces(md, start, end);
    if closes_fence(md, i, end, fence) {
        return State {
            mode: Mode::Text,
            open: Open::Nothing,
        };
    }
    code_line(out, md, start, end);
    State {
        mode: Mode::Fence(fence),
        open: Open::Nothing,
    }
}

// Headings and rules

/// The count of `#` before a space or the end, or 0 for no heading.
fn heading_level(md: &[u8], i: usize, end: usize) -> usize {
    let n = run_len(md, i, end, b'#');
    let after = i + n;
    if n == 0 || n > 6 || (after < end && md[after] != b' ') {
        return 0;
    }
    n
}

fn heading(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize, n: usize) {
    start_block(out, HEADING);
    push_level(out, min_usize(n, MAX_HEADING));
    push_text_field(out, md, skip_spaces(md, i + n, end), end, Escape::Html);
}

/// Three or more of one of `-`, `*`, `_`, with only spaces between.
fn is_rule(md: &[u8], i: usize, end: usize) -> bool {
    let b = md[i];
    if b != b'-' && b != b'*' && b != b'_' {
        return false;
    }
    let mut count = 0;
    let mut j = i;
    while j < end && (md[j] == b || md[j] == b' ') {
        if md[j] == b {
            count += 1;
        }
        j += 1;
    }
    j == end && count >= 3
}

fn rule(out: &mut Vec<u8>) {
    start_block(out, RULE);
}

// List items

/// The index after `- `, `* `, `+ `, `1. `, or `1) `, or `i` for no item.
fn item_marker_end(md: &[u8], i: usize, end: usize) -> usize {
    let b = md[i];
    let mark_end = if b == b'-' || b == b'*' || b == b'+' {
        i + 1
    } else {
        number_end(md, i, end)
    };
    if mark_end == i || (mark_end < end && md[mark_end] != b' ') {
        return i;
    }
    mark_end
}

/// The index after `12.` or `12)`, or `i` for no number.
fn number_end(md: &[u8], i: usize, end: usize) -> usize {
    let digits = run_len_digits(md, i, end);
    let after = i + digits;
    if digits == 0 || digits > MAX_DIGITS || after >= end {
        return i;
    }
    if md[after] == b'.' || md[after] == b')' {
        after + 1
    } else {
        i
    }
}

fn run_len_digits(md: &[u8], i: usize, end: usize) -> usize {
    let mut j = i;
    while j < end && is_digit(md[j]) {
        j += 1;
    }
    j - i
}

/// Fields: the nesting level, the number (empty for a bullet), and the text.
fn item(out: &mut Vec<u8>, md: &[u8], line: (usize, usize), indent: usize, mark_end: usize) {
    let (i, end) = line;
    start_block(out, ITEM);
    push_level(out, min_usize(indent / 2, MAX_LEVEL));
    out.push(FIELD);
    if is_digit(md[i]) {
        push_digits(out, md, i, mark_end - 1);
    }
    push_text_field(out, md, skip_spaces(md, mark_end, end), end, Escape::Html);
}

fn push_digits(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize) {
    let mut j = start;
    while j < end {
        out.push(md[j]);
        j += 1;
    }
}

// Quotes

fn skip_quote_marks(md: &[u8], i: usize, end: usize) -> usize {
    let mut j = i;
    while j < end && (md[j] == b'>' || md[j] == b' ') {
        j += 1;
    }
    j
}

fn quote(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize, open: Open) {
    let text = skip_quote_marks(md, i, end);
    if open == Open::Quote {
        continue_text(out, md, text, end);
    } else {
        start_block(out, QUOTE);
        push_text_field(out, md, text, end, Escape::Html);
    }
}

// Tables

/// Only `|`, `-`, `:`, and spaces, with at least one `-`: `|---|:--:|`.
fn is_delimiter_row(md: &[u8], i: usize, end: usize) -> bool {
    let mut dashes = 0;
    let mut j = i;
    while j < end && (md[j] == b'|' || md[j] == b'-' || md[j] == b':' || md[j] == b' ') {
        if md[j] == b'-' {
            dashes += 1;
        }
        j += 1;
    }
    j == trim_end(md, i, end) && dashes > 0
}

/// A header row is the row just above a delimiter row.
fn next_is_delimiter(md: &[u8], next: usize) -> bool {
    if next >= md.len() {
        return false;
    }
    let end = line_end(md, next);
    let i = skip_spaces(md, next, end);
    i < end && md[i] == b'|' && is_delimiter_row(md, i, end)
}

/// The next `|` that no backslash escapes, or `end`.
fn cell_end(md: &[u8], from: usize, end: usize) -> usize {
    let mut j = from;
    while j < end && md[j] != b'|' {
        j += if md[j] == b'\\' && j + 1 < end && is_punct(md[j + 1]) {
            2
        } else {
            1
        };
    }
    j
}

/// Fields: `1` for a header row or `0`, then one field per cell.
fn row(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize, header: bool) {
    start_block(out, ROW);
    out.push(FIELD);
    out.push(if header { b'1' } else { b'0' });
    let e = trim_end(md, i, end);
    let mut s = i + 1;
    while s < e {
        let c = cell_end(md, s, e);
        push_text_field(out, md, skip_spaces(md, s, c), c, Escape::Wow);
        s = c + 1;
    }
}

// Paragraphs

fn continue_text(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize) {
    out.push(b' ');
    push_inline(out, md, i, trim_end(md, i, end), Escape::Html);
}

fn paragraph(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize, open: Open) -> Open {
    if open == Open::Paragraph || open == Open::Item {
        continue_text(out, md, i, end);
        return open;
    }
    start_block(out, PARAGRAPH);
    push_text_field(out, md, i, end, Escape::Html);
    Open::Paragraph
}

// Lines

fn text_state(open: Open) -> State {
    State {
        mode: Mode::Text,
        open,
    }
}

/// A line that starts with `|` is a table row. A delimiter row shows nothing.
fn table_line(out: &mut Vec<u8>, md: &[u8], i: usize, end: usize, next: usize) {
    if !is_delimiter_row(md, i, end) {
        row(out, md, i, end, next_is_delimiter(md, next));
    }
}

/// A line that is not in a fence. `next` is the start of the line after it.
fn text_line(out: &mut Vec<u8>, md: &[u8], line: (usize, usize), next: usize, open: Open) -> State {
    let (start, end) = line;
    let i = skip_spaces(md, start, end);
    if i == end {
        return text_state(Open::Nothing);
    }
    if is_fence(md, i, end) {
        return State {
            mode: Mode::Fence(md[i]),
            open: Open::Nothing,
        };
    }
    let level = heading_level(md, i, end);
    if level > 0 {
        heading(out, md, i, end, level);
        return text_state(Open::Nothing);
    }
    if is_rule(md, i, end) {
        rule(out);
        return text_state(Open::Nothing);
    }
    block_line(out, md, (start, i, end), next, open)
}

/// List items, quotes, table rows, and paragraphs. `line` is the start, the first
/// byte after the indent, and the end.
fn block_line(
    out: &mut Vec<u8>,
    md: &[u8],
    line: (usize, usize, usize),
    next: usize,
    open: Open,
) -> State {
    let (start, i, end) = line;
    let mark_end = item_marker_end(md, i, end);
    if mark_end > i {
        item(out, md, (i, end), i - start, mark_end);
        return text_state(Open::Item);
    }
    if md[i] == b'>' {
        quote(out, md, i, end, open);
        return text_state(Open::Quote);
    }
    if md[i] == b'|' {
        table_line(out, md, i, end, next);
        return text_state(Open::Nothing);
    }
    text_state(paragraph(out, md, i, end, open))
}

fn render_line(out: &mut Vec<u8>, md: &[u8], start: usize, end: usize, state: State) -> State {
    match state.mode {
        Mode::Fence(fence) => fence_line(out, md, start, end, fence),
        Mode::Text => text_line(out, md, (start, end), end + 1, state.open),
    }
}

/// Renders Markdown as blocks for the game window. Never fails: every byte string
/// is some Markdown.
#[must_use]
pub fn render_markdown(md: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    push_bytes(&mut out, &MARKER);
    let mut state = text_state(Open::Nothing);
    let mut start = 0;
    while start < md.len() {
        let end = line_end(md, start);
        state = render_line(&mut out, md, start, end, state);
        start = end + 1;
    }
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blocks with `|` for FIELD and no marker, for short asserts.
    fn blocks(md: &str) -> Vec<String> {
        let out = render_markdown(md.as_bytes());
        assert!(out.starts_with(&MARKER));
        assert!(out.ends_with(b"\n"));
        let body = String::from_utf8(out[MARKER.len()..].to_vec()).unwrap();
        body.split('\n')
            .filter(|l| !l.is_empty())
            .map(|l| l.replace('\x1f', "/"))
            .collect()
    }

    #[test]
    fn empty_text_is_the_marker_and_one_line_break() {
        assert_eq!(render_markdown(b""), b"\x1bM1\n");
    }

    #[test]
    fn a_heading_has_its_level_and_text() {
        assert_eq!(blocks("# Title"), ["h/1/Title"]);
        assert_eq!(blocks("### Sub  "), ["h/3/Sub"]);
    }

    #[test]
    fn deep_headings_show_as_level_three() {
        assert_eq!(blocks("###### tiny"), ["h/3/tiny"]);
    }

    #[test]
    fn a_hash_with_no_space_is_a_paragraph() {
        assert_eq!(blocks("#hashtag"), ["p/#hashtag"]);
        assert_eq!(blocks("####### seven"), ["p/####### seven"]);
    }

    #[test]
    fn lines_of_one_paragraph_join_with_a_space() {
        assert_eq!(blocks("one\ntwo\n\nthree"), ["p/one two", "p/three"]);
    }

    #[test]
    fn a_bullet_item_has_a_level_and_no_number() {
        assert_eq!(
            blocks("- a\n  * b\n    + c"),
            ["l/0//a", "l/1//b", "l/2//c"]
        );
    }

    #[test]
    fn a_numbered_item_keeps_its_number() {
        assert_eq!(
            blocks("1. first\n12) twelfth"),
            ["l/0/1/first", "l/0/12/twelfth"]
        );
    }

    #[test]
    fn a_deep_list_stops_at_the_last_level() {
        assert_eq!(blocks(&format!("{}- deep", " ".repeat(40))), ["l/4//deep"]);
    }

    #[test]
    fn a_line_under_an_item_continues_it() {
        assert_eq!(blocks("- a\n  more"), ["l/0//a more"]);
    }

    #[test]
    fn a_number_with_no_dot_is_text() {
        assert_eq!(blocks("2024 was a year"), ["p/2024 was a year"]);
        assert_eq!(blocks("1234567890. long"), ["p/1234567890. long"]);
        assert_eq!(blocks("-x"), ["p/-x"]);
    }

    #[test]
    fn a_quote_joins_its_lines() {
        assert_eq!(blocks("> a\n> b\n>> c"), ["q/a b c"]);
    }

    #[test]
    fn a_rule_is_a_block_of_its_own() {
        assert_eq!(blocks("a\n\n---\n* * *\n___"), ["p/a", "r", "r", "r"]);
    }

    #[test]
    fn a_code_fence_gives_one_block_per_line() {
        assert_eq!(
            blocks("```rust\nfn main() {\n\tx < 1 | 2\n}\n```\nafter"),
            ["c/fn main() {", "c/    x < 1 || 2", "c/}", "p/after"]
        );
    }

    #[test]
    fn a_blank_code_line_stays() {
        assert_eq!(
            render_markdown(b"~~~\na\n\nb\n~~~"),
            b"\x1bM1\nc\x1fa\nc\x1f\nc\x1fb\n"
        );
    }

    #[test]
    fn a_fence_closes_only_with_its_own_byte() {
        assert_eq!(blocks("~~~\n```\n~~~"), ["c/```"]);
    }

    #[test]
    fn an_unclosed_fence_makes_the_rest_code() {
        assert_eq!(blocks("```\n# not a heading"), ["c/# not a heading"]);
    }

    #[test]
    fn code_keeps_marks_and_markup_as_text() {
        assert_eq!(blocks("```\n**a** <b>\n```"), ["c/**a** <b>"]);
    }

    #[test]
    fn a_table_has_a_header_row_and_body_rows() {
        assert_eq!(
            blocks("| a | b |\n|---|:-:|\n| 1 | **2** |"),
            ["t/1/a/b", "t/0/1/|cffffd1002|r"]
        );
    }

    #[test]
    fn a_table_with_no_delimiter_has_no_header() {
        assert_eq!(blocks("| a | b |"), ["t/0/a/b"]);
    }

    #[test]
    fn a_row_with_no_last_pipe_keeps_its_last_cell() {
        assert_eq!(blocks("| a | b"), ["t/0/a/b"]);
    }

    #[test]
    fn empty_cells_stay() {
        assert_eq!(blocks("|||x|"), ["t/0///x"]);
    }

    #[test]
    fn an_escaped_pipe_stays_in_its_cell() {
        assert_eq!(blocks("| a \\| b | c |"), ["t/0/a || b/c"]);
    }

    #[test]
    fn table_cells_keep_markup_bytes() {
        assert_eq!(blocks("| <b> | & |"), ["t/0/<b>/&"]);
    }

    #[test]
    fn a_huge_table_row_keeps_every_cell() {
        let md = "|x".repeat(5000);
        let row = &blocks(&md)[0];
        assert_eq!(row.matches("/x").count(), 5000);
    }

    #[test]
    fn text_blocks_escape_markup() {
        assert_eq!(blocks("# <h1>&"), ["h/1/&lt;h1&gt;&amp;"]);
        assert_eq!(blocks("<script>"), ["p/&lt;script&gt;"]);
    }

    #[test]
    fn a_pipe_in_agent_text_is_doubled() {
        assert_eq!(blocks("|cffff0000 red"), ["t/0/cffff0000 red"]);
        assert_eq!(blocks("a |cffff0000 red"), ["p/a ||cffff0000 red"]);
        assert_eq!(blocks("- |Hitem|h"), ["l/0//||Hitem||h"]);
    }

    #[test]
    fn control_bytes_never_reach_a_block() {
        let out = render_markdown(b"a\x1b\x1f\x00b\r\n```\n\x1f\x1bc\r\n");
        assert_eq!(out, b"\x1bM1\np\x1fab\nc\x1fc\n");
    }

    #[test]
    fn windows_line_ends_are_dropped() {
        assert_eq!(blocks("# a\r\nb\r\n"), ["h/1/a", "p/b"]);
    }

    #[test]
    fn invalid_utf8_passes_through_as_text() {
        let out = render_markdown(&[0xFF, 0xFE]);
        assert_eq!(
            out,
            [0x1B, b'M', b'1', b'\n', b'p', FIELD, 0xFF, 0xFE, b'\n']
        );
    }
}
