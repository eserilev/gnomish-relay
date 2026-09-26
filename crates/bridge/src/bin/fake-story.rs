//! A scripted Timeways story program for the tests of `story.rs`, with the shapes of
//! SPEC.md 9.8. The first argument names the script. It is test code: the bridge starts
//! it only when a test names it.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

fn send_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn send(message: &Value) {
    send_line(&message.to_string());
}

fn hello(protocol: u32) {
    send(&json!({ "type": "hello", "protocol": protocol }));
}

fn answer_to(id: &Value, text: &Value) {
    send(&json!({
        "type": "lore_answer", "id": id, "text": text,
        "passages": [{ "text": "The portal hums.", "source": "https://example.test/portal" }],
    }));
}

fn answer(line: &Value, text: &str) {
    answer_to(&line["id"], &json!(text));
}

fn question(line: &Value) -> &str {
    line["question"].as_str().unwrap_or_default()
}

/// The journal answers at once, with no model call. The asked page is its last page.
fn journal(line: &Value) {
    let page = line["page"].as_u64().unwrap_or_default();
    send(&json!({
        "type": "journal", "id": line["id"], "page": page, "pages": page + 1,
        "places": [{ "name": "Goldshire", "within": "Elwynn Forest", "first_visit": 100 }],
        "people": [{
            "name": "Marshal Dughan", "place": null, "first_met": 101, "trust": 40, "slapped": null,
        }],
        "deeds": [{ "kind": "level", "from": null, "to": 2, "at": 102, "place": "Goldshire" }],
        "chapters": [{
            "number": 1, "began": 100, "zones": ["Elwynn Forest"], "people": ["Marshal Dughan"],
            "deeds": [
                { "kind": "level", "from": 1, "to": 2, "at": 102, "place": null },
                { "kind": "defeated", "foe": "Hogger", "times": 2 },
                { "kind": "died", "killer": "Hogger" },
            ],
            "left_out": 0, "prose": "The road to Goldshire | began.",
        }],
    }));
}

/// An answer line of 24577 bytes, one over the limit of the bridge. The text takes the
/// place of `null`, so it is 2 bytes longer than what it adds.
fn too_long(line: &Value) {
    let passages: Vec<Value> = (0..6)
        .map(|_| json!({ "text": "p".repeat(3500), "source": "" }))
        .collect();
    let mut answer =
        json!({ "type": "lore_answer", "id": line["id"], "text": null, "passages": passages });
    let with_null = answer.to_string().len();
    answer["text"] = json!("t".repeat(24_577 + 2 - with_null));
    send(&answer);
}

/// Its own folder is its working folder, and the only folder it can write.
fn remember(line: &str) {
    let seen = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("seen.txt");
    if let Ok(mut seen) = seen {
        let _ = writeln!(seen, "{line}");
    }
}

fn crash() -> ! {
    eprintln!("the story crashed on purpose");
    std::process::exit(3);
}

/// The question holds checks divided by `|`: `write:<path>`, `read:<path>`, or
/// `connect:<address>`. The answer has one line for each: `ok` or `denied`.
fn probe(line: &Value) {
    let results: Vec<&str> = question(line)
        .split('|')
        .map(|check| if try_check(check) { "ok" } else { "denied" })
        .collect();
    answer(line, &results.join("\n"));
}

fn try_check(check: &str) -> bool {
    let Some((what, target)) = check.split_once(':') else {
        return false;
    };
    match what {
        "write" => std::fs::write(target, "from the story").is_ok(),
        "read" => std::fs::read(target).is_ok(),
        "connect" => target.parse().is_ok_and(|address| {
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok()
        }),
        _ => false,
    }
}

fn environment(line: &Value) {
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    answer(line, &names.join(","));
}

/// Starts a child that sleeps, and writes the id of the child into `child.pid`.
fn fork() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(child) = std::process::Command::new(exe).arg("sleep").spawn() else {
        return;
    };
    let _ = std::fs::write("child.pid", child.id().to_string());
}

/// The scripts that act on each line, game event or question.
fn on_any_line(script: &str) {
    match script {
        "crash" => crash(),
        "crash-once" if !Path::new("crashed").exists() => {
            let _ = std::fs::write("crashed", "");
            crash();
        }
        _ => {}
    }
}

fn on_reply_line(script: &str, line: &Value) {
    match script {
        "garbage" => {
            send_line("not json");
            send_line(r#"{"type":"lore_answer"}"#);
            send_line(r#"{"type":"shell","command":"rm -rf ~"}"#);
            answer(line, &format!("story: {}", question(line)));
        }
        "flood" => (0..50).for_each(|_| send_line("garbage")),
        "huge" => {
            send_line(&"x".repeat(2 * 1024 * 1024));
            answer(line, &format!("story: {}", question(line)));
        }
        "hang" => {}
        "fork" => fork(),
        "strangers" => {
            let id = line["id"].as_u64().unwrap_or_default();
            (0..20).for_each(|n| answer_to(&json!(id + 1000 + n), &json!("for nobody")));
        }
        "wrong-id" => {
            let id = line["id"].as_u64().unwrap_or_default();
            answer_to(&json!(id + 1), &json!("wrong id"));
            answer(line, &format!("story: {}", question(line)));
        }
        "null-text" => answer_to(&line["id"], &Value::Null),
        "twice" => {
            answer(line, "first");
            answer(line, "second");
        }
        "companion" | "long-companion" => send(&json!({
            "type": "lore_answer", "id": line["id"], "text": "story", "passages": [],
            "companion": companion(script),
        })),
        "model" => send(&json!({ "type": "model_call", "call": 1, "prompt": "tell a story" })),
        "env" => environment(line),
        "probe" => probe(line),
        "too-long" => too_long(line),
        _ if line["type"] == "journal_asked" => journal(line),
        _ if line["type"] == "talk_asked" => talk(line),
        _ => answer(line, &format!("story: {}", question(line))),
    }
}

fn companion(script: &str) -> Value {
    match script {
        "companion" => json!("A wolf howls."),
        "long-companion" => json!("c".repeat(1001)),
        _ => Value::Null,
    }
}

/// A batch with no line that wants a reply gets `events_seen` after its end. `asked` is
/// the last line that wanted a reply.
fn on_batch_end(script: &str, end: &Value) {
    let ends = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("ends.txt");
    if let Ok(mut ends) = ends {
        let _ = writeln!(ends, "{}", end["id"]);
    }
    match script {
        "missing" => return,
        "late" => std::thread::sleep(Duration::from_secs(2)),
        _ => {}
    }
    send(&json!({ "type": "events_seen", "id": end["id"], "companion": companion(script) }));
    if script == "bard" {
        // A bard call belongs to no batch. The third one finds two calls open.
        (1..=3).for_each(|call| {
            send(&json!({ "type": "model_call", "call": call, "prompt": "sing a saga" }));
        });
    }
}

fn talk(line: &Value) {
    let npc = line["npc"].as_str().unwrap_or_default();
    let text = line["text"].as_str().unwrap_or_default();
    send(
        &json!({ "type": "talk_answer", "id": line["id"], "npc": npc, "text": format!("{npc}: {text}") }),
    );
}

/// Stops at the end of its input, as the protocol asks: the bridge is gone.
fn main() {
    let script = std::env::args().nth(1).unwrap_or_default();
    if script == "sleep" {
        std::thread::sleep(Duration::from_mins(5));
        return;
    }
    let mut lines = std::io::stdin().lock().lines();
    let Some(Ok(_bridge_hello)) = lines.next() else {
        return;
    };
    match script.as_str() {
        "no-hello" => {}
        "newer" => hello(2),
        "older" => hello(0),
        _ => hello(1),
    }
    let mut asked = Value::Null;
    for text in lines.map_while(Result::ok) {
        let Ok(line) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if line["type"] == "batch_end" {
            on_batch_end(&script, &line);
            continue;
        }
        remember(&text);
        match line["type"].as_str() {
            Some("model_failed") if script == "model" => answer_to(&asked["id"], &Value::Null),
            Some("model_failed") => {}
            Some("lore_asked" | "journal_asked" | "talk_asked") => {
                on_any_line(&script);
                asked = line.clone();
                on_reply_line(&script, &line);
            }
            _ => on_any_line(&script),
        }
    }
}
