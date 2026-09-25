//! Any flags field from the game: the parser never panics, and every value that it
//! takes has the shape of SPEC.md 7.1.1.
#![no_main]

use bridge::flags::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let flags = parse(data);
    if let Some(answer) = &flags.perm {
        assert!(protocol::record::is_valid_id(answer.request.as_bytes()));
        assert_eq!(answer.hash.len(), 16);
    }
    if let Some(build) = &flags.build {
        assert!(build.bytes().all(|b| b.is_ascii_digit()));
    }
    if let Some(agent) = &flags.agent {
        assert!(protocol::record::is_valid_id(agent.as_bytes()));
    }
});
