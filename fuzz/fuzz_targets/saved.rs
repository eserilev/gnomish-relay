//! Any text in a saved variables file: the frame reader and the reader of the self-test
//! results never panic (SPEC.md 14.4). Another addon can write any string into them.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = bridge::saved::frames(text);
    if let Ok(parts) = bridge::selftest::Parts::read(text) {
        let _ = parts.shots();
        let _ = bridge::fixture::build(&parts.results, &parts.load, parts.combat.as_ref(), true);
    }
});
