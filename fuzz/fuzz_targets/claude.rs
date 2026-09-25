//! Any output line of `claude -p` and any session file of Claude Code: reading them
//! never panics, a progress line stays short, a popup text is printable, and a copy
//! of a session never keeps an old id.
#![no_main]

use bridge::claude::{Message, read_message, tool_call};
use bridge::claude_sessions::{fork_entries, last_exchange, read_info};
use libfuzzer_sys::fuzz_target;

const OLD: &str = "0b6ad9d2-1f2e-4c55-9a7e-2b1f4e6c8d01";

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for line in text.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match read_message(&message) {
            Message::Said { steps, .. } => assert!(steps.iter().all(|s| s.len() <= 200)),
            Message::Ask { request, .. } | Message::Hook { request: Some(request), .. } => {
                assert!(request.text.iter().all(|b| *b == b'\n' || (b' '..=b'~').contains(b)), "S15");
                assert!(request.title.len() <= 200);
                let _ = tool_call(&request, std::path::Path::new("/w"));
            }
            _ => {}
        }
    }
    let _ = read_info(&text, &text, || None);
    let _ = last_exchange(&text);
    let mut n = 0;
    let fresh = || {
        n += 1;
        Ok(format!("new-{n}"))
    };
    if let Ok(entries) = fork_entries(&text, OLD, "new", None, "now", fresh) {
        for entry in entries {
            assert_eq!(entry["sessionId"], "new");
            let uuid = entry.get("uuid").and_then(|u| u.as_str()).unwrap_or("new-");
            assert!(uuid.starts_with("new-"), "{uuid}");
        }
    }
});
