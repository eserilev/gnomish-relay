//! Any line of the link, on both sides (SPEC.md 11.7.11): no line panics a half, and a
//! line that parses writes back as the same message.
#![no_main]

use bridge::link::{
    self, ToGame, ToServer, check_publish, check_request, read_line, read_records, read_tag,
};
use libfuzzer_sys::fuzz_target;

fn check_game_side(line: &str) {
    let Ok(message) = serde_json::from_str::<ToGame>(line) else {
        return;
    };
    match &message {
        ToGame::Publish(publish) => {
            let _ = check_publish(publish);
            let _ = link::unhex(&publish.body);
        }
        ToGame::Request(request) => {
            let _ = check_request(request);
        }
        _ => {}
    }
    let again = link::line_of(&message).unwrap();
    assert_eq!(serde_json::from_str::<ToGame>(again.trim_end()).unwrap(), message);
}

fn check_server_side(line: &str) {
    let Ok(message) = serde_json::from_str::<ToServer>(line) else {
        return;
    };
    if let ToServer::Records(records) = &message {
        let _ = read_tag(&records.tag);
        let _ = read_records(&records.records);
    }
    let again = link::line_of(&message).unwrap();
    assert_eq!(serde_json::from_str::<ToServer>(again.trim_end()).unwrap(), message);
}

fuzz_target!(|data: &[u8]| {
    let mut reader = std::io::Cursor::new(data);
    while let Ok(Some(line)) = read_line(&mut reader) {
        check_game_side(&line);
        check_server_side(&line);
    }
});
