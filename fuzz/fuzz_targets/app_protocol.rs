//! Any line of the story program, and any batch of the addon (SPEC.md 9.8): reading
//! never panics, a forwarded line is JSON, and a reply for the game is one JSON line
//! with every `|` doubled that the slot writer keeps whole.
#![no_main]

use bridge::addon_lines::{forwarded_line, read_batch};
use bridge::app_protocol::{FromStory, MAX_ANSWER_LINE, MAX_PROMPT, RequestId, read_line, reply_text};
use libfuzzer_sys::fuzz_target;
use protocol::slot::{Reply, Status, prepare_replies};

fn check_batch(text: &str) {
    let Ok(batch) = read_batch(text) else {
        return;
    };
    let replies = batch.lines.iter().filter(|l| l.wants_reply()).count();
    assert!(replies <= 1, "one line with a reply at most");
    for line in &batch.lines {
        let forwarded = forwarded_line(RequestId(1), line);
        assert!(forwarded.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(&forwarded).unwrap();
        assert_eq!(value["id"], 1);
    }
}

fn check_reply(reply: &str) {
    assert!(serde_json::from_str::<serde_json::Value>(reply).is_ok());
    assert!(!reply.contains('\n'), "one line");
    let odd_pipes = reply
        .split(|c| c != '|')
        .any(|run| run.len() % 2 == 1);
    assert!(!odd_pipes, "S10: every | is doubled");
    let slot = Reply {
        chat: Vec::new(),
        id: 0,
        status: Status::Done,
        text: reply.as_bytes().to_vec(),
    };
    assert_eq!(prepare_replies(&[slot])[0].text.len(), reply.len(), "S12 keeps it whole");
}

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        check_batch(text);
    }
    match read_line(data) {
        Ok(FromStory::Answer {
            answer: Some(answer),
            ..
        }) => {
            assert!(data.len() <= MAX_ANSWER_LINE);
            if let Some(reply) = reply_text(&answer, Some("note")) {
                check_reply(&reply);
            }
        }
        Ok(FromStory::ModelCall { prompt, .. }) => assert!(prompt.len() <= MAX_PROMPT),
        _ => {}
    }
});
