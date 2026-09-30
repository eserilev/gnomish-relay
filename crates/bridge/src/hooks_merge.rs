//! Our hook groups in the `hooks.json` shape of Claude Code and Codex, merged into the
//! settings of the user or taken out again (SPEC.md 10.5). No I/O here.

use std::path::Path;

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use crate::spool::Source;

pub const PROGRAM: &str = "gnomish-relay";
/// Seconds. The hook ends itself after 300 ms.
const TIMEOUT: u64 = 5;

/// One event of 10.1, with the matcher of its group.
pub struct OurHook {
    pub event: &'static str,
    pub matcher: Option<&'static str>,
}

/// `compact` is not in the matcher: a compaction inside a turn must not end the turn.
const SESSION_START: Option<&str> = Some("startup|resume|clear");

pub const CLAUDE_HOOKS: [OurHook; 6] = [
    OurHook {
        event: "SessionStart",
        matcher: SESSION_START,
    },
    OurHook {
        event: "UserPromptSubmit",
        matcher: None,
    },
    OurHook {
        event: "Stop",
        matcher: None,
    },
    OurHook {
        event: "StopFailure",
        matcher: None,
    },
    OurHook {
        event: "Notification",
        matcher: Some(
            "permission_prompt|elicitation_dialog|elicitation_url_dialog|worker_permission_prompt",
        ),
    },
    OurHook {
        event: "SessionEnd",
        matcher: None,
    },
];

pub const CODEX_HOOKS: [OurHook; 5] = [
    OurHook {
        event: "SessionStart",
        matcher: SESSION_START,
    },
    OurHook {
        event: "UserPromptSubmit",
        matcher: None,
    },
    OurHook {
        event: "Stop",
        matcher: None,
    },
    OurHook {
        event: "PermissionRequest",
        matcher: None,
    },
    OurHook {
        event: "SessionEnd",
        matcher: None,
    },
];

pub fn agent_word(source: Source) -> &'static str {
    match source {
        Source::Claude => "claude",
        Source::Codex => "codex",
    }
}

pub fn our_hooks(source: Source) -> &'static [OurHook] {
    match source {
        Source::Claude => &CLAUDE_HOOKS,
        Source::Codex => &CODEX_HOOKS,
    }
}

/// The path is quoted, for a space on Windows.
pub fn our_command(program: &Path, source: Source) -> String {
    format!("\"{}\" hook {}", program.display(), agent_word(source))
}

/// The program of our command, or `None` for a command of the user. It is ours when
/// `hook <agent>` follows a path whose file name is `gnomish-relay`.
pub fn our_program(command: &str, source: Source) -> Option<&str> {
    let suffix = format!(" hook {}", agent_word(source));
    let program = command.strip_suffix(&suffix)?.trim();
    let program = program.strip_prefix('"').unwrap_or(program);
    let program = program.strip_suffix('"').unwrap_or(program);
    let name = Path::new(program).file_name()?.to_str()?;
    let ours = name == PROGRAM || name == format!("{PROGRAM}.exe");
    ours.then_some(program)
}

/// Claude never waits for an `async` hook and ignores its output. Codex runs its hooks in
/// turn, so it gets no `async`: the hook ends in 300 ms and prints nothing.
fn our_group(hook: &OurHook, program: &Path, source: Source) -> Value {
    let mut handler = json!({
        "type": "command",
        "command": our_command(program, source),
        "timeout": TIMEOUT,
    });
    if source == Source::Claude {
        handler["async"] = Value::Bool(true);
    }
    let mut group = Map::new();
    if let Some(matcher) = hook.matcher {
        group.insert("matcher".into(), matcher.into());
    }
    group.insert("hooks".into(), Value::Array(vec![handler]));
    Value::Object(group)
}

/// The program of a group with exactly one hook, our command.
fn group_program(group: &Value, source: Source) -> Option<&str> {
    let [handler] = group.get("hooks")?.as_array()?.as_slice() else {
        return None;
    };
    our_program(handler.get("command")?.as_str()?, source)
}

pub fn is_our_group(group: &Value, source: Source) -> bool {
    group_program(group, source).is_some()
}

/// The `hooks` object, and each of our events in it, must have the type of the format.
/// A check before any change means that a refusal changes nothing.
fn check_types(root: &Value, source: Source) -> Result<()> {
    let Some(root) = root.as_object() else {
        bail!("the file is not a JSON object");
    };
    let Some(hooks) = root.get("hooks") else {
        return Ok(());
    };
    let Some(hooks) = hooks.as_object() else {
        bail!("the key hooks is not an object");
    };
    for hook in our_hooks(source) {
        if hooks.get(hook.event).is_some_and(|e| !e.is_array()) {
            bail!("the key hooks.{} is not a list", hook.event);
        }
    }
    Ok(())
}

/// Replaces our old group in its place, so a second install changes nothing.
fn put_group(groups: &mut Vec<Value>, group: Value, source: Source) {
    let first = groups.iter().position(|g| is_our_group(g, source));
    groups.retain(|g| !is_our_group(g, source));
    match first {
        Some(at) => groups.insert(at, group),
        None => groups.push(group),
    }
}

/// Adds one group of ours to each event, and keeps every key and hook of the user.
pub fn install(mut root: Value, program: &Path, source: Source) -> Result<Value> {
    check_types(&root, source)?;
    let Some(object) = root.as_object_mut() else {
        bail!("the file is not a JSON object");
    };
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        bail!("the key hooks is not an object");
    };
    for hook in our_hooks(source) {
        let groups = hooks
            .entry(hook.event)
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(groups) = groups.as_array_mut() {
            put_group(groups, our_group(hook, program, source), source);
        }
    }
    Ok(root)
}

/// Takes out our groups, an event with no group left, and a `hooks` with no event left.
pub fn remove(mut root: Value, source: Source) -> Result<Value> {
    check_types(&root, source)?;
    let Some(object) = root.as_object_mut() else {
        bail!("the file is not a JSON object");
    };
    let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(root);
    };
    let mut removed = false;
    for hook in our_hooks(source) {
        let Some(groups) = hooks.get_mut(hook.event).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = groups.len();
        groups.retain(|g| !is_our_group(g, source));
        if groups.len() < before {
            removed = true;
            if groups.is_empty() {
                hooks.shift_remove(hook.event);
            }
        }
    }
    if removed && hooks.is_empty() {
        object.shift_remove("hooks");
    }
    Ok(root)
}

/// The programs of our groups in `root`, one for each group.
pub fn our_programs(root: &Value, source: Source) -> Vec<String> {
    let mut programs = Vec::new();
    for hook in our_hooks(source) {
        let groups = root
            .get("hooks")
            .and_then(|h| h.get(hook.event))
            .and_then(Value::as_array);
        for group in groups.into_iter().flatten() {
            programs.extend(group_program(group, source).map(str::to_owned));
        }
    }
    programs
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM_PATH: &str = "/home/u/.local/bin/gnomish-relay";

    fn program() -> &'static Path {
        Path::new(PROGRAM_PATH)
    }

    fn user_settings() -> Value {
        serde_json::from_str(
            r#"{
              "model": "opus",
              "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "notify-send done"}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "audit"}]}]
              },
              "permissions": {"allow": ["Bash(ls)"]}
            }"#,
        )
        .unwrap()
    }

    fn count_ours(root: &Value, source: Source) -> usize {
        our_programs(root, source).len()
    }

    #[test]
    fn install_adds_one_group_for_each_event_of_the_spec() {
        let root = install(json!({}), program(), Source::Claude).unwrap();
        let hooks = root["hooks"].as_object().unwrap();
        let events: Vec<&str> = hooks.keys().map(String::as_str).collect();
        assert_eq!(
            events,
            [
                "SessionStart",
                "UserPromptSubmit",
                "Stop",
                "StopFailure",
                "Notification",
                "SessionEnd"
            ]
        );
        assert_eq!(
            hooks["Stop"],
            json!([{"hooks": [{
                "type": "command",
                "command": format!("\"{PROGRAM_PATH}\" hook claude"),
                "timeout": 5,
                "async": true,
            }]}])
        );
        assert_eq!(hooks["SessionStart"][0]["matcher"], "startup|resume|clear");
        assert!(
            !hooks["Notification"][0]["matcher"]
                .as_str()
                .unwrap()
                .contains("idle_prompt")
        );
    }

    #[test]
    fn codex_gets_its_own_events_and_no_async() {
        let root = install(json!({}), program(), Source::Codex).unwrap();
        let hooks = root["hooks"].as_object().unwrap();
        let events: Vec<&str> = hooks.keys().map(String::as_str).collect();
        assert_eq!(
            events,
            [
                "SessionStart",
                "UserPromptSubmit",
                "Stop",
                "PermissionRequest",
                "SessionEnd"
            ]
        );
        let handler = &hooks["PermissionRequest"][0]["hooks"][0];
        assert_eq!(handler["command"], format!("\"{PROGRAM_PATH}\" hook codex"));
        assert!(handler.get("async").is_none());
    }

    #[test]
    fn install_keeps_the_hooks_of_the_user() {
        let root = install(user_settings(), program(), Source::Claude).unwrap();
        let keys: Vec<&str> = root
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["model", "hooks", "permissions"]);
        let stop = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop[0]["hooks"][0]["command"], "notify-send done");
        assert_eq!(stop.len(), 2);
        assert_eq!(
            root["hooks"]["PreToolUse"],
            user_settings()["hooks"]["PreToolUse"]
        );
    }

    #[test]
    fn a_second_install_changes_nothing() {
        let once = install(user_settings(), program(), Source::Claude).unwrap();
        let twice = install(once.clone(), program(), Source::Claude).unwrap();
        assert_eq!(once, twice);
        assert_eq!(count_ours(&twice, Source::Claude), 6);
    }

    #[test]
    fn an_install_after_a_move_of_the_binary_replaces_the_old_path_in_place() {
        let old = install(
            user_settings(),
            Path::new("/opt/old/gnomish-relay"),
            Source::Claude,
        )
        .unwrap();
        let mut with_user_after = old.clone();
        with_user_after["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks": [{"type": "command", "command": "later"}]}));

        let moved = install(with_user_after, program(), Source::Claude).unwrap();

        assert_eq!(our_programs(&moved, Source::Claude), vec![PROGRAM_PATH; 6]);
        let stop = moved["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 3);
        assert_eq!(stop[2]["hooks"][0]["command"], "later");
    }

    #[test]
    fn install_then_remove_gives_the_same_value_as_before() {
        for before in [user_settings(), json!({}), json!({"model": "opus"})] {
            let installed = install(before.clone(), program(), Source::Claude).unwrap();
            assert_eq!(remove(installed, Source::Claude).unwrap(), before);
        }
    }

    /// Remove cannot tell an empty list that the user wrote from one that it emptied.
    #[test]
    fn install_then_remove_drops_only_an_empty_list_of_our_events_and_an_empty_hooks() {
        let cases = [
            (json!({"hooks": {}}), json!({})),
            (json!({"hooks": {"Stop": []}}), json!({})),
            (
                json!({"hooks": {"Stop": [], "PreToolUse": []}}),
                json!({"hooks": {"PreToolUse": []}}),
            ),
        ];
        for (before, after) in cases {
            let installed = install(before, program(), Source::Claude).unwrap();
            assert_eq!(remove(installed, Source::Claude).unwrap(), after);
        }
    }

    #[test]
    fn remove_takes_out_only_our_groups() {
        let both = install(user_settings(), program(), Source::Claude).unwrap();
        let both = install(both, program(), Source::Codex).unwrap();
        let removed = remove(both, Source::Claude).unwrap();
        assert_eq!(count_ours(&removed, Source::Claude), 0);
        assert_eq!(count_ours(&removed, Source::Codex), 5);
        assert_eq!(
            removed["hooks"]["Stop"][0]["hooks"][0]["command"],
            "notify-send done"
        );
    }

    #[test]
    fn a_wrong_type_is_refused_and_names_the_key() {
        let cases = [
            (json!([1]), "not a JSON object"),
            (json!({"hooks": []}), "hooks is not an object"),
            (json!({"hooks": {"Stop": {}}}), "hooks.Stop is not a list"),
        ];
        for (root, key) in cases {
            let install_error = install(root.clone(), program(), Source::Claude).unwrap_err();
            let remove_error = remove(root, Source::Claude).unwrap_err();
            assert!(install_error.to_string().contains(key), "{install_error}");
            assert!(remove_error.to_string().contains(key), "{remove_error}");
        }
    }

    #[test]
    fn our_command_is_found_by_the_file_name_of_its_program() {
        assert_eq!(
            our_program("\"/a b/gnomish-relay\" hook claude", Source::Claude),
            Some("/a b/gnomish-relay")
        );
        assert_eq!(
            our_program("gnomish-relay.exe hook codex", Source::Codex),
            Some("gnomish-relay.exe")
        );
        assert_eq!(our_program("/bin/other hook claude", Source::Claude), None);
        assert_eq!(
            our_program("\"/x/gnomish-relay\" hook codex", Source::Claude),
            None
        );
        assert_eq!(
            our_program("gnomish-relay hook claude --x", Source::Claude),
            None
        );
    }

    #[test]
    fn a_group_of_the_user_with_our_command_and_more_stays() {
        let mixed = json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": "gnomish-relay hook claude"},
            {"type": "command", "command": "say done"},
        ]}]}});
        let removed = remove(mixed.clone(), Source::Claude).unwrap();
        assert_eq!(removed, mixed);
    }
}
