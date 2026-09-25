//! A scripted `claude -p` for the tests of `claude.rs`. The first argument names the
//! script, and the bridge adds its own arguments after it. It is test code: nothing in
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

/// The value after `flag`, or "none".
fn flag(args: &[String], flag: &str) -> String {
    let at = args.iter().position(|a| a == flag);
    at.and_then(|at| args.get(at + 1))
        .cloned()
        .unwrap_or_else(|| "none".into())
}

fn said(content: &Value) {
    send(&json!({ "type": "assistant", "message": { "role": "assistant", "content": content } }));
}

fn result(session: &str, text: &str) {
    send(
        &json!({ "type": "result", "subtype": "success", "is_error": false, "result": text, "session_id": session }),
    );
}

/// Asks the bridge about a tool call, and returns its answer.
fn ask(tool: &str, input: &Value) -> Option<Value> {
    send(
        &json!({ "type": "control_request", "request_id": "perm1", "request": {
            "subtype": "can_use_tool", "tool_name": tool, "input": input,
            "description": "clean the build",
            "permission_suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash" }], "behavior": "allow", "destination": "localSettings" }],
        }}),
    );
    let answer = read()?;
    answer.pointer("/response/response").cloned()
}

fn reply(script: &str, args: &[String], prompt: &str, session: &str) -> Option<String> {
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
        "failed" => {
            send(
                &json!({ "type": "result", "subtype": "success", "is_error": true, "result": "Invalid API key · Please run /login", "session_id": session }),
            );
            None
        }
        "steps" => {
            said(&json!([
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "Let me look. " },
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "cargo test", "description": "Run the tests" } },
                { "type": "tool_use", "id": "t2", "name": "Edit", "input": { "file_path": "src/main.rs" } },
            ]));
            Some(String::new())
        }
        "permission" => {
            let input = json!({ "command": "rm -rf build" });
            let answer = ask("Bash", &input)?;
            let behavior = answer
                .get("behavior")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let same = answer.get("updatedInput") == Some(&input);
            let why = answer.get("message").and_then(Value::as_str).unwrap_or("");
            let rules = answer.to_string().contains("addRules");
            Some(format!("{behavior} same={same} rules={rules} {why}"))
        }
        "hook" => {
            send(
                &json!({ "type": "control_request", "request_id": "h1", "request": { "subtype": "hook_callback", "callback_id": "x" } }),
            );
            let answer = read()?;
            let subtype = answer
                .pointer("/response/subtype")
                .and_then(Value::as_str)
                .unwrap_or("?");
            Some(format!("hook answer {subtype}"))
        }
        "slow" => {
            // Waits for the interrupt, then ends the turn as Claude Code does.
            while let Some(message) = read() {
                if message.pointer("/request/subtype").and_then(Value::as_str) == Some("interrupt")
                {
                    send(
                        &json!({ "type": "control_response", "response": { "subtype": "success", "request_id": message["request_id"] } }),
                    );
                    break;
                }
            }
            send(
                &json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "session_id": session }),
            );
            None
        }
        _ => {
            let secret = if std::env::var_os("CARGO_MANIFEST_DIR").is_some() {
                "leaked"
            } else {
                "hidden"
            };
            let job = std::env::var("GNOMISH_RELAY_JOB").unwrap_or_default();
            let mode = flag(args, "--permission-mode");
            let resume = flag(args, "--resume");
            let tool = flag(args, "--permission-prompt-tool");
            Some(format!(
                "you said: {prompt} [mode={mode} resume={resume} tool={tool} secret={secret} job={job}]"
            ))
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let script = args.first().cloned().unwrap_or_default();
    if args.iter().any(|a| a == "--version") {
        println!("9.9.9 (Claude Code)");
        return;
    }
    if args.iter().any(|a| a == "auth") {
        let logged_in = script != "nologin";
        println!(
            "{}",
            json!({ "loggedIn": logged_in, "authMethod": "claude.ai" })
        );
        std::process::exit(i32::from(!logged_in));
    }
    let resume = flag(&args, "--resume");
    let session = if resume == "none" {
        "s1".to_owned()
    } else {
        resume
    };
    let Some(init) = read() else { return };
    if script == "badinit" {
        send(
            &json!({ "type": "control_response", "response": { "subtype": "error", "request_id": init["request_id"], "error": "no hooks here" } }),
        );
        return;
    }
    send(
        &json!({ "type": "control_response", "response": { "subtype": "success", "request_id": init["request_id"], "response": {} } }),
    );
    let Some(prompt) = read() else { return };
    let text = prompt
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    send(
        &json!({ "type": "system", "subtype": "init", "session_id": session, "cwd": ".", "tools": [] }),
    );
    send(&json!({ "type": "rate_limit_event", "rate_limit_info": {} }));
    let Some(reply) = reply(&script, &args, text, &session) else {
        return;
    };
    if !reply.is_empty() {
        said(&json!([{ "type": "text", "text": reply }]));
    }
    result(&session, &reply);
}
