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

/// Asks the client a question and waits for its answer.
fn ask(method: &str, params: &Value) -> Option<Value> {
    send(&json!({ "jsonrpc": "2.0", "id": "q1", "method": method, "params": params }));
    read()
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
        "permission" => {
            let answer = ask(
                "session/request_permission",
                &json!({
                    "sessionId": session,
                    "toolCall": { "toolCallId": "t1", "title": "rm -rf build" },
                    "options": [
                        { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
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
                "agentCapabilities": {
                    "loadSession": script == "load",
                    "sessionCapabilities": if script == "resume" { json!({ "resume": {} }) } else { json!({}) },
                },
            }),
            method @ ("session/resume" | "session/load") => {
                let id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                if method == "session/load" {
                    chunk(&params["sessionId"], "OLD HISTORY ");
                }
                format!("{id} by {method}").clone_into(&mut resumed);
                json!({})
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
