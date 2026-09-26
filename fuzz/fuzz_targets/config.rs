//! Any text in `config.toml`: the parser gives a config or an error, never a panic
//! (SPEC.md 14.4). A config it takes has only existing absolute roots, and a relay part
//! only with its default agent.
#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let Ok(config) = bridge::config::parse(text, Path::new("/")) else {
        return;
    };
    let Some(relay) = &config.relay else {
        return;
    };
    for root in &relay.policy.folders.roots {
        assert!(root.starts_with(b"/"));
    }
    assert!(relay.policy.agents.contains_key(&relay.policy.default_agent));
});
