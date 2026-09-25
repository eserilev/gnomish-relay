//! The body of a slot file. See `SPEC.md` 7.3.

use crate::apps::{App, push_slot_global};
use crate::ascii::{push_bytes, push_decimal, push_range};
use crate::lua::{is_plain, lua_string};
use crate::record::MAX_ID_LEN;

/// Slot addons per UI session. The bridge makes this many at setup.
pub const SLOTS: usize = 1000;
/// The slots that one publish writes, from the next slot that the addon reported.
pub const SLOT_WINDOW: usize = 30;
pub const MAX_REPLIES: usize = 30;
pub const MAX_TEXT: usize = 32_768;
pub const SLOT_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub enum Status {
    Working,
    Done,
    Error,
}

pub struct Reply {
    pub chat: Vec<u8>,
    pub id: u32,
    pub status: Status,
    pub text: Vec<u8>,
}

const HEAD: [u8; 21] = *b" = {proto = 1, now = ";
const REPLIES: [u8; 14] = *b", replies = {\n";
const TAIL: [u8; 3] = *b"}}\n";
const CHAT: [u8; 8] = *b"{chat = ";
const ID: [u8; 7] = *b", id = ";
const STATUS: [u8; 11] = *b", status = ";
const TEXT: [u8; 9] = *b", text = ";
const REPLY_END: [u8; 3] = *b"},\n";
const WORKING: [u8; 9] = *b"\"working\"";
const DONE: [u8; 6] = *b"\"done\"";
const ERROR: [u8; 7] = *b"\"error\"";

/// Bytes that `lua_string` writes for `b`.
fn escaped_len(b: u8) -> usize {
    if is_plain(b) { 1 } else { 4 }
}

fn next_fits(text: &[u8], i: usize, size: usize) -> bool {
    i < text.len() && size + escaped_len(text[i]) <= MAX_TEXT
}

/// The length of the longest prefix of `text` whose Lua literal fits in `MAX_TEXT`.
fn fitting_prefix(text: &[u8]) -> usize {
    // The two quotes of the literal.
    let mut size = 2;
    let mut i = 0;
    while next_fits(text, i, size) {
        size += escaped_len(text[i]);
        i += 1;
    }
    i
}

pub(crate) fn min_len(n: usize, max: usize) -> usize {
    if n > max { max } else { n }
}

/// The first `max` bytes. The bridge cuts at a character boundary first, so this
/// cut only keeps a bound.
pub(crate) fn cut(bytes: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::new();
    push_range(&mut out, bytes, 0, min_len(bytes.len(), max));
    out
}

/// The first index of the last `max` items of a list of `len`.
#[allow(clippy::implicit_saturating_sub)] // Aeneas has no model for `saturating_sub`
pub(crate) fn keep_from(len: usize, max: usize) -> usize {
    if len > max { len - max } else { 0 }
}

#[allow(clippy::implicit_saturating_sub)] // Aeneas has no model for `saturating_sub`
fn first_kept(len: usize) -> usize {
    if len > MAX_REPLIES {
        len - MAX_REPLIES
    } else {
        0
    }
}

fn prepare_reply(reply: &Reply) -> Reply {
    let mut chat = Vec::new();
    push_range(
        &mut chat,
        &reply.chat,
        0,
        min_len(reply.chat.len(), MAX_ID_LEN),
    );
    let mut text = Vec::new();
    push_range(&mut text, &reply.text, 0, fitting_prefix(&reply.text));
    Reply {
        chat,
        id: reply.id,
        status: reply.status,
        text,
    }
}

/// Keeps the last `MAX_REPLIES` replies. Cuts each text so that its Lua literal
/// fits in `MAX_TEXT` bytes, and each chat id to `MAX_ID_LEN` bytes.
#[must_use]
pub fn prepare_replies(replies: &[Reply]) -> Vec<Reply> {
    let mut out = Vec::new();
    let mut i = first_kept(replies.len());
    while i < replies.len() {
        out.push(prepare_reply(&replies[i]));
        i += 1;
    }
    out
}

fn push_status(out: &mut Vec<u8>, status: Status) {
    match status {
        Status::Working => push_bytes(out, &WORKING),
        Status::Done => push_bytes(out, &DONE),
        Status::Error => push_bytes(out, &ERROR),
    }
}

fn push_reply(out: &mut Vec<u8>, reply: &Reply) {
    push_bytes(out, &CHAT);
    push_bytes(out, &lua_string(&reply.chat));
    push_bytes(out, &ID);
    push_decimal(out, reply.id);
    push_bytes(out, &STATUS);
    push_status(out, reply.status);
    push_bytes(out, &TEXT);
    push_bytes(out, &lua_string(&reply.text));
    push_bytes(out, &REPLY_END);
}

/// Every hole is an escaped string or a number, so a reply cannot change the
/// shape of the table. Takes the output of `prepare_replies`.
#[must_use]
pub fn slot_body(app: App, now: u32, replies: &[Reply]) -> Vec<u8> {
    let mut out = Vec::new();
    push_slot_global(&mut out, app);
    push_bytes(&mut out, &HEAD);
    push_decimal(&mut out, now);
    push_bytes(&mut out, &REPLIES);
    let mut i = 0;
    while i < replies.len() {
        push_reply(&mut out, &replies[i]);
        i += 1;
    }
    push_bytes(&mut out, &TAIL);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(chat: &[u8], id: u32, text: &[u8]) -> Reply {
        Reply {
            chat: chat.to_vec(),
            id,
            status: Status::Done,
            text: text.to_vec(),
        }
    }

    #[test]
    fn a_body_is_the_fixed_table_with_escaped_strings() {
        let body = slot_body(
            App::Relay,
            1_790_211_079,
            &[reply(b"c1", 12, b"hi \"there\"")],
        );
        assert_eq!(
            body,
            b"GnomishRelay_SlotData = {proto = 1, now = 1790211079, replies = {\n\
              {chat = \"c1\", id = 12, status = \"done\", text = \"hi \\034there\\034\"},\n}}\n"
        );
    }

    #[test]
    fn an_empty_body_has_no_replies() {
        assert_eq!(
            slot_body(App::Relay, 0, &[]),
            b"GnomishRelay_SlotData = {proto = 1, now = 0, replies = {\n}}\n"
        );
    }

    #[test]
    fn a_timeways_body_sets_the_timeways_global() {
        assert_eq!(
            slot_body(App::Timeways, 0, &[]),
            b"Timeways_SlotData = {proto = 1, now = 0, replies = {\n}}\n"
        );
    }

    #[test]
    fn a_reply_cannot_end_the_table() {
        let body = slot_body(App::Relay, 0, &[reply(b"c", 1, b"\"}} os.exit() --")]);
        assert!(body.ends_with(b"text = \"\\034}} os.exit() --\"},\n}}\n"));
    }

    #[test]
    fn only_the_last_30_replies_are_kept() {
        let replies: Vec<Reply> = (0..40).map(|id| reply(b"c", id, b"t")).collect();
        let kept = prepare_replies(&replies);
        assert_eq!(kept.len(), 30);
        assert_eq!(kept[0].id, 10);
        assert_eq!(kept[29].id, 39);
    }

    #[test]
    fn a_long_chat_id_is_cut_to_32_bytes() {
        let kept = prepare_replies(&[reply(&[b'c'; 50], 1, b"")]);
        assert_eq!(kept[0].chat, vec![b'c'; 32]);
    }

    #[test]
    fn a_long_plain_text_is_cut_so_its_literal_fits() {
        let kept = prepare_replies(&[reply(b"c", 1, &vec![b'a'; 40_000])]);
        assert_eq!(kept[0].text.len(), MAX_TEXT - 2);
        assert_eq!(lua_string(&kept[0].text).len(), MAX_TEXT);
    }

    #[test]
    fn escaped_bytes_count_four_times() {
        let kept = prepare_replies(&[reply(b"c", 1, &[b'\n'; 10_000])]);
        assert_eq!(kept[0].text.len(), (MAX_TEXT - 2) / 4);
        assert!(lua_string(&kept[0].text).len() <= MAX_TEXT);
    }

    #[test]
    fn a_short_reply_is_kept_whole() {
        let kept = prepare_replies(&[reply(b"c1", 7, b"hello")]);
        assert_eq!(kept[0].chat, b"c1");
        assert_eq!(kept[0].id, 7);
        assert_eq!(kept[0].text, b"hello");
    }

    #[test]
    fn a_full_body_stays_under_the_limit() {
        let replies: Vec<Reply> = (0..40)
            .map(|id| reply(&[0xFF; 50], u32::MAX - id, &vec![0xFF; 40_000]))
            .collect();
        let body = slot_body(App::Timeways, u32::MAX, &prepare_replies(&replies));
        assert!(body.len() <= SLOT_BODY_LIMIT);
    }
}
