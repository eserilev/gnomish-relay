//! What the agents do now, and the permission requests for the game (SPEC.md 7.3,
//! 9.3). It has its own file, `Live.lua`, so the slot body keeps its own 1 MiB bound.

use crate::ascii::{push_bytes, push_decimal};
use crate::lua::lua_string;
use crate::record::MAX_ID_LEN;
use crate::slot::{cut, keep_from, min_len};

pub const MAX_PROGRESS: usize = 30;
pub const MAX_LINES: usize = 5;
pub const MAX_LINE: usize = 200;
pub const MAX_REQUESTS: usize = 4;
pub const MAX_OPTIONS: usize = 4;
/// `popup_text` (S15) writes less, so this cut never shortens a popup.
pub const MAX_POPUP: usize = 2000;
pub const MAX_LABEL: usize = 64;

/// The last steps of one run in progress.
pub struct Progress {
    pub chat: Vec<u8>,
    pub id: u32,
    pub lines: Vec<Vec<u8>>,
}

#[derive(Clone, Copy)]
pub enum OptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

pub struct PermOption {
    pub id: Vec<u8>,
    pub kind: OptionKind,
    pub label: Vec<u8>,
}

/// One permission request. `text` is the output of `popup_text`.
pub struct Request {
    pub request: Vec<u8>,
    pub chat: Vec<u8>,
    pub id: u32,
    pub text: Vec<u8>,
    pub options: Vec<PermOption>,
}

const HEAD: [u8; 34] = *b"GnomishRelay_Live = {progress = {\n";
const PERMISSIONS: [u8; 19] = *b"}, permissions = {\n";
const TAIL: [u8; 3] = *b"}}\n";
const CHAT: [u8; 8] = *b"{chat = ";
const ID: [u8; 7] = *b", id = ";
const LINES: [u8; 11] = *b", lines = {";
const LINE_END: [u8; 2] = *b", ";
const PROGRESS_END: [u8; 4] = *b"}},\n";
const REQUEST: [u8; 11] = *b"{request = ";
const REQUEST_CHAT: [u8; 9] = *b", chat = ";
const TEXT: [u8; 9] = *b", text = ";
const OPTIONS: [u8; 14] = *b", options = {\n";
const REQUEST_END: [u8; 4] = *b"}},\n";
const OPTION: [u8; 6] = *b"{id = ";
const KIND: [u8; 9] = *b", kind = ";
const LABEL: [u8; 10] = *b", label = ";
const OPTION_END: [u8; 3] = *b"},\n";
const ALLOW_ONCE: [u8; 12] = *b"\"allow_once\"";
const ALLOW_ALWAYS: [u8; 14] = *b"\"allow_always\"";
const REJECT_ONCE: [u8; 13] = *b"\"reject_once\"";
const REJECT_ALWAYS: [u8; 15] = *b"\"reject_always\"";

fn prepare_lines(lines: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = keep_from(lines.len(), MAX_LINES);
    while i < lines.len() {
        out.push(cut(&lines[i], MAX_LINE));
        i += 1;
    }
    out
}

fn prepare_progress_one(progress: &Progress) -> Progress {
    Progress {
        chat: cut(&progress.chat, MAX_ID_LEN),
        id: progress.id,
        lines: prepare_lines(&progress.lines),
    }
}

/// Keeps the last `MAX_PROGRESS` entries and the last `MAX_LINES` lines of each.
#[must_use]
pub fn prepare_progress(progress: &[Progress]) -> Vec<Progress> {
    let mut out = Vec::new();
    let mut i = keep_from(progress.len(), MAX_PROGRESS);
    while i < progress.len() {
        out.push(prepare_progress_one(&progress[i]));
        i += 1;
    }
    out
}

fn prepare_option(option: &PermOption) -> PermOption {
    PermOption {
        id: cut(&option.id, MAX_ID_LEN),
        kind: option.kind,
        label: cut(&option.label, MAX_LABEL),
    }
}

/// Keeps the first `MAX_OPTIONS` options: agents list the common answers first.
fn prepare_options(options: &[PermOption]) -> Vec<PermOption> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < min_len(options.len(), MAX_OPTIONS) {
        out.push(prepare_option(&options[i]));
        i += 1;
    }
    out
}

fn prepare_request(request: &Request) -> Request {
    Request {
        request: cut(&request.request, MAX_ID_LEN),
        chat: cut(&request.chat, MAX_ID_LEN),
        id: request.id,
        text: cut(&request.text, MAX_POPUP),
        options: prepare_options(&request.options),
    }
}

/// Keeps the first `MAX_REQUESTS` requests: the oldest ones wait longest.
#[must_use]
pub fn prepare_requests(requests: &[Request]) -> Vec<Request> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < min_len(requests.len(), MAX_REQUESTS) {
        out.push(prepare_request(&requests[i]));
        i += 1;
    }
    out
}

fn push_kind(out: &mut Vec<u8>, kind: OptionKind) {
    match kind {
        OptionKind::AllowOnce => push_bytes(out, &ALLOW_ONCE),
        OptionKind::AllowAlways => push_bytes(out, &ALLOW_ALWAYS),
        OptionKind::RejectOnce => push_bytes(out, &REJECT_ONCE),
        OptionKind::RejectAlways => push_bytes(out, &REJECT_ALWAYS),
    }
}

fn push_lines(out: &mut Vec<u8>, lines: &[Vec<u8>]) {
    let mut i = 0;
    while i < lines.len() {
        push_bytes(out, &lua_string(&lines[i]));
        push_bytes(out, &LINE_END);
        i += 1;
    }
}

fn push_progress(out: &mut Vec<u8>, progress: &Progress) {
    push_bytes(out, &CHAT);
    push_bytes(out, &lua_string(&progress.chat));
    push_bytes(out, &ID);
    push_decimal(out, progress.id);
    push_bytes(out, &LINES);
    push_lines(out, &progress.lines);
    push_bytes(out, &PROGRESS_END);
}

fn push_progress_all(out: &mut Vec<u8>, progress: &[Progress]) {
    let mut i = 0;
    while i < progress.len() {
        push_progress(out, &progress[i]);
        i += 1;
    }
}

fn push_option(out: &mut Vec<u8>, option: &PermOption) {
    push_bytes(out, &OPTION);
    push_bytes(out, &lua_string(&option.id));
    push_bytes(out, &KIND);
    push_kind(out, option.kind);
    push_bytes(out, &LABEL);
    push_bytes(out, &lua_string(&option.label));
    push_bytes(out, &OPTION_END);
}

fn push_options(out: &mut Vec<u8>, options: &[PermOption]) {
    let mut i = 0;
    while i < options.len() {
        push_option(out, &options[i]);
        i += 1;
    }
}

fn push_request(out: &mut Vec<u8>, request: &Request) {
    push_bytes(out, &REQUEST);
    push_bytes(out, &lua_string(&request.request));
    push_bytes(out, &REQUEST_CHAT);
    push_bytes(out, &lua_string(&request.chat));
    push_bytes(out, &ID);
    push_decimal(out, request.id);
    push_bytes(out, &TEXT);
    push_bytes(out, &lua_string(&request.text));
    push_bytes(out, &OPTIONS);
    push_options(out, &request.options);
    push_bytes(out, &REQUEST_END);
}

fn push_requests(out: &mut Vec<u8>, requests: &[Request]) {
    let mut i = 0;
    while i < requests.len() {
        push_request(out, &requests[i]);
        i += 1;
    }
}

/// Every hole is an escaped string or a number, as in the slot body. Takes the
/// output of `prepare_progress` and `prepare_requests`.
#[must_use]
pub fn live_body(progress: &[Progress], requests: &[Request]) -> Vec<u8> {
    let mut out = Vec::new();
    push_bytes(&mut out, &HEAD);
    push_progress_all(&mut out, progress);
    push_bytes(&mut out, &PERMISSIONS);
    push_requests(&mut out, requests);
    push_bytes(&mut out, &TAIL);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: &[u8], kind: OptionKind, label: &[u8]) -> PermOption {
        PermOption {
            id: id.to_vec(),
            kind,
            label: label.to_vec(),
        }
    }

    fn request(name: &[u8], text: &[u8]) -> Request {
        Request {
            request: name.to_vec(),
            chat: b"c1".to_vec(),
            id: 12,
            text: text.to_vec(),
            options: vec![
                option(b"o1", OptionKind::AllowOnce, b"Allow"),
                option(b"o2", OptionKind::RejectOnce, b"Reject"),
            ],
        }
    }

    #[test]
    fn a_live_file_is_the_fixed_table_with_escaped_strings() {
        let progress = [Progress {
            chat: b"c1".to_vec(),
            id: 12,
            lines: vec![b"edit src/main.rs".to_vec(), b"$ cargo \"test\"".to_vec()],
        }];
        let requests = [request(b"p7", b"rm -rf build")];
        assert_eq!(
            live_body(&progress, &requests),
            b"GnomishRelay_Live = {progress = {\n\
              {chat = \"c1\", id = 12, lines = {\"edit src/main.rs\", \"$ cargo \\034test\\034\", }},\n\
              }, permissions = {\n\
              {request = \"p7\", chat = \"c1\", id = 12, text = \"rm -rf build\", options = {\n\
              {id = \"o1\", kind = \"allow_once\", label = \"Allow\"},\n\
              {id = \"o2\", kind = \"reject_once\", label = \"Reject\"},\n\
              }},\n}}\n"
        );
    }

    #[test]
    fn an_empty_live_file_has_no_progress_and_no_requests() {
        assert_eq!(
            live_body(&[], &[]),
            b"GnomishRelay_Live = {progress = {\n}, permissions = {\n}}\n"
        );
    }

    #[test]
    fn prepare_keeps_the_last_progress_and_the_first_requests() {
        let progress: Vec<Progress> = (0..40)
            .map(|id| Progress {
                chat: b"c1".to_vec(),
                id,
                lines: (0..9).map(|n| vec![b'a' + n; 300]).collect(),
            })
            .collect();
        let prepared = prepare_progress(&progress);
        assert_eq!(prepared.len(), MAX_PROGRESS);
        assert_eq!(prepared[0].id, 10);
        assert_eq!(prepared[0].lines.len(), MAX_LINES);
        assert_eq!(prepared[0].lines[0], vec![b'e'; MAX_LINE]);

        let requests: Vec<Request> = (0..6).map(|n| request(&[b'p', b'0' + n], b"x")).collect();
        let prepared = prepare_requests(&requests);
        assert_eq!(prepared.len(), MAX_REQUESTS);
        assert_eq!(prepared[0].request, b"p0");
    }

    #[test]
    fn prepare_cuts_every_string_to_its_limit() {
        let long = vec![b'a'; 5000];
        let mut big = request(&long, &long);
        big.chat.clone_from(&long);
        big.options = (0..6)
            .map(|_| option(&long, OptionKind::AllowAlways, &long))
            .collect();
        let r = &prepare_requests(&[big])[0];
        assert_eq!(
            [r.request.len(), r.chat.len(), r.text.len(), r.options.len()],
            [MAX_ID_LEN, MAX_ID_LEN, MAX_POPUP, MAX_OPTIONS]
        );
        assert_eq!(
            [r.options[0].id.len(), r.options[0].label.len()],
            [MAX_ID_LEN, MAX_LABEL]
        );
    }
}
