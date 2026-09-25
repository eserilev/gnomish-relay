//! A scripted `codex app-server` for the tests of `codex.rs`. The first argument names
//! the script, and the bridge adds `app-server` after it. It is test code: nothing in
//! the bridge starts it.

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

fn notify(method: &str, params: &Value) {
    send(&json!({ "method": method, "params": params }));
}

fn said(text: &str) {
    notify(
        "item/completed",
        &json!({ "threadId": "t1", "turnId": "u1", "item": { "type": "agentMessage", "id": "m1", "text": text } }),
    );
}

fn completed(status: &str) {
    let error = if status == "failed" {
        json!({ "message": "You've hit your usage limit." })
    } else {
        Value::Null
    };
    notify(
        "turn/completed",
        &json!({ "threadId": "t1", "turn": { "id": "u1", "items": [], "status": status, "error": error } }),
    );
}

/// Asks the bridge a question and returns its decision.
fn ask(method: &str, params: &Value) -> String {
    send(&json!({ "method": method, "id": 90, "params": params }));
    let Some(answer) = read() else {
        return "none".into();
    };
    match answer.pointer("/result/decision") {
        Some(Value::String(decision)) => decision.clone(),
        Some(other) => other.to_string(),
        None => "error".into(),
    }
}

/// Plays one turn and returns its final text, or `None` when the turn ends another way.
/// One approval for the gate: `arg` is the command, or the path of a change.
fn gate_turn(script: &str, arg: &str) -> String {
    if script == "command" {
        let decision = ask(
            "item/commandExecution/requestApproval",
            &json!({ "threadId": "t1", "turnId": "u1", "itemId": "c1", "command": arg }),
        );
        return format!("command {decision}");
    }
    notify(
        "item/started",
        &json!({ "threadId": "t1", "turnId": "u1", "item": { "type": "fileChange", "id": "f1", "changes": [{ "path": arg }] } }),
    );
    let decision = ask(
        "item/fileChange/requestApproval",
        &json!({ "threadId": "t1", "turnId": "u1", "itemId": "f1" }),
    );
    format!("change {decision}")
}

fn turn(script: &str, text: &str, state: &str, arg: &str) -> Option<String> {
    match script {
        "command" | "change" => Some(gate_turn(script, arg)),
        "hang" => loop {
            std::thread::park();
        },
        "crash" => {
            eprintln!("boom: stream disconnected");
            std::process::exit(3);
        }
        "garbage" => {
            println!("this is not json");
            None
        }
        "failed" => {
            completed("failed");
            None
        }
        "steps" => {
            let started = |item: Value| {
                notify(
                    "item/started",
                    &json!({ "threadId": "t1", "turnId": "u1", "item": item }),
                );
            };
            started(
                json!({ "type": "commandExecution", "id": "c1", "command": "cargo  test", "cwd": "/w" }),
            );
            started(
                json!({ "type": "fileChange", "id": "f1", "changes": [{ "path": "src/main.rs", "kind": { "type": "update" }, "diff": "" }] }),
            );
            started(json!({ "type": "reasoning", "id": "r1" }));
            said("Looking.");
            Some("done".into())
        }
        "approval" => {
            let command = ask(
                "item/commandExecution/requestApproval",
                &json!({ "threadId": "t1", "turnId": "u1", "itemId": "c1", "command": "/bin/bash -lc 'rm -rf build'", "reason": "clean the build",
                         "proposedExecpolicyAmendment": ["rm"] }),
            );
            notify(
                "item/started",
                &json!({ "threadId": "t1", "turnId": "u1", "item": { "type": "fileChange", "id": "f1", "changes": [{ "path": "src/a.rs" }] } }),
            );
            let change = ask(
                "item/fileChange/requestApproval",
                &json!({ "threadId": "t1", "turnId": "u1", "itemId": "f1" }),
            );
            Some(format!("command {command}, change {change}"))
        }
        "other" => {
            let answer = ask(
                "item/tool/requestUserInput",
                &json!({ "threadId": "t1", "questions": [] }),
            );
            Some(format!("input answer {answer}"))
        }
        "slow" => {
            // Waits for the interrupt, then ends the turn as Codex does.
            while let Some(message) = read() {
                if message.get("method").and_then(Value::as_str) == Some("turn/interrupt") {
                    send(&json!({ "id": message["id"], "result": {} }));
                    break;
                }
            }
            completed("interrupted");
            None
        }
        _ => {
            let secret = if std::env::var_os("CARGO_MANIFEST_DIR").is_some() {
                "leaked"
            } else {
                "hidden"
            };
            let job = std::env::var("GNOMISH_RELAY_JOB").unwrap_or_default();
            said("thinking out loud");
            Some(format!(
                "you said: {text} [{state} secret={secret} job={job}]"
            ))
        }
    }
}

fn thread(id: &str, preview: &str) -> Value {
    json!({ "id": id, "preview": preview, "name": null, "cwd": "/w/app", "updatedAt": 1_790_318_781, "status": { "type": "notLoaded" } })
}

fn answer(script: &str, method: &str, params: &Value, state: &mut String) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({ "userAgent": "fake", "codexHome": "/h/.codex" })),
        "thread/start" => {
            *state = format!(
                "sandbox={} approval={}",
                params["sandbox"].as_str().unwrap_or("?"),
                params["approvalPolicy"].as_str().unwrap_or("?")
            );
            Ok(json!({ "thread": thread("t1", "") }))
        }
        "thread/resume" if script == "noresume" => Err("thread not found".into()),
        "thread/resume" => {
            let id = params["threadId"].as_str().unwrap_or("?");
            *state = format!("resumed {id}");
            Ok(json!({ "thread": thread(id, "") }))
        }
        "thread/fork" => {
            let id = params["threadId"].as_str().unwrap_or("?");
            Ok(json!({ "thread": thread(&format!("fork-of-{id}"), "") }))
        }
        "thread/list" => Ok(json!({ "data": [
            { "id": "a1", "name": "Fix bugs", "preview": "fix the bugs", "cwd": "/w/app", "updatedAt": 1_790_318_781 },
            { "id": "b2", "name": null, "preview": "add\na test", "cwd": "/w/lib", "updatedAt": 5 },
            { "id": "c3", "preview": "no folder" },
        ], "nextCursor": null })),
        "thread/turns/list" => Ok(
            json!({ "data": [{ "id": "u9", "status": "completed", "items": [
                { "type": "userMessage", "id": "i1", "content": [{ "type": "text", "text": "fix the\nbugs" }] },
                { "type": "commandExecution", "id": "i2", "command": "cargo test" },
                { "type": "agentMessage", "id": "i3", "text": "Looking." },
                { "type": "agentMessage", "id": "i4", "text": "All fixed." },
            ]}]}),
        ),
        _ => Ok(json!({})),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let script = args.first().cloned().unwrap_or_default();
    if args.iter().any(|a| a == "--version") {
        println!("codex-cli 9.9.9");
        return;
    }
    if args.iter().any(|a| a == "login") {
        let logged_in = script != "nologin";
        println!(
            "{}",
            if logged_in {
                "Logged in using ChatGPT"
            } else {
                "Not logged in"
            }
        );
        std::process::exit(i32::from(!logged_in));
    }
    let mut state = String::new();
    while let Some(message) = read() {
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if method == "turn/start" {
            send(
                &json!({ "id": id, "result": { "turn": { "id": "u1", "status": "inProgress", "items": [] } } }),
            );
            let text = params
                .pointer("/input/0/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arg = args.get(1).map_or("", String::as_str);
            let Some(reply) = turn(&script, text, &state, arg) else {
                continue;
            };
            said(&reply);
            completed("completed");
            continue;
        }
        match answer(&script, method, &params, &mut state) {
            Ok(result) => send(&json!({ "id": id, "result": result })),
            Err(message) => {
                send(&json!({ "id": id, "error": { "code": -32600, "message": message } }));
            }
        }
    }
}
