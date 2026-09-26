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

/// The `PreToolUse` hook that the bridge registered in `initialize`, with its timeout.
struct Hook {
    id: String,
    timeout: u64,
}

fn hook_of(init: &Value) -> Option<Hook> {
    let matcher = init.pointer("/request/hooks/PreToolUse/0")?;
    Some(Hook {
        id: matcher.pointer("/hookCallbackIds/0")?.as_str()?.to_owned(),
        timeout: matcher.get("timeout")?.as_u64()?,
    })
}

fn tool_result(id: &str, is_error: bool) {
    send(
        &json!({ "type": "user", "message": { "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": id, "content": "done", "is_error": is_error },
        ]}}),
    );
}

/// Calls a tool as Claude Code does: the tool use, the hook, then the result.
fn use_tool(hook: Option<&Hook>, tool: &str, input: &Value) -> Option<String> {
    said(&json!([{ "type": "tool_use", "id": "tu1", "name": tool, "input": input }]));
    let Some(hook) = hook else {
        tool_result("tu1", false);
        return Some("no hook".into());
    };
    send(
        &json!({ "type": "control_request", "request_id": "hk1", "request": {
            "subtype": "hook_callback", "callback_id": hook.id, "tool_use_id": "tu1",
            "input": { "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input, "tool_use_id": "tu1", "cwd": "." },
        }}),
    );
    let answer = read()?;
    let output = answer.pointer("/response/response/hookSpecificOutput")?;
    let decision = output.get("permissionDecision")?.as_str()?;
    let reason = output.get("permissionDecisionReason")?.as_str()?;
    tool_result("tu1", decision != "allow");
    Some(format!("{decision}: {reason}"))
}

/// The scripts of the gate: a tool call through the hook, and hooks that go wrong.
fn gate_reply(script: &str, args: &[String], hook: Option<&Hook>) -> Option<String> {
    match script {
        "tool" => {
            let tool = args.get(1)?;
            let input: Value = serde_json::from_str(args.get(2)?).ok()?;
            use_tool(hook, tool, &input)
        }
        "hookinfo" => hook.map(|h| format!("hook={} timeout={}", h.id, h.timeout)),
        "nohook" => {
            said(
                &json!([{ "type": "tool_use", "id": "tu9", "name": "Read", "input": { "file_path": "x" } }]),
            );
            tool_result("tu9", false);
            Some("read with no hook".into())
        }
        "badhook" => {
            send(
                &json!({ "type": "control_request", "request_id": "hk2", "request": {
                    "subtype": "hook_callback", "callback_id": "gate", "input": { "hook_event_name": "PreToolUse" },
                }}),
            );
            let answer = read()?;
            let decision =
                answer.pointer("/response/response/hookSpecificOutput/permissionDecision");
            Some(format!(
                "bad hook: {}",
                decision.and_then(Value::as_str).unwrap_or("none")
            ))
        }
        _ => None,
    }
}

/// Its working folder, what is in it, its mode, its arguments, and the names of its
/// environment, as JSON: the checks of the model route of the story program.
fn whereabouts(args: &[String]) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let entries: Vec<String> = std::fs::read_dir(&cwd)
        .map(|dir| {
            dir.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(&cwd).map_or(0, |m| m.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode = 0;
    let env: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    json!({ "cwd": cwd, "entries": entries, "mode": mode, "args": args, "env": env }).to_string()
}

fn reply(
    script: &str,
    args: &[String],
    prompt: &str,
    session: &str,
    hook: Option<&Hook>,
) -> Option<String> {
    match script {
        "tool" | "hookinfo" | "nohook" | "badhook" => gate_reply(script, args, hook),
        "hang" => loop {
            std::thread::park();
        },
        "where" => Some(whereabouts(args)),
        "hang-pid" => {
            let _ = std::fs::write(args.get(1)?, std::process::id().to_string());
            loop {
                std::thread::park();
            }
        }
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
                &json!({ "type": "control_request", "request_id": "h1", "request": { "subtype": "mcp_message", "server_name": "x" } }),
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
    let hook = hook_of(&init);
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
    let Some(reply) = reply(&script, &args, text, &session, hook.as_ref()) else {
        return;
    };
    if !reply.is_empty() {
        said(&json!([{ "type": "text", "text": reply }]));
    }
    result(&session, &reply);
}
