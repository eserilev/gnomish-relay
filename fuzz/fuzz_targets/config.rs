//! Any text in `config.toml`: the parser gives a config or an error, never a panic
//! (SPEC.md 14.4). A config it takes has only existing absolute roots.
#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Ok(config) = bridge::config::parse(text, Path::new("/")) {
        for root in &config.policy.folders.roots {
            assert!(root.starts_with(b"/"));
        }
        assert!(config.policy.agents.contains_key(&config.policy.default_agent));
    }
});
