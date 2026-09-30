//! Spool bytes to an event or a refusal, then the session table (SPEC.md 10.7). No input
//! panics the reader, each accepted text passes S40, and a sequence of files keeps the
//! bounds of S41. A zero byte ends one file, so one input is a sequence of files.
#![no_main]

use std::path::Path;

use bridge::spool::SpoolFile;
use bridge::terminal_sessions::TerminalSessions;
use libfuzzer_sys::fuzz_target;
use protocol::live::MAX_NOTICES;
use protocol::notice::{MAX_REPO, MAX_TEXT};
use protocol::sessions::MAX_SESSIONS;

/// No control byte, and each `|` only as `||` (S40).
fn is_safe(text: &[u8]) -> bool {
    let no_controls = text.iter().all(|b| *b >= 0x20 && *b != 0x7F);
    let pipes_doubled = text
        .split(|b| *b == b'|')
        .skip(1)
        .step_by(2)
        .all(<[u8]>::is_empty);
    let pipes_even = text.iter().filter(|b| **b == b'|').count() % 2 == 0;
    no_controls && pipes_doubled && pipes_even
}

fuzz_target!(|data: &[u8]| {
    // No file on this path, so the table starts empty and nothing touches the disk.
    let mut now = 1_790_000_000u32;
    let (mut sessions, _) = TerminalSessions::load(Path::new("/nonexistent/gnomish-relay"), now);
    for bytes in data.split(|b| *b == 0) {
        now += 7;
        let Ok(file) = SpoolFile::parse(bytes) else {
            continue;
        };
        sessions.apply(&file, now);
        sessions.expire(now);
        let notices = sessions.notices();
        assert!(notices.open as usize <= MAX_SESSIONS);
        assert!(notices.busy <= notices.open);
        assert!(notices.list.len() <= MAX_NOTICES);
        for n in &notices.list {
            assert!(n.repo.len() <= MAX_REPO && n.text.len() <= MAX_TEXT);
            assert!(is_safe(&n.repo) && is_safe(&n.text));
        }
    }
});
