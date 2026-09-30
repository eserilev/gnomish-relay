//! The free local model of setup (SPEC.md 11.6), with a fake installer and a fake Ollama
//! on 127.0.0.1. No test downloads anything.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bridge::config_text::timeways_config;
use bridge::model_setup::FoundModel;
use bridge::ollama_install::{self, LOCAL_STORY_MODEL, Ollama};
use bridge::program::find_program;
use bridge::setup;

#[derive(Clone, Copy)]
enum Pull {
    Done,
    Refused,
    Cut,
}

#[derive(Clone, Copy)]
enum Chat {
    Answers,
    Fails,
}

/// A fake Ollama: `/api/version`, a streamed `/api/pull`, and `/v1/chat/completions`.
struct FakeOllama {
    url: String,
    /// The path and the body of each request.
    requests: Arc<Mutex<Vec<(String, String)>>>,
}

impl FakeOllama {
    fn start(pull: Pull, chat: Chat) -> FakeOllama {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let seen = Arc::clone(&seen);
                thread::spawn(move || serve(stream, pull, chat, &seen));
            }
        });
        FakeOllama { url, requests }
    }

    fn bodies_of(&self, path: &str) -> Vec<String> {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, body)| body.clone())
            .collect()
    }
}

fn read_request(stream: &TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let path = first.split(' ').nth(1).unwrap_or_default().to_owned();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header.trim().is_empty() {
            break;
        }
        let lower = header.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (path, String::from_utf8(body).unwrap())
}

fn serve(mut stream: TcpStream, pull: Pull, chat: Chat, seen: &Mutex<Vec<(String, String)>>) {
    let (path, body) = read_request(&stream);
    seen.lock().unwrap().push((path.clone(), body));
    let (status, text) = match path.as_str() {
        "/api/version" => ("200 OK", r#"{"version":"0.12.0"}"#.to_owned()),
        "/api/pull" => ("200 OK", pull_stream(pull)),
        "/v1/chat/completions" => match chat {
            Chat::Answers => (
                "200 OK",
                r#"{"choices":[{"message":{"role":"assistant","content":"Well met, traveler, well met."}}]}"#
                    .to_owned(),
            ),
            Chat::Fails => ("500 Internal Server Error", "{}".to_owned()),
        },
        _ => ("404 Not Found", String::new()),
    };
    let head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(text.as_bytes());
}

fn pull_stream(pull: Pull) -> String {
    let start = "{\"status\":\"pulling manifest\"}\n\
        {\"status\":\"pulling a\",\"digest\":\"sha256:a\",\"total\":2000000000,\"completed\":0}\n\
        {\"status\":\"pulling a\",\"digest\":\"sha256:a\",\"total\":2000000000,\"completed\":1000000000}\n";
    let end = match pull {
        Pull::Done => "{\"status\":\"verifying sha256 digest\"}\n{\"status\":\"success\"}\n",
        Pull::Refused => "{\"error\":\"no space left on device\"}\n",
        Pull::Cut => "",
    };
    format!("{start}{end}")
}

fn curl() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_program("curl", &path, cfg!(windows))
}

fn ollama_at(url: &str) -> Option<Ollama> {
    Some(Ollama {
        curl: curl()?,
        url: url.to_owned(),
    })
}

fn closed_port() -> String {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    format!("http://127.0.0.1:{port}")
}

#[cfg(unix)]
fn fake_installer(dir: &std::path::Path, script: &str) -> PathBuf {
    let file = dir.join("install.sh");
    std::fs::write(&file, script).unwrap();
    file
}

#[cfg(unix)]
#[test]
fn the_installer_runs_and_setup_waits_until_ollama_answers() {
    let server = FakeOllama::start(Pull::Done, Chat::Answers);
    let Some(ollama) = ollama_at(&server.url) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ran = dir.path().join("ran");
    let script = fake_installer(dir.path(), &format!("touch '{}'\n", ran.display()));
    let installer = ollama_install::run_command(ollama_install::Os::LinuxOrMac, &script);

    ollama_install::install_and_start(&ollama, installer, Duration::ZERO).unwrap();

    assert!(ran.exists());
    assert_eq!(server.bodies_of("/api/version").len(), 1);
}

#[cfg(unix)]
#[test]
fn a_failed_installer_stops_with_its_exit_status() {
    let server = FakeOllama::start(Pull::Done, Chat::Answers);
    let Some(ollama) = ollama_at(&server.url) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let script = fake_installer(dir.path(), "exit 3\n");
    let installer = ollama_install::run_command(ollama_install::Os::LinuxOrMac, &script);

    let error = ollama_install::install_and_start(&ollama, installer, Duration::ZERO).unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("The Ollama installer stopped with"),
        "{error}"
    );
    assert!(error.to_string().contains('3'), "{error}");
    assert!(server.bodies_of("/api/version").is_empty());
}

#[cfg(unix)]
#[test]
fn ollama_that_does_not_start_after_the_installer_is_an_error() {
    let Some(ollama) = ollama_at(&closed_port()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let script = fake_installer(dir.path(), "exit 0\n");
    let installer = ollama_install::run_command(ollama_install::Os::LinuxOrMac, &script);

    let error = ollama_install::install_and_start(&ollama, installer, Duration::ZERO).unwrap_err();

    assert_eq!(error.to_string(), "Ollama didn't start.");
}

#[test]
fn a_running_ollama_answers_and_a_closed_port_does_not() {
    let server = FakeOllama::start(Pull::Done, Chat::Answers);
    let (Some(running), Some(closed)) = (ollama_at(&server.url), ollama_at(&closed_port())) else {
        return;
    };

    assert!(ollama_install::server_answers(&running));
    assert!(!ollama_install::server_answers(&closed));
}

#[test]
fn a_pull_asks_for_the_model_by_name_and_shows_the_megabytes() {
    let server = FakeOllama::start(Pull::Done, Chat::Answers);
    let Some(ollama) = ollama_at(&server.url) else {
        return;
    };
    let mut seen = Vec::new();

    ollama_install::pull(&ollama, LOCAL_STORY_MODEL, &mut |done, all| {
        seen.push((done, all));
    })
    .unwrap();

    assert_eq!(seen, [(0, 2000), (1000, 2000)]);
    let body: serde_json::Value = serde_json::from_str(&server.bodies_of("/api/pull")[0]).unwrap();
    assert_eq!(body["model"], LOCAL_STORY_MODEL);
    assert_eq!(body["stream"], true);
}

#[test]
fn a_pull_that_ollama_refuses_is_an_error_with_its_reason() {
    let server = FakeOllama::start(Pull::Refused, Chat::Answers);
    let Some(ollama) = ollama_at(&server.url) else {
        return;
    };

    let error = ollama_install::pull(&ollama, LOCAL_STORY_MODEL, &mut |_, _| {}).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("Ollama couldn't download {LOCAL_STORY_MODEL}: no space left on device")
    );
}

#[test]
fn a_pull_that_stops_before_its_end_is_an_error() {
    let server = FakeOllama::start(Pull::Cut, Chat::Answers);
    let Some(ollama) = ollama_at(&server.url) else {
        return;
    };

    let error = ollama_install::pull(&ollama, LOCAL_STORY_MODEL, &mut |_, _| {}).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("The download of {LOCAL_STORY_MODEL} stopped before its end.")
    );
}

/// A config folder with the `[story]` of a setup that found no model.
fn config_with_no_model(home: &std::path::Path) -> PathBuf {
    let dir = home.join("config");
    let text = timeways_config(&home.join("wow"), &[]);
    setup::write_config(&dir, &text, home).unwrap();
    dir
}

#[test]
fn the_pulled_model_goes_into_the_story_config_and_answers_through_the_story_route() {
    let server = FakeOllama::start(Pull::Done, Chat::Answers);
    if curl().is_none() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let dir = config_with_no_model(home.path());
    let model = FoundModel::Local {
        url: server.url.clone(),
        model: LOCAL_STORY_MODEL.into(),
    };

    let config = setup::write_story_model(&dir, &model, home.path()).unwrap();
    ollama_install::check_answer(&config).unwrap();

    let reloaded = bridge::config::load(&dir, home.path()).unwrap();
    assert_eq!(reloaded.story, config.story);
    let asked = &server.bodies_of("/v1/chat/completions")[0];
    let body: serde_json::Value = serde_json::from_str(asked).unwrap();
    assert_eq!(body["model"], LOCAL_STORY_MODEL);
}

#[test]
fn a_model_that_fails_the_test_prompt_is_an_error() {
    let server = FakeOllama::start(Pull::Done, Chat::Fails);
    if curl().is_none() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let dir = config_with_no_model(home.path());
    let model = FoundModel::Local {
        url: server.url.clone(),
        model: LOCAL_STORY_MODEL.into(),
    };
    let config = setup::write_story_model(&dir, &model, home.path()).unwrap();

    assert!(ollama_install::check_answer(&config).is_err());
}

#[test]
fn a_config_with_no_local_model_has_nothing_to_test() {
    let home = tempfile::tempdir().unwrap();
    let dir = config_with_no_model(home.path());
    let config = bridge::config::load(&dir, home.path()).unwrap();

    let error = ollama_install::check_answer(&config).unwrap_err();

    assert_eq!(error.to_string(), "The config has no local model.");
}

/// With no terminal, setup asks nothing, downloads nothing, and names the command for
/// later. No `curl` on the `PATH`, so no download can start.
#[cfg(target_os = "linux")]
#[test]
fn setup_with_no_terminal_downloads_nothing_and_names_the_command_for_later() {
    let home = tempfile::tempdir().unwrap();
    let timeways = home.path().join("wow/Interface/AddOns/Timeways");
    std::fs::create_dir_all(&timeways).unwrap();
    std::fs::write(timeways.join("Timeways.toc"), "## Title: x\n").unwrap();
    let empty = home.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("setup")
        .arg(home.path().join("wow"))
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("PATH", &empty)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains(
            "No AI model found. Timeways works without one, but it writes no story text.\n\
             To install a free local model, run gnomish-relay setup --timeways in a terminal\n"
        ),
        "{stdout}"
    );
    assert!(!stdout.contains("Install a free local model?"), "{stdout}");
    assert!(!stdout.contains("Ollama"), "{stdout}");
    let config = bridge::config::load(&home.path().join("config/gnomish-relay"), home.path());
    let story = config.unwrap().story.unwrap();
    assert_eq!(story.model.choice, bridge::model::ModelChoice::None);
}

#[test]
fn a_story_model_that_does_not_load_is_never_written() {
    let home = tempfile::tempdir().unwrap();
    let dir = config_with_no_model(home.path());
    let before = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    let model = FoundModel::Local {
        url: "http://localhost:11434".into(),
        model: LOCAL_STORY_MODEL.into(),
    };

    assert!(setup::write_story_model(&dir, &model, home.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        before
    );
}
