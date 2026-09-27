//! Any text as `rules.json` (SPEC.md 6.6.5): the reader never panics, and each rule
//! that loads has an id of 4 hex digits, a clean absolute folder, and 1 or 2 plain words.
#![no_main]

use std::path::Component;

use bridge::always_rules::{DAYS_KEPT, day_of, is_rule, parse_rules};
use libfuzzer_sys::fuzz_target;

const NOW: u32 = 1_790_000_000;

fuzz_target!(|text: &str| {
    for rule in parse_rules(text, NOW) {
        assert!(rule.id.len() == 4 && rule.id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(rule.folder.is_absolute());
        assert!(
            rule.folder
                .components()
                .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
        );
        assert!(is_rule(&rule.words));
        assert!(day_of(NOW) - rule.used_day.min(day_of(NOW)) <= DAYS_KEPT);
    }
});
