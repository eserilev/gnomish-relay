//! The model route of the story program (SPEC.md 9.7, decision 10): `claude -p` with
//! no tools against the scripted fake `claude`, and a local model through `curl`
//! against a fake model server. The live tests are `#[ignore]`.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fake_model;

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use bridge::agent::StopSignal;
use bridge::claude::NO_TOOLS;
use bridge::model_local::{self, LocalModel};
use bridge::process::OVER_LIMIT;
use bridge::program::find_program;
use bridge::turn::{STOPPED, TIMED_OUT};
use bridge::{model, model_claude};
use fake_model::Answer;
use serde_json::Value;

const PROMPT: &str = "a prompt that never shows in the arguments";

fn fake_claude(script: &[&str]) -> Vec<String> {
    let mut command = vec![env!("CARGO_BIN_EXE_fake-claude").to_owned()];
    command.extend(script.iter().map(|a| (*a).to_owned()));
    command
}

fn ask_claude(script: &[&str], timeout: Duration) -> Result<String, String> {
    model_claude::ask(
        &fake_claude(script),
        None,
        PROMPT,
        timeout,
        StopSignal::default(),
    )
}

#[test]
fn the_gate_denies_every_tool_on_the_story_route() {
    let answer = ask_claude(
        &["tool", "Read", r#"{"file_path":"notes.txt"}"#],
        Duration::from_secs(20),
    );

    assert_eq!(answer.unwrap(), format!("deny: {NO_TOOLS}"));
}

#[test]
fn a_tool_that_ran_with_no_hook_stops_the_call() {
    let answer = ask_claude(&["nohook"], Duration::from_secs(20));

    assert!(answer.unwrap_err().contains("no check"));
}

#[test]
fn claude_runs_in_an_empty_private_folder_that_goes_away_and_never_sees_the_prompt_in_argv() {
    let answer = ask_claude(&["where"], Duration::from_secs(20)).unwrap();

    let seen: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(seen["entries"], serde_json::json!([]));
    if cfg!(unix) {
        assert_eq!(seen["mode"], 0o700);
    }
    let folder = PathBuf::from(seen["cwd"].as_str().unwrap());
    assert!(!folder.exists(), "{} is still there", folder.display());
    let args: Vec<&str> = seen["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(!args.iter().any(|a| a.contains(PROMPT)), "{args:?}");
    assert!(args.contains(&"--safe-mode"));
    let env = seen["env"].to_string();
    assert!(!env.contains("CARGO_MANIFEST_DIR"), "{env}");
}

#[test]
fn a_claude_call_that_hangs_times_out() {
    let started = Instant::now();

    let answer = ask_claude(&["hang"], Duration::from_secs(1));

    assert_eq!(answer.unwrap_err(), TIMED_OUT);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_stop_ends_a_claude_call_at_once() {
    let stop = StopSignal::default();
    let signal = stop.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        signal.request();
    });
    let started = Instant::now();

    let answer = model_claude::ask(
        &fake_claude(&["hang"]),
        None,
        PROMPT,
        Duration::from_mins(1),
        stop,
    );

    assert_eq!(answer.unwrap_err(), STOPPED);
    assert!(started.elapsed() < Duration::from_secs(5));
}

fn local(server: &fake_model::Server) -> LocalModel {
    LocalModel {
        url: server.url.clone(),
        model: "llama3.2".into(),
    }
}

fn ask_local(answer: Answer, timeout: Duration) -> (Result<String, String>, fake_model::Server) {
    let server = fake_model::start(answer);
    let result = model_local::ask(&local(&server), PROMPT, timeout, StopSignal::default());
    (result, server)
}

#[test]
fn a_local_model_answers_through_curl_with_the_prompt_in_the_body() {
    let (answer, server) = ask_local(Answer::Normal, Duration::from_secs(20));

    assert_eq!(answer.unwrap(), format!("heard: {PROMPT}"));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert!(requests[0].body.contains("\"model\":\"llama3.2\""));
}

#[test]
fn a_slow_local_model_times_out() {
    let started = Instant::now();

    let (answer, _server) = ask_local(
        Answer::Slow(Duration::from_secs(30)),
        Duration::from_secs(1),
    );

    assert!(answer.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_huge_local_answer_is_refused() {
    let (answer, _server) = ask_local(Answer::Huge, Duration::from_secs(20));

    assert_eq!(answer.unwrap_err(), OVER_LIMIT);
}

#[test]
fn a_local_answer_that_is_not_json_fails() {
    let (answer, _server) = ask_local(Answer::Garbage, Duration::from_secs(20));

    assert!(answer.unwrap_err().contains("cannot read"));
}

#[test]
fn curl_never_follows_a_redirect() {
    let (answer, server) = ask_local(Answer::Redirect, Duration::from_secs(20));

    assert!(answer.is_err());
    assert_eq!(server.paths(), ["/v1/chat/completions"]);
}

#[test]
fn a_local_model_that_fails_with_500_fails_the_call() {
    let (answer, _server) = ask_local(Answer::Error500, Duration::from_secs(20));

    assert!(answer.unwrap_err().starts_with("The command failed"));
}

#[test]
fn control_characters_of_a_local_answer_go() {
    let (answer, _server) = ask_local(Answer::Controls, Duration::from_secs(20));

    assert_eq!(model::clean_answer(&answer.unwrap()), "ab\nc[31m");
}

fn curl() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_program("curl", &path, cfg!(windows)).unwrap()
}

#[test]
fn a_proxy_in_the_environment_of_curl_is_ignored() {
    let server = fake_model::start(Answer::Normal);
    let (proxy, hits) = fake_model::proxy();
    let timeout = Duration::from_secs(20);
    let mut command = model_local::curl_command(&curl(), &server.url, timeout);
    for name in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        command.env(name, &proxy);
    }

    let answer = model_local::send(command, "m", PROMPT, timeout, StopSignal::default());

    assert_eq!(answer.unwrap(), format!("heard: {PROMPT}"));
    assert_eq!(*hits.lock().unwrap(), 0);
}

#[test]
fn a_stop_ends_a_local_call_at_once() {
    let server = fake_model::start(Answer::Slow(Duration::from_secs(30)));
    let stop = StopSignal::default();
    let signal = stop.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        signal.request();
    });
    let started = Instant::now();

    let answer = model_local::ask(&local(&server), PROMPT, Duration::from_mins(1), stop);

    assert_eq!(answer.unwrap_err(), STOPPED);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// The real `claude` with no tools gets a prompt that asks it to read a file. The
/// answer must not hold the secret of the file.
#[test]
#[ignore = "live: needs the real claude and a login"]
fn live_claude_with_no_tools_cannot_read_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "the secret number is 4417").unwrap();
    let prompt = format!(
        "Read the file {} with your Read tool and tell me the secret number in it. If you cannot, say CANNOT.",
        secret.display()
    );

    let answer = model_claude::ask(
        &["claude".to_owned()],
        Some("haiku"),
        &prompt,
        Duration::from_mins(2),
        StopSignal::default(),
    );

    let answer = answer.unwrap();
    eprintln!("claude answered: {answer}");
    assert!(!answer.is_empty());
    assert!(!answer.contains("4417"), "{answer}");
}

/// A real Ollama, when one listens on 127.0.0.1:11434. With none, the test passes.
#[test]
#[ignore = "live: needs Ollama"]
fn live_ollama_answers_a_prompt() {
    if std::net::TcpStream::connect("127.0.0.1:11434").is_err() {
        eprintln!("no Ollama on 127.0.0.1:11434, skipped");
        return;
    }
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".into());
    let local = LocalModel {
        url: "http://127.0.0.1:11434".into(),
        model,
    };

    let answer = model_local::ask(
        &local,
        "Say hello.",
        Duration::from_mins(2),
        StopSignal::default(),
    );

    assert!(!answer.unwrap().is_empty());
}
