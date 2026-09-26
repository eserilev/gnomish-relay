//! Any flags field from the game: the parsers never panic, and every value that they
//! take has the shape of SPEC.md 7.1.1. The transport flags, which the Timeways lane
//! reads, never change because of a coding flag (SPEC.md 9.7, decision 6).
#![no_main]

use bridge::flags::{coding, transport};
use libfuzzer_sys::fuzz_target;

const CODING: [&str; 8] = ["perm", "level", "agent", "attach", "list", "d", "n", "stop"];

fn is_coding(flag: &str) -> bool {
    let name = flag.split_once('=').map_or(flag, |(name, _)| name);
    CODING.contains(&name)
}

fuzz_target!(|data: &[u8]| {
    let flags = coding(data);
    if let Some(answer) = &flags.perm {
        assert!(protocol::record::is_valid_id(answer.request.as_bytes()));
        assert_eq!(answer.hash.len(), 16);
    }
    if let Some(agent) = &flags.agent {
        assert!(protocol::record::is_valid_id(agent.as_bytes()));
    }
    let report = transport(data);
    if let Some(build) = &report.build {
        assert!(build.bytes().all(|b| b.is_ascii_digit()));
    }
    let text = String::from_utf8_lossy(data);
    let without_coding: Vec<&str> = text.split(';').filter(|f| !is_coding(f)).collect();
    assert_eq!(transport(without_coding.join(";").as_bytes()), report);
});
