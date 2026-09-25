//! A scripted ACP agent for the tests of `acp.rs`. The first argument names the script.
//! It is test code: nothing in the bridge starts it.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn send(message: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn read() -> Option<Value> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

fn chunk(session: &Value, text: &str) {
    send(
        &json!({ "jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": session,
            "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } },
        }}),
    );
}

fn user_chunk(session: &Value, text: &str) {
    send(
        &json!({ "jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": session,
            "update": { "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": text } },
        }}),
    );
}

/// Two exchanges, in chunks, as `session/load` of a saved session replays them.
fn replay(session: &Value) {
    user_chunk(session, "first question");
    chunk(session, "first answer");
    user_chunk(session, "fix the\n");
    user_chunk(session, "bugs");
    chunk(session, "All ");
    chunk(session, "fixed.");
}

fn capabilities(script: &str) -> Value {
    let sessions = match script {
        "resume" => json!({ "resume": {} }),
        "sessions" => json!({ "list": {}, "fork": {} }),
        _ => json!({}),
    };
    json!({ "loadSession": script == "load" || script == "sessions", "sessionCapabilities": sessions })
}

/// Asks the client a question and waits for its answer.
fn ask(method: &str, params: &Value) -> Option<Value> {
    send(&json!({ "jsonrpc": "2.0", "id": "q1", "method": method, "params": params }));
    read()
}

/// Asks the client about `tool_call`, and returns the option that it chose.
fn ask_permission(session: &Value, tool_call: &Value) -> Option<String> {
    let answer = ask(
        "session/request_permission",
        &json!({
            "sessionId": session,
            "toolCall": tool_call,
            "options": [
                { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                { "optionId": "always", "name": "Always", "kind": "allow_always" },
                { "optionId": "no", "name": "Reject", "kind": "reject_once" },
            ],
        }),
    )?;
    let outcome = answer
        .pointer("/result/outcome/optionId")
        .and_then(Value::as_str)
        .unwrap_or("cancelled");
    Some(format!("chose {outcome}"))
}

fn prompt_reply(script: &str, params: &Value, mode: &str, resumed: &str) -> Option<String> {
    let session = params.get("sessionId").cloned().unwrap_or(Value::Null);
    let text = params
        .pointer("/prompt/0/text")
        .and_then(Value::as_str)
        .unwrap_or("");
    match script {
        "hang" => loop {
            std::thread::park();
        },
        "crash" => {
            eprintln!("boom: not logged in");
            std::process::exit(3);
        }
        "garbage" => {
            println!("this is not json");
            None
        }
        "huge" => {
            println!("{}", "x".repeat(9 * 1024 * 1024));
            None
        }
        "permission" => ask_permission(
            &session,
            &json!({ "toolCallId": "t1", "title": "clean the build", "rawInput": { "command": "rm -rf build" } }),
        ),
        "readkey" => ask_permission(
            &session,
            &json!({ "toolCallId": "t2", "title": "read the key", "kind": "read",
                     "locations": [{ "path": "config/strip.key" }], "rawInput": {} }),
        ),
        "steps" => {
            for title in ["edit src/main.rs", "$ cargo test"] {
                send(
                    &json!({ "jsonrpc": "2.0", "method": "session/update", "params": {
                        "sessionId": session,
                        "update": { "sessionUpdate": "tool_call", "toolCallId": title, "title": title },
                    }}),
                );
            }
            Some("done".into())
        }
        "resume" | "load" | "noresume" => {
            let session = params
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("?");
            Some(format!("in {session}, resumed {resumed}"))
        }
        "slow" => {
            // Waits for `session/cancel`, then ends the turn as ACP says.
            while let Some(message) = read() {
                if message.get("method").and_then(Value::as_str) == Some("session/cancel") {
                    break;
                }
            }
            None
        }
        "files" => {
            let answer = ask(
                "fs/read_text_file",
                &json!({ "sessionId": session, "path": "/etc/passwd" }),
            )?;
            let code = answer
                .pointer("/error/code")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            Some(format!("fs answer {code}"))
        }
        _ => {
            chunk(&session, "you said: ");
            let secret = if std::env::var_os("CARGO_MANIFEST_DIR").is_some() {
                "leaked"
            } else {
                "hidden"
            };
            let job = std::env::var("GNOMISH_RELAY_JOB").unwrap_or_default();
            Some(format!("{text} [mode={mode} secret={secret} job={job}]"))
        }
    }
}

fn main() {
    let script = std::env::args().nth(1).unwrap_or_default();
    let mut mode = "none".to_owned();
    let mut resumed = "no".to_owned();
    while let Some(message) = read() {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match message.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": if script == "v2" { 2 } else { 1 },
                "agentInfo": { "name": "fake", "version": "1.0" },
                "agentCapabilities": capabilities(&script),
            }),
            method @ ("session/resume" | "session/load") => {
                let id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                if method == "session/load" && script == "sessions" {
                    replay(&params["sessionId"]);
                } else if method == "session/load" {
                    chunk(&params["sessionId"], "OLD HISTORY ");
                }
                format!("{id} by {method}").clone_into(&mut resumed);
                json!({})
            }
            "session/list" => json!({ "sessions": [
                { "sessionId": "a1", "cwd": "/w/app", "title": "Fix bugs", "updatedAt": "2026-09-25T06:46:21.432Z" },
                { "sessionId": "b2", "title": "No folder" },
            ]}),
            "session/fork" => {
                let id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                json!({ "sessionId": format!("fork-of-{id}") })
            }
            "session/new" => json!({
                "sessionId": "s1",
                "modes": { "currentModeId": "default", "availableModes": [
                    { "id": "default", "name": "Default" },
                    { "id": "plan", "name": "Plan" },
                ]},
            }),
            "session/set_mode" => {
                params
                    .get("modeId")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .clone_into(&mut mode);
                json!({})
            }
            "session/prompt" => {
                if script == "slow" {
                    prompt_reply(&script, &params, &mode, &resumed);
                    send(
                        &json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "cancelled" } }),
                    );
                    continue;
                }
                let Some(reply) = prompt_reply(&script, &params, &mode, &resumed) else {
                    return;
                };
                chunk(&params["sessionId"], &reply);
                json!({ "stopReason": "end_turn" })
            }
            _ => json!({}),
        };
        send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
}
