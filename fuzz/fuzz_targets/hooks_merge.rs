//! Any `settings.json` text (SPEC.md 10.7): the merge never panics. It refuses and changes
//! nothing, or its result holds every key and hook of the input, and each group of ours
//! once. A remove then takes out every group of ours. For an input with no group of
//! ours, install then remove gives the input back, but an empty list of one of our
//! events and an empty `hooks` go.
#![no_main]

use std::path::Path;

use bridge::hooks_merge::{install, is_our_group, our_hooks, our_programs, remove};
use bridge::spool::Source;
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

/// The groups of the user in one event, in their order.
fn user_groups(root: &Value, event: &str, source: Source) -> Vec<Value> {
    let groups = root
        .get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array);
    groups
        .into_iter()
        .flatten()
        .filter(|g| !is_our_group(g, source))
        .cloned()
        .collect()
}

/// The input as install then remove gives it back.
fn without_empty_lists_of_our_events(root: &Value, source: Source) -> Value {
    let mut out = root.clone();
    let Some(hooks) = out.get_mut("hooks").and_then(Value::as_object_mut) else {
        return out;
    };
    for hook in our_hooks(source) {
        if hooks
            .get(hook.event)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            hooks.shift_remove(hook.event);
        }
    }
    if hooks.is_empty() {
        out.as_object_mut()
            .expect("it has hooks")
            .shift_remove("hooks");
    }
    out
}

fn check(root: &Value, source: Source) {
    let Ok(merged) = install(root.clone(), Path::new("/opt/bin/gnomish-relay"), source) else {
        return;
    };
    let input = root.as_object().expect("only an object merges");
    for (key, value) in input {
        if key != "hooks" {
            assert_eq!(&merged[key], value, "the key {key} stays");
        }
    }
    let hooks = our_hooks(source);
    assert_eq!(
        our_programs(&merged, source).len(),
        hooks.len(),
        "each group of ours once"
    );
    for hook in hooks {
        assert_eq!(
            user_groups(&merged, hook.event, source),
            user_groups(root, hook.event, source),
            "the hooks of the user in {} stay",
            hook.event
        );
    }
    let removed = remove(merged, source).expect("our own result always removes");
    assert!(our_programs(&removed, source).is_empty());
    if our_programs(root, source).is_empty() {
        assert_eq!(removed, without_empty_lists_of_our_events(root, source));
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(root) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    for source in [Source::Claude, Source::Codex] {
        check(&root, source);
        if remove(root.clone(), source).is_err() {
            assert!(install(root.clone(), Path::new("/x/gnomish-relay"), source).is_err());
        }
    }
});
