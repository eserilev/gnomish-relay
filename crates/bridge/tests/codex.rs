//! The native Codex backend against a scripted fake `codex app-server`
//! (`src/bin/fake-codex.rs`).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use bridge::agent::{Agent, Control, Event, Events, Question, StopSignal};
use bridge::codex::CodexAgent;
use bridge::config::Permission;
use bridge::relay::{ChatId, Job, MessageId, Session, Work};

fn agent(script: &str) -> CodexAgent {
    CodexAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-codex").into(), script.into()],
        env: Vec::new(),
        timeout: Duration::from_secs(20),
        permission_timeout: Duration::from_secs(20),
    }
}

fn job(dir: &tempfile::TempDir, permission: Permission, text: &str) -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId("c1".into()),
        id: MessageId(1),
        agent: "codex".into(),
        permission,
        cwd: dir.path().to_string_lossy().into_owned(),
        session: Session::New,
        resume: None,
        text: text.into(),
        work: Work::Prompt,
    }
}

fn run(codex: &CodexAgent, permission: Permission) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    codex
        .run(&job(&dir, permission, "hello"), &Control::default())
        .reply
}

fn resume(script: &str) -> (Result<String, String>, Option<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut job = job(&dir, Permission::Ask, "again");
    job.resume = Some("old-7".into());
    let run = agent(script).run(&job, &Control::default());
    (run.reply, run.session)
}

#[test]
fn the_last_agent_message_is_the_reply_and_the_agent_sees_only_allowed_variables() {
    let reply = run(&agent("reply"), Permission::AutoEdit).unwrap();
    assert_eq!(
        reply,
        "you said: hello [sandbox=workspace-write approval=on-request secret=hidden job=1]"
    );
}

#[test]
fn a_variable_in_the_env_list_reaches_the_agent() {
    let mut codex = agent("reply");
    codex.env = vec!["CARGO_MANIFEST_DIR".into()];
    let reply = run(&codex, Permission::AutoEdit).unwrap();
    assert!(reply.contains("secret=leaked"), "{reply}");
}

#[test]
fn ask_runs_in_the_read_only_sandbox_and_asks_for_every_untrusted_command() {
    let reply = run(&agent("reply"), Permission::Ask).unwrap();
    assert!(
        reply.contains("sandbox=read-only approval=untrusted"),
        "{reply}"
    );
    let full = run(&agent("reply"), Permission::FullAuto).unwrap();
    assert!(
        full.contains("sandbox=workspace-write approval=on-request"),
        "{full}"
    );
}

#[test]
fn a_new_thread_comes_back_so_the_next_message_can_resume_it() {
    let dir = tempfile::tempdir().unwrap();
    let run = agent("reply").run(&job(&dir, Permission::Ask, "hi"), &Control::default());
    assert_eq!(run.session.as_deref(), Some("t1"));
}

#[test]
fn a_saved_thread_resumes() {
    let (reply, session) = resume("reply");
    assert!(reply.unwrap().contains("[resumed old-7 "));
    assert_eq!(session.as_deref(), Some("old-7"));
}

#[test]
fn a_thread_that_cannot_resume_starts_again_and_the_reply_says_so() {
    let (reply, session) = resume("noresume");
    let reply = reply.unwrap();
    assert!(reply.starts_with("(New session:"), "{reply}");
    assert_eq!(session.as_deref(), Some("t1"));
}

#[test]
fn below_full_auto_with_no_game_each_approval_is_declined_and_named() {
    let reply = run(&agent("approval"), Permission::AutoEdit).unwrap();
    assert_eq!(
        reply,
        "command decline, change decline\n\nNot allowed from the game: clean the build; change files"
    );
}

#[test]
fn at_full_auto_each_approval_is_accepted_once() {
    let reply = run(&agent("approval"), Permission::FullAuto).unwrap();
    assert_eq!(reply, "command accept, change accept");
}

#[test]
fn a_server_request_that_the_bridge_does_not_serve_gets_an_error() {
    let reply = run(&agent("other"), Permission::Ask).unwrap();
    assert_eq!(reply, "input answer error");
}

#[test]
fn a_failed_turn_shows_the_error_of_codex() {
    let error = run(&agent("failed"), Permission::Ask).unwrap_err();
    assert_eq!(error, "The agent stopped: You've hit your usage limit.");
}

#[test]
fn a_hung_agent_times_out_and_the_run_ends() {
    let mut codex = agent("hang");
    codex.timeout = Duration::from_millis(500);
    let start = Instant::now();
    assert_eq!(run(&codex, Permission::Ask).unwrap_err(), "Timed out.");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_crash_reports_the_last_line_of_stderr() {
    let error = run(&agent("crash"), Permission::Ask).unwrap_err();
    assert_eq!(error, "The agent stopped: boom: stream disconnected");
}

#[test]
fn a_line_that_is_not_json_ends_the_run() {
    let error = run(&agent("garbage"), Permission::Ask).unwrap_err();
    assert!(error.contains("not JSON"), "{error}");
}

#[test]
fn stop_interrupts_the_turn_and_keeps_the_thread() {
    let dir = tempfile::tempdir().unwrap();
    let stop = StopSignal::default();
    let later = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        later.request();
    });
    let control = Control {
        stop,
        ..Control::default()
    };
    let start = Instant::now();
    let run = agent("slow").run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
    assert_eq!(run.session.as_deref(), Some("t1"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn stop_before_the_turn_ends_the_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let control = Control::default();
    control.stop.request();
    let run = agent("reply").run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
}

/// Runs `script` with a listener, as the bridge does. `answer` answers each question.
fn run_with_game(
    codex: &CodexAgent,
    permission: Permission,
    answer: impl Fn(&Question) -> Option<Option<usize>> + Send + 'static,
) -> (Result<String, String>, Vec<Event>) {
    let dir = tempfile::tempdir().unwrap();
    let job = job(&dir, permission, "go");
    let (to, events) = std::sync::mpsc::channel();
    let control = Control {
        stop: StopSignal::default(),
        events: Events::to_bridge(to, &job),
    };
    let seen = std::thread::spawn(move || {
        let mut seen = Vec::new();
        while let Ok((_, _, event)) = events.recv() {
            if let Event::Question(question) = &event
                && let Some(choice) = answer(question)
            {
                question.answer.send(choice).unwrap();
            }
            seen.push(event);
        }
        seen
    });
    let reply = codex.run(&job, &control).reply;
    drop(control);
    (reply, seen.join().unwrap())
}

#[test]
fn each_command_and_change_becomes_a_progress_line() {
    let (reply, events) = run_with_game(&agent("steps"), Permission::AutoEdit, |_| None);
    assert_eq!(reply.unwrap(), "done");
    let lines: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            Event::Progress(line) => Some(line),
            Event::Question(_) => None,
        })
        .collect();
    assert_eq!(lines, ["$ cargo test", "edit src/main.rs"]);
}

#[test]
fn an_approval_goes_to_the_game_with_the_honest_text_and_no_always() {
    let (reply, events) = run_with_game(&agent("approval"), Permission::AutoEdit, |q| {
        let labels: Vec<&str> = q.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Allow", "Deny"]);
        Some(Some(usize::from(!q.text.starts_with(b"rm"))))
    });
    assert_eq!(reply.unwrap(), "command accept, change decline");
    let texts: Vec<Vec<u8>> = events
        .into_iter()
        .filter_map(|e| match e {
            Event::Question(q) => Some(q.text),
            Event::Progress(_) => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            b"rm -rf build\nthe agent says: clean the build".to_vec(),
            b"src/a.rs\nthe agent says: change files".to_vec()
        ]
    );
}

#[test]
fn an_unanswered_approval_is_declined_after_the_permission_timeout() {
    let mut codex = agent("approval");
    codex.permission_timeout = Duration::from_millis(200);
    let (reply, _) = run_with_game(&codex, Permission::AutoEdit, |_| None);
    assert_eq!(reply.unwrap(), "command decline, change decline");
}

fn attach(fork: bool) -> (Result<String, String>, Option<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut job = job(&dir, Permission::Ask, "");
    job.work = Work::Attach {
        session: "a1".into(),
        fork,
    };
    let run = agent("reply").run(&job, &Control::default());
    (run.reply, run.session)
}

#[test]
fn an_attach_returns_the_last_exchange_of_the_thread() {
    let (reply, session) = attach(false);
    assert_eq!(reply.unwrap(), "fix the bugs\nLooking.\n\nAll fixed.");
    assert_eq!(session.as_deref(), Some("a1"));
}

#[test]
fn an_attach_with_fork_continues_a_copy() {
    let (reply, session) = attach(true);
    assert_eq!(reply.unwrap(), "fix the bugs\nLooking.\n\nAll fixed.");
    assert_eq!(session.as_deref(), Some("fork-of-a1"));
}

#[test]
fn the_list_has_each_thread_with_a_folder_and_its_name_or_preview() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = agent("reply")
        .sessions(&dir.path().to_string_lossy())
        .unwrap();
    let rows: Vec<(&str, &str, &str, u32)> = sessions
        .iter()
        .map(|s| (s.id.as_str(), s.cwd.as_str(), s.title.as_str(), s.updated))
        .collect();
    assert_eq!(
        rows,
        [
            ("a1", "/w/app", "Fix bugs", 1_790_318_781),
            ("b2", "/w/lib", "add a test", 5)
        ]
    );
}

#[test]
fn check_shows_the_version_and_needs_a_login() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_string_lossy();
    let report = agent("reply").check(&cwd).unwrap();
    assert_eq!(report.version, "9.9.9");
    let error = agent("nologin").check(&cwd).unwrap_err();
    assert!(bridge::install::needs_login(&error), "{error}");
}

/// A live run against the real `codex` on `PATH`, with a read-only prompt in an empty
/// folder: `cargo test --test codex live -- --ignored --nocapture`.
#[test]
#[ignore = "needs codex and a Codex login"]
fn live_codex_answers_lists_forks_and_resumes() {
    let home = std::env::var("HOME").unwrap();
    let mut codex = agent("");
    codex.command = vec!["codex".into()];
    let report = codex.check(&home).unwrap();
    println!("codex {}", report.version);
    let dir = tempfile::tempdir().unwrap();
    let run = codex.run(
        &job(&dir, Permission::Ask, "Reply with the single word: pong"),
        &Control::default(),
    );
    println!("{:?} {:?}", run.session, run.reply);
    assert!(run.reply.unwrap().to_lowercase().contains("pong"));
    let thread = run.session.unwrap();
    let sessions = codex.sessions(&home).unwrap();
    assert!(
        sessions.iter().any(|s| s.id == thread),
        "the new thread is in the list"
    );
    let mut attach = job(&dir, Permission::Ask, "");
    attach.work = Work::Attach {
        session: thread,
        fork: true,
    };
    let forked = codex.run(&attach, &Control::default());
    println!("{:?} {:?}", forked.session, forked.reply);
    let mut next = job(
        &dir,
        Permission::Ask,
        "Which single word did you reply before? Reply with only that word.",
    );
    next.resume = forked.session.clone();
    let run = codex.run(&next, &Control::default());
    println!("{:?} {:?}", run.session, run.reply);
    assert!(
        run.reply.unwrap().to_lowercase().contains("pong"),
        "the copy has the history"
    );
}
