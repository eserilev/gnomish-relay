//! Any message from an agent: reading an update or a permission request, and building
//! its classifier input, never panics, a progress line stays short, and the game never sees "allow always".
#![no_main]

use bridge::acp::{Update, read_request, read_update, request_call};
use libfuzzer_sys::fuzz_target;
use protocol::live::{MAX_OPTIONS, OptionKind};

fuzz_target!(|data: &[u8]| {
    let Ok(params) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    if let Update::Step(line) = read_update(&params) {
        assert!(line.len() <= 200);
    }
    let request = read_request(&params);
    assert!(request.options.len() <= MAX_OPTIONS);
    assert!(request.options.iter().all(|(_, kind, _)| !matches!(kind, OptionKind::AllowAlways)));
    assert!(request.text.iter().all(|b| *b == b'\n' || (b' '..=b'~').contains(b)), "S15");
    let _ = request_call(&params, std::path::Path::new("/w"));
});
