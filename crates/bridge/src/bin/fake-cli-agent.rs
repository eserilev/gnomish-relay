//! A scripted command-line harness for the tests of the `command` backend. The first
//! argument names the script. The message comes after it, or after `--file` as a file,
//! or on stdin when no argument holds it. It is test code: nothing in the bridge starts it.

#[path = "shared/net_probe.rs"]
mod net_probe;

use std::io::{Read, Write};
use std::time::Duration;

/// The message, and whether `--continue` came.
struct Message {
    text: String,
    goes_on: bool,
}

fn message(args: &[String]) -> Message {
    let goes_on = args.iter().any(|a| a == "--continue");
    let words: Vec<&String> = args.iter().filter(|a| *a != "--continue").collect();
    let text = match words.as_slice() {
        [flag, path] if *flag == "--file" => std::fs::read_to_string(path).unwrap_or_default(),
        [] => {
            let mut text = String::new();
            let _ = std::io::stdin().read_to_string(&mut text);
            text
        }
        more => more
            .iter()
            .map(|w| w.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    };
    Message {
        text: text.trim_end_matches('\n').to_owned(),
        goes_on,
    }
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Three steps with colors and a progress bar, then the answer.
fn stream(text: &str) {
    for step in [
        "\x1b[1mreading main.rs\x1b[0m",
        "10%\r60%\r100%",
        "editing main.rs",
    ] {
        say(step);
        std::thread::sleep(Duration::from_millis(150));
    }
    say(&format!("done: {text}"));
}

/// A line for each run in this folder, so a test sees the history that a harness keeps.
fn history(message: &Message) {
    let file = ".fake-history";
    let old = std::fs::read_to_string(file).unwrap_or_default();
    let runs = old.lines().count() + 1;
    let _ = std::fs::write(file, format!("{old}{}\n", message.text));
    say(&format!("run {runs}, continue {}", message.goes_on));
}

/// A server in the background of the harness, which Stop must end with the harness.
fn spawn_sleeper(marker: &str) {
    let _ = std::process::Command::new("sh")
        .args(["-c", &format!("sleep 3; echo late > {marker}")])
        .spawn();
    say("started");
    std::thread::sleep(Duration::from_mins(1));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (script, rest) = args
        .split_first()
        .map_or(("", &[][..]), |(s, r)| (s.as_str(), r));
    let message = message(rest);
    match script {
        "--version" => say("fake-cli-agent 1.2.3"),
        "echo" => say(&format!("echo: {}", message.text)),
        "args" => say(&format!("{rest:?}")),
        "stream" => stream(&message.text),
        "slow" => std::thread::sleep(Duration::from_mins(1)),
        "sleeper" => spawn_sleeper(&message.text),
        "huge" => {
            let line = "x".repeat(1023);
            let mut out = std::io::stdout().lock();
            let mib: usize = message.text.parse().unwrap_or(1);
            for _ in 0..mib * 1024 {
                if writeln!(out, "{line}").is_err() {
                    return;
                }
            }
            let _ = writeln!(out, "the end");
        }
        "crash" => {
            say("partial output");
            eprintln!("Error: no API key. Set FAKE_API_KEY.");
            std::process::exit(3);
        }
        "env" => {
            let names: Vec<&str> = message.text.split_whitespace().collect();
            let seen: Vec<String> = names
                .iter()
                .map(|n| format!("{n}={}", std::env::var(n).unwrap_or_else(|_| "-".into())))
                .collect();
            say(&seen.join(" "));
        }
        "history" => history(&message),
        "pwd" => say(&std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default()),
        "probe" => say(&net_probe::answer(&message.text).unwrap_or_else(|| "bad probe".into())),
        _ => {
            eprintln!("unknown script {script:?}");
            std::process::exit(2);
        }
    }
}
