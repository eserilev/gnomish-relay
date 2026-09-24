//! Any text in the saved variables file: the frame reader never panics (SPEC.md 14.4).
//! Another addon can write any string into `GnomishRelayDB`.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = bridge::saved::frames(text);
});
