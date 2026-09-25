//! Any message of `codex app-server`: reading it never panics, a progress line stays
//! short, and a popup text is printable.
#![no_main]

use bridge::codex::{
    Event, approval_call, last_exchange, read_event, read_request, read_threads, unwrap_shell,
};
use libfuzzer_sys::fuzz_target;

const METHODS: [&str; 4] = [
    "item/started",
    "item/completed",
    "turn/completed",
    "item/fileChange/requestApproval",
];

fuzz_target!(|data: &[u8]| {
    let Ok(message) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    for method in METHODS {
        if let Event::Started { step, .. } = read_event(method, &message) {
            assert!(step.len() <= 200);
        }
    }
    for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
        let request = read_request(method, &message, &["src/a.rs".to_owned()]);
        assert!(request.text.iter().all(|b| *b == b'\n' || (b' '..=b'~').contains(b)), "S15");
        assert!(request.title.len() <= 200);
        let _ = approval_call(method, &message, &["a.rs".to_owned()], std::path::Path::new("/w"));
    }
    if let Some(command) = message.get("command").and_then(|c| c.as_str()) {
        let _ = unwrap_shell(command);
    }
    let _ = read_threads(&message);
    let _ = last_exchange(&message);
});
