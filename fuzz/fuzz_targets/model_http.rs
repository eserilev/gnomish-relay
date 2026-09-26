//! Any answer of a local model (SPEC.md 9.7, decision 10): reading never panics, and the
//! text that goes to the story program is short and has no control character but a
//! newline and a tab. A `model_answered` line with it is one line of JSON.
#![no_main]

use bridge::app_protocol::{CallId, model_answered_line};
use bridge::model::{MAX_ANSWER, clean_answer};
use bridge::model_local::read_answer;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = read_answer(data) else {
        return;
    };
    let clean = clean_answer(&text);
    assert!(clean.len() <= MAX_ANSWER);
    assert!(clean.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'));
    let line = model_answered_line(CallId(1), &clean);
    assert_eq!(line.matches('\n').count(), 1, "one line");
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["text"], clean.as_str());
});
