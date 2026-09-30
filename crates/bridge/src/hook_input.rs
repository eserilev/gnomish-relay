//! The JSON that Claude Code and Codex give a hook on stdin, as one spool event
//! (SPEC.md 10.1). Every field but the few below is ignored.

use serde_json::Value;

use crate::spool::{Source, SpoolEvent, is_session_id};

/// The text of a `Stop` with no text: the turn ended on a tool call.
pub const TURN_DONE: &str = "Turn done.";
const FAILED: &str = "The turn failed.";
const WAITING: &str = "Waiting for your answer.";

/// The `Notification` types that wait for the user. `idle_prompt` comes 60 seconds after
/// each `Stop`, so it is a copy of `finished`.
pub const WAITING_TYPES: [&str; 4] = [
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "worker_permission_prompt",
];

#[derive(Debug, PartialEq, Eq)]
pub struct HookEvent {
    pub event: SpoolEvent,
    pub session: String,
    pub cwd: Option<String>,
    pub text: String,
}

fn string_at<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

fn text_or(input: &Value, key: &str, fallback: &str) -> String {
    match string_at(input, key).map(str::trim) {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => fallback.to_owned(),
    }
}

/// `None` for an event that gives no notification, or for input with no session.
pub fn parse_hook_input(source: Source, bytes: &[u8]) -> Option<HookEvent> {
    let input: Value = serde_json::from_slice(bytes).ok()?;
    let session = string_at(&input, "session_id").filter(|s| is_session_id(s))?;
    let name = string_at(&input, "hook_event_name")?;
    let (event, text) = match source {
        Source::Claude => claude_event(name, &input)?,
        Source::Codex => codex_event(name, &input)?,
    };
    Some(HookEvent {
        event,
        session: session.to_owned(),
        cwd: string_at(&input, "cwd").map(str::to_owned),
        text,
    })
}

fn claude_event(name: &str, input: &Value) -> Option<(SpoolEvent, String)> {
    let event = match name {
        "SessionStart" => (SpoolEvent::SessionStart, String::new()),
        "UserPromptSubmit" => (SpoolEvent::TurnStart, String::new()),
        "Stop" => (
            SpoolEvent::Finished,
            text_or(input, "last_assistant_message", TURN_DONE),
        ),
        "StopFailure" => (SpoolEvent::Failed, failure_text(input)),
        "Notification" if waits_for_user(input) => {
            (SpoolEvent::Waiting, text_or(input, "message", WAITING))
        }
        "SessionEnd" => (SpoolEvent::SessionEnd, String::new()),
        _ => return None,
    };
    Some(event)
}

/// The matcher of the install already filters. A matcher that the user changed must not
/// bring back `idle_prompt`.
fn waits_for_user(input: &Value) -> bool {
    match string_at(input, "notification_type") {
        Some(kind) => WAITING_TYPES.contains(&kind),
        None => true,
    }
}

/// `error` is a short code such as `rate_limit`. `error_details` says more.
fn failure_text(input: &Value) -> String {
    let code = text_or(input, "error", FAILED);
    text_or(input, "error_details", &code)
}

fn codex_event(name: &str, input: &Value) -> Option<(SpoolEvent, String)> {
    let event = match name {
        "SessionStart" => (SpoolEvent::SessionStart, String::new()),
        "UserPromptSubmit" => (SpoolEvent::TurnStart, String::new()),
        "Stop" => (
            SpoolEvent::Finished,
            text_or(input, "last_assistant_message", TURN_DONE),
        ),
        "PermissionRequest" => (SpoolEvent::Waiting, permission_text(input)),
        "SessionEnd" => (SpoolEvent::SessionEnd, String::new()),
        _ => return None,
    };
    Some(event)
}

/// The command of a shell call, as a string or as its words, else the name of the tool.
fn permission_text(input: &Value) -> String {
    let tool_input = input.get("tool_input");
    let command = tool_input.and_then(|t| t.get("command"));
    let text = match command {
        Some(Value::String(line)) => line.clone(),
        Some(Value::Array(words)) => words
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => string_at(input, "tool_name").unwrap_or("").to_owned(),
    };
    if text.trim().is_empty() {
        return WAITING.into();
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: Source, json: &str) -> Option<HookEvent> {
        parse_hook_input(source, json.as_bytes())
    }

    /// The fields that every hook input of both agents has.
    fn input(event: &str, extra: &str) -> String {
        format!(
            r#"{{"session_id":"4f1c-9a","transcript_path":"/t.jsonl","cwd":"/home/u/Code/app","hook_event_name":"{event}"{extra}}}"#
        )
    }

    fn event_of(source: Source, event: &str, extra: &str) -> Option<(SpoolEvent, String)> {
        parse(source, &input(event, extra)).map(|e| (e.event, e.text))
    }

    #[test]
    fn claude_session_start_and_prompt_submit_carry_no_text() {
        let prompt = r#","prompt":"my secret plan""#;
        assert_eq!(
            event_of(Source::Claude, "SessionStart", r#","source":"startup""#),
            Some((SpoolEvent::SessionStart, String::new()))
        );
        assert_eq!(
            event_of(Source::Claude, "UserPromptSubmit", prompt),
            Some((SpoolEvent::TurnStart, String::new()))
        );
    }

    #[test]
    fn claude_stop_carries_the_last_message_or_turn_done() {
        let extra = r#","stop_hook_active":false,"last_assistant_message":"All tests pass.""#;
        assert_eq!(
            event_of(Source::Claude, "Stop", extra),
            Some((SpoolEvent::Finished, "All tests pass.".into()))
        );
        assert_eq!(
            event_of(Source::Claude, "Stop", r#","last_assistant_message":"  ""#),
            Some((SpoolEvent::Finished, TURN_DONE.into()))
        );
        assert_eq!(
            event_of(Source::Claude, "Stop", ""),
            Some((SpoolEvent::Finished, TURN_DONE.into()))
        );
    }

    #[test]
    fn claude_stop_failure_carries_the_details_or_the_error_code() {
        let details = r#","error":"rate_limit","error_details":"Try again in 5 hours.""#;
        assert_eq!(
            event_of(Source::Claude, "StopFailure", details),
            Some((SpoolEvent::Failed, "Try again in 5 hours.".into()))
        );
        assert_eq!(
            event_of(Source::Claude, "StopFailure", r#","error":"overloaded""#),
            Some((SpoolEvent::Failed, "overloaded".into()))
        );
    }

    #[test]
    fn a_claude_notification_waits_only_for_the_types_that_need_the_user() {
        let permission = r#","message":"Claude needs your permission to use Bash","notification_type":"permission_prompt""#;
        assert_eq!(
            event_of(Source::Claude, "Notification", permission),
            Some((
                SpoolEvent::Waiting,
                "Claude needs your permission to use Bash".into()
            ))
        );
        let idle =
            r#","message":"Claude is waiting for your input","notification_type":"idle_prompt""#;
        assert_eq!(event_of(Source::Claude, "Notification", idle), None);
        let teammate = r#","message":"x","notification_type":"agent_needs_input""#;
        assert_eq!(event_of(Source::Claude, "Notification", teammate), None);
    }

    #[test]
    fn claude_session_end_ends_the_session_and_a_subagent_stop_gives_nothing() {
        assert_eq!(
            event_of(Source::Claude, "SessionEnd", r#","reason":"exit""#),
            Some((SpoolEvent::SessionEnd, String::new()))
        );
        assert_eq!(event_of(Source::Claude, "SubagentStop", ""), None);
        assert_eq!(event_of(Source::Claude, "PreToolUse", ""), None);
    }

    #[test]
    fn codex_events_map_as_the_table_of_the_spec() {
        let turn = r#","turn_id":"t1","model":"gpt","prompt":"x""#;
        assert_eq!(
            event_of(Source::Codex, "UserPromptSubmit", turn),
            Some((SpoolEvent::TurnStart, String::new()))
        );
        let stop = r#","turn_id":"t1","last_assistant_message":"Fixed the build.""#;
        assert_eq!(
            event_of(Source::Codex, "Stop", stop),
            Some((SpoolEvent::Finished, "Fixed the build.".into()))
        );
        assert_eq!(
            event_of(Source::Codex, "Stop", r#","last_assistant_message":null"#),
            Some((SpoolEvent::Finished, TURN_DONE.into()))
        );
        assert_eq!(
            event_of(Source::Codex, "SessionStart", r#","source":"resume""#),
            Some((SpoolEvent::SessionStart, String::new()))
        );
        assert_eq!(
            event_of(Source::Codex, "SessionEnd", ""),
            Some((SpoolEvent::SessionEnd, String::new()))
        );
        assert_eq!(event_of(Source::Codex, "PostToolUse", ""), None);
    }

    #[test]
    fn a_codex_permission_request_names_the_command() {
        let line = r#","tool_name":"Bash","tool_input":{"command":"cargo test"}"#;
        assert_eq!(
            event_of(Source::Codex, "PermissionRequest", line),
            Some((SpoolEvent::Waiting, "cargo test".into()))
        );
        let words = r#","tool_name":"shell","tool_input":{"command":["rm","-rf","build"]}"#;
        assert_eq!(
            event_of(Source::Codex, "PermissionRequest", words),
            Some((SpoolEvent::Waiting, "rm -rf build".into()))
        );
        let tool = r#","tool_name":"apply_patch","tool_input":{}"#;
        assert_eq!(
            event_of(Source::Codex, "PermissionRequest", tool),
            Some((SpoolEvent::Waiting, "apply_patch".into()))
        );
        assert_eq!(
            event_of(Source::Codex, "PermissionRequest", ""),
            Some((SpoolEvent::Waiting, WAITING.into()))
        );
    }

    #[test]
    fn input_with_no_valid_session_or_no_json_gives_nothing() {
        let bad_session = input("Stop", "").replace("4f1c-9a", "../../x");
        assert_eq!(parse(Source::Claude, &bad_session), None);
        assert_eq!(parse(Source::Claude, "not json"), None);
        assert_eq!(parse(Source::Claude, "{}"), None);
        assert_eq!(parse(Source::Claude, ""), None);
    }

    #[test]
    fn the_cwd_comes_along_for_the_repo_name() {
        let event = parse(Source::Claude, &input("Stop", "")).unwrap();
        assert_eq!(event.cwd.as_deref(), Some("/home/u/Code/app"));
        assert_eq!(event.session, "4f1c-9a");
    }
}
