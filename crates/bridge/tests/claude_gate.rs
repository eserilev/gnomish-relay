//! The gate on every tool call of native Claude Code, through its `PreToolUse` hook,
//! against the scripted fake `claude` (`src/bin/fake-claude.rs`).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bridge::agent::{Agent, Control, Event, Events, Question, StopSignal};
use bridge::allow::{self, AllowFile};
use bridge::claude::ClaudeAgent;
use bridge::config::Permission;
use bridge::desktop::{self, Approvals, Notice};
use bridge::gate::Gate;
use bridge::relay::{ChatId, Job, MessageId, Session, Work};
use serde_json::json;

/// A home with a code root, a chat folder, the config folder of the bridge, and `~/.ssh`.
struct Home {
    _tmp: tempfile::TempDir,
    path: PathBuf,
    chat: PathBuf,
    gate: Gate,
}

fn home(allow_toml: &str) -> Home {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().canonicalize().unwrap();
    let chat = path.join("Code").join("app");
    let config = path.join(".config").join("gnomish-relay");
    for dir in [&chat, &config, &path.join(".ssh")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(config.join("strip.key"), "not a key").unwrap();
    let file: AllowFile = toml::from_str(allow_toml).unwrap();
    let gate = Gate {
        roots: vec![path.join("Code")],
        config_dir: config,
        allow: Arc::new(allow::parse(&file, &path).unwrap()),
        approvals: Approvals::new(&path.join("data"), Notice::Off),
    };
    Home {
        _tmp: tmp,
        path,
        chat,
        gate,
    }
}

fn claude(home: &Home, args: &[&str], permission_timeout: Duration) -> ClaudeAgent {
    let mut command = vec![env!("CARGO_BIN_EXE_fake-claude").to_owned()];
    command.extend(args.iter().map(|a| (*a).to_owned()));
    ClaudeAgent {
        command,
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(20),
        permission_timeout,
        projects: home.path.join("projects"),
        gate: home.gate.clone(),
    }
}

fn job(home: &Home, permission: Permission) -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId("c1".into()),
        id: MessageId(1),
        agent: "claude".into(),
        permission,
        cwd: home.chat.to_string_lossy().into_owned(),
        session: Session::New,
        resume: None,
        text: "go".into(),
        work: Work::Prompt,
    }
}

/// One tool call with nobody in the game.
fn call(home: &Home, tool: &str, input: &serde_json::Value, level: Permission) -> String {
    let input = input.to_string();
    let claude = claude(home, &["tool", tool, &input], Duration::from_millis(300));
    claude
        .run(&job(home, level), &Control::default())
        .reply
        .unwrap()
}

/// One tool call with the game listening. `answer` answers each question.
fn call_with_game(
    home: &Home,
    tool: &str,
    input: &serde_json::Value,
    level: Permission,
    answer: impl Fn(&Question) -> Option<Option<usize>> + Send + 'static,
) -> (String, Vec<Question>) {
    let input = input.to_string();
    let claude = claude(home, &["tool", tool, &input], Duration::from_secs(10));
    let job = job(home, level);
    let (to, events) = std::sync::mpsc::channel();
    let control = Control {
        stop: StopSignal::default(),
        events: Events::to_bridge(to, &job),
    };
    let seen = std::thread::spawn(move || {
        let mut questions = Vec::new();
        while let Ok((_, _, event)) = events.recv() {
            if let Event::Question(question) = event {
                if let Some(choice) = answer(&question) {
                    question.answer.send(choice).unwrap();
                }
                questions.push(question);
            }
        }
        questions
    });
    let reply = claude.run(&job, &control).reply.unwrap();
    drop(control);
    (reply, seen.join().unwrap())
}

const LEVELS: [Permission; 3] = [Permission::Ask, Permission::AutoEdit, Permission::FullAuto];

#[test]
fn the_hook_is_registered_and_waits_longer_than_the_game() {
    let home = home("");
    let claude = claude(&home, &["hookinfo"], Duration::from_mins(10));
    let reply = claude
        .run(&job(&home, Permission::Ask), &Control::default())
        .reply
        .unwrap();
    assert_eq!(reply, "hook=gate timeout=900");
}

#[test]
fn a_read_of_the_strip_key_is_denied_at_every_level() {
    let home = home("");
    let key = home.path.join(".config/gnomish-relay/strip.key");
    for level in LEVELS {
        let reply = call(&home, "Read", &json!({ "file_path": key }), level);
        assert!(
            reply.starts_with("deny: It touches the config folder"),
            "{reply}"
        );
    }
}

#[test]
fn a_read_of_an_ssh_key_from_the_game_never_runs_and_asks_on_the_desktop() {
    let home = home("");
    let key = home.path.join(".ssh/id_rsa");
    let approvals = home.gate.approvals.clone();
    let desktop = std::thread::spawn(move || {
        for _ in 0..250 {
            if let Some(open) = approvals.list().pop() {
                approvals.answer(&open.id, desktop::Verdict::Deny).unwrap();
                return open.text;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        String::new()
    });
    let (reply, questions) = call_with_game(
        &home,
        "Read",
        &json!({ "file_path": key }),
        Permission::FullAuto,
        |_| None,
    );
    assert!(reply.starts_with("deny: Denied on the desktop."), "{reply}");
    assert!(desktop.join().unwrap().contains(".ssh/id_rsa"));
    let labels: Vec<&str> = questions[0]
        .choices
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(labels, ["Deny"], "the game cannot allow a desktop call");
    assert!(
        questions[0]
            .text
            .starts_with(b"Approve on your desktop: gnomish-relay approve\n")
    );
}

#[test]
fn an_approval_on_the_desktop_runs_the_call() {
    let home = home("");
    let approvals = home.gate.approvals.clone();
    std::thread::spawn(move || {
        for _ in 0..250 {
            if let Some(open) = approvals.list().pop() {
                approvals
                    .answer(&open.id, desktop::Verdict::Approve)
                    .unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let (reply, _) = call_with_game(
        &home,
        "WebFetch",
        &json!({ "url": "https://example.com" }),
        Permission::AutoEdit,
        |_| None,
    );
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
}

#[test]
fn a_write_outside_the_chat_folder_asks_on_the_desktop_and_times_out_as_refused() {
    let home = home("");
    let outside = home.path.join("Code").join("other.rs");
    let reply = call(
        &home,
        "Write",
        &json!({ "file_path": outside }),
        Permission::FullAuto,
    );
    assert!(
        reply.starts_with("deny: No answer on the desktop."),
        "{reply}"
    );
    assert!(home.gate.approvals.list().is_empty(), "the request closes");
}

#[test]
fn an_unknown_tool_asks_on_the_desktop() {
    let home = home("");
    let reply = call(&home, "mcp__web__fetch", &json!({}), Permission::FullAuto);
    assert!(
        reply.starts_with("deny: No answer on the desktop."),
        "{reply}"
    );
}

#[test]
fn cargo_test_in_the_allow_table_runs_with_no_popup_at_auto_edit() {
    let home = home("commands = [\"cargo test *\"]");
    let (reply, questions) = call_with_game(
        &home,
        "Bash",
        &json!({ "command": "cargo test -q" }),
        Permission::AutoEdit,
        |_| Some(Some(1)),
    );
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
    assert!(questions.is_empty());
}

#[test]
fn rm_r_asks_in_the_game_at_auto_edit_and_runs_at_full_auto() {
    let home = home("commands = [\"rm *\"]");
    let rm = json!({ "command": "rm -r build" });
    let (reply, questions) =
        call_with_game(&home, "Bash", &rm, Permission::AutoEdit, |_| Some(Some(0)));
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
    assert_eq!(questions.len(), 1, "the allow table never covers rm -r");
    let (reply, questions) =
        call_with_game(&home, "Bash", &rm, Permission::FullAuto, |_| Some(Some(1)));
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
    assert!(questions.is_empty());
}

#[test]
fn a_deny_in_the_game_reaches_claude() {
    let home = home("");
    let (reply, _) = call_with_game(
        &home,
        "Bash",
        &json!({ "command": "make" }),
        Permission::AutoEdit,
        |_| Some(Some(1)),
    );
    assert_eq!(reply, "deny: Denied in the game.");
}

#[test]
fn at_the_level_ask_a_read_runs_and_an_edit_in_the_folder_asks() {
    let home = home("");
    let read = call(
        &home,
        "Read",
        &json!({ "file_path": "src/a.rs" }),
        Permission::Ask,
    );
    assert_eq!(read, "allow: Allowed by Gnomish Relay.");
    let edit = call(
        &home,
        "Edit",
        &json!({ "file_path": "a.rs" }),
        Permission::Ask,
    );
    assert!(
        edit.starts_with("deny: Not allowed from the game."),
        "{edit}"
    );
    let edit = call(
        &home,
        "Edit",
        &json!({ "file_path": "a.rs" }),
        Permission::AutoEdit,
    );
    assert_eq!(edit, "allow: Allowed by Gnomish Relay.");
}

#[test]
fn a_glob_that_leaves_its_folder_asks_on_the_desktop() {
    let home = home("");
    let inside = call(
        &home,
        "Glob",
        &json!({ "pattern": "**/*.rs" }),
        Permission::Ask,
    );
    assert_eq!(inside, "allow: Allowed by Gnomish Relay.");
    let outside = call(
        &home,
        "Glob",
        &json!({ "pattern": "/etc/**" }),
        Permission::FullAuto,
    );
    assert!(
        outside.starts_with("deny: No answer on the desktop."),
        "{outside}"
    );
}

#[test]
fn a_tool_of_the_session_only_runs_with_no_question() {
    let home = home("");
    let reply = call(
        &home,
        "ToolSearch",
        &json!({ "query": "x" }),
        Permission::Ask,
    );
    assert_eq!(reply, "allow: Allowed by Gnomish Relay.");
}

#[test]
fn a_malformed_hook_request_fails_closed() {
    let home = home("");
    let claude = claude(&home, &["badhook"], Duration::from_millis(300));
    let reply = claude
        .run(&job(&home, Permission::FullAuto), &Control::default())
        .reply
        .unwrap();
    assert_eq!(reply, "bad hook: deny");
}

#[test]
fn a_tool_that_ran_with_no_hook_stops_the_run() {
    let home = home("");
    let claude = claude(&home, &["nohook"], Duration::from_millis(300));
    let error = claude
        .run(&job(&home, Permission::FullAuto), &Control::default())
        .reply
        .unwrap_err();
    assert_eq!(
        error,
        "A tool ran with no check by the bridge, so the run stopped."
    );
}

/// A live run against the real `claude` on `PATH`, with a read-only prompt in a temp
/// folder: `cargo test --test claude_gate live -- --ignored --nocapture`.
#[test]
#[ignore = "needs claude and a Claude login"]
fn live_the_hook_of_claude_fires_for_a_read() {
    let home = home("");
    std::fs::write(home.chat.join("hello.txt"), "the word is banana\n").unwrap();
    std::fs::write(home.chat.join(".env"), "MODE=debug\n").unwrap();
    let mut claude = claude(&home, &[], Duration::from_secs(2));
    claude.command = vec![PROGRAM.into()];
    claude.timeout = Duration::from_mins(3);
    claude.projects = PathBuf::from(std::env::var("HOME").unwrap()).join(".claude/projects");
    let mut job = job(&home, Permission::AutoEdit);
    job.text = "Use the Read tool on hello.txt, then on .env. Reply with the word from \
                hello.txt, then the MODE value from .env, or NONE if you could not read .env."
        .into();
    let (to, events) = std::sync::mpsc::channel();
    let control = Control {
        stop: StopSignal::default(),
        events: Events::to_bridge(to, &job),
    };
    let run = claude.run(&job, &control);
    drop(control);
    let steps: Vec<String> = events
        .try_iter()
        .map(|(_, _, e)| match e {
            Event::Progress(line) => line,
            Event::Question(q) => String::from_utf8_lossy(&q.text).into_owned(),
        })
        .collect();
    println!("{steps:#?}\n{:?}", run.reply);
    let reply = run.reply.unwrap().to_lowercase();
    assert!(reply.contains("banana"), "the read inside the folder ran");
    assert!(!reply.contains("debug"), "the read of .env did not run");
    assert!(
        steps
            .iter()
            .any(|s| s.starts_with("Approve on your desktop")),
        "the read of .env asked the desktop"
    );
}

/// `claude` on `PATH`. A wrapper that logs both streams also works here.
const PROGRAM: &str = "claude";
