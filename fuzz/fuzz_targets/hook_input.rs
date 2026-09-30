//! The stdin of any agent version into a spool file (SPEC.md 10.7): no input panics the
//! hook, and its spool file is one JSON object of at most 4 KiB that the bridge takes.
#![no_main]

use bridge::hook::hook_file;
use bridge::spool::{MAX_FILE, Source, SpoolFile};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for source in [Source::Claude, Source::Codex] {
        let Some(file) = hook_file(source, data) else {
            continue;
        };
        let bytes = file.to_bytes();
        assert!(bytes.len() as u64 <= MAX_FILE, "{} bytes", bytes.len());
        assert_eq!(SpoolFile::parse(&bytes).as_ref(), Ok(&file));
    }
});
