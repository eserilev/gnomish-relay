//! The ACP backend against a scripted fake agent (`src/bin/fake-acp-agent.rs`).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bridge::acp::{AcpAgent, unix_time};
use bridge::agent::{Agent, Control, Event, Events, StopSignal};
use bridge::config::Permission;
use bridge::relay::{ChatId, Job, MessageId, Session, Work};

fn agent(script: &str) -> AcpAgent {
    AcpAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-acp-agent").into(), script.into()],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(20),
        permission_timeout: Duration::from_secs(20),
    }
}

fn job(dir: &tempfile::TempDir, permission: Permission, text: &str) -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId("c1".into()),
        id: MessageId(1),
        agent: "fake".into(),
        permission,
        cwd: dir.path().to_string_lossy().into_owned(),
        session: Session::New,
        resume: None,
        text: text.into(),
        work: Work::Prompt,
    }
}

fn run(agent: &AcpAgent, permission: Permission, text: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    agent
        .run(&job(&dir, permission, text), &Control::default())
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
fn the_reply_streams_back_and_the_agent_sees_only_allowed_variables() {
    let reply = run(&agent("reply"), Permission::AutoEdit, "hello").unwrap();
    assert_eq!(reply, "you said: hello [mode=none secret=hidden job=1]");
}

#[test]
fn a_variable_in_the_env_list_reaches_the_agent() {
    let mut agent = agent("reply");
    agent.env = vec!["CARGO_MANIFEST_DIR".into()];
    let reply = run(&agent, Permission::AutoEdit, "hi").unwrap();
    assert!(reply.contains("secret=leaked"), "{reply}");
}

#[test]
fn the_mode_of_the_level_is_set_before_the_prompt() {
    let mut agent = agent("reply");
    agent.modes.insert(Permission::Ask, "plan".into());
    let reply = run(&agent, Permission::Ask, "hi").unwrap();
    assert!(reply.contains("mode=plan"), "{reply}");
}

#[test]
fn a_mode_that_the_agent_does_not_offer_stops_the_run() {
    let mut agent = agent("reply");
    agent.modes.insert(Permission::Ask, "yolo".into());
    let error = run(&agent, Permission::Ask, "hi").unwrap_err();
    assert!(error.contains("no mode yolo"), "{error}");
}

#[test]
fn below_full_auto_a_permission_request_is_refused_and_named_in_the_reply() {
    let reply = run(&agent("permission"), Permission::AutoEdit, "clean").unwrap();
    assert!(reply.starts_with("chose no"), "{reply}");
    assert!(
        reply.ends_with("Not allowed from the game: clean the build"),
        "{reply}"
    );
}

#[test]
fn at_full_auto_a_permission_request_is_allowed_once() {
    let reply = run(&agent("permission"), Permission::FullAuto, "clean").unwrap();
    assert_eq!(reply, "chose yes");
}

#[test]
fn a_file_request_from_the_agent_gets_method_not_found() {
    let reply = run(&agent("files"), Permission::FullAuto, "read").unwrap();
    assert_eq!(reply, "fs answer -32601");
}

#[test]
fn a_hung_agent_times_out_and_the_run_ends() {
    let mut agent = agent("hang");
    agent.timeout = Duration::from_millis(500);
    let start = Instant::now();
    assert_eq!(
        run(&agent, Permission::Ask, "hi").unwrap_err(),
        "Timed out."
    );
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_crash_reports_the_last_line_of_stderr() {
    let error = run(&agent("crash"), Permission::Ask, "hi").unwrap_err();
    assert_eq!(error, "The agent stopped: boom: not logged in");
}

#[test]
fn a_line_that_is_not_json_ends_the_run() {
    let error = run(&agent("garbage"), Permission::Ask, "hi").unwrap_err();
    assert!(error.contains("not JSON"), "{error}");
}

#[test]
fn a_message_over_the_size_limit_ends_the_run() {
    let error = run(&agent("huge"), Permission::Ask, "hi").unwrap_err();
    assert!(error.contains("size limit"), "{error}");
}

#[test]
fn another_protocol_version_is_refused() {
    let error = run(&agent("v2"), Permission::Ask, "hi").unwrap_err();
    assert!(error.contains("ACP version 2"), "{error}");
}

#[test]
fn a_missing_program_is_an_error_not_a_panic() {
    let mut agent = agent("reply");
    agent.command = vec!["no-such-agent-gnomish".into()];
    let error = run(&agent, Permission::Ask, "hi").unwrap_err();
    assert!(
        error.starts_with("Cannot start no-such-agent-gnomish"),
        "{error}"
    );
}

#[test]
fn check_shows_the_agent_and_its_modes() {
    let dir = tempfile::tempdir().unwrap();
    let report = agent("reply").check(&dir.path().to_string_lossy()).unwrap();
    assert_eq!(
        (report.name.as_str(), report.version.as_str()),
        ("fake", "1.0")
    );
    assert_eq!(report.modes, ["default", "plan"]);
    assert!(!report.load_session);
}

#[test]
fn a_new_session_comes_back_so_the_next_message_can_resume_it() {
    let dir = tempfile::tempdir().unwrap();
    let run = agent("reply").run(&job(&dir, Permission::Ask, "hi"), &Control::default());
    assert_eq!(run.session.as_deref(), Some("s1"));
}

#[test]
fn an_agent_with_session_resume_continues_the_old_session() {
    let (reply, session) = resume("resume");
    assert_eq!(reply.unwrap(), "in old-7, resumed old-7 by session/resume");
    assert_eq!(session.as_deref(), Some("old-7"));
}

#[test]
fn a_loaded_session_replays_history_that_stays_out_of_the_reply() {
    let (reply, _) = resume("load");
    assert_eq!(reply.unwrap(), "in old-7, resumed old-7 by session/load");
}

#[test]
fn an_agent_that_cannot_resume_gets_a_new_session_and_the_reply_says_so() {
    let (reply, session) = resume("noresume");
    let reply = reply.unwrap();
    assert!(reply.starts_with("(New session:"), "{reply}");
    assert!(reply.ends_with("in s1, resumed no"), "{reply}");
    assert_eq!(session.as_deref(), Some("s1"));
}

#[test]
fn stop_cancels_the_turn_and_keeps_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let stop = StopSignal::default();
    let later = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        later.request();
    });
    let start = Instant::now();
    let control = Control {
        stop,
        ..Control::default()
    };
    let run = agent("slow").run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
    assert_eq!(run.session.as_deref(), Some("s1"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn stop_before_the_session_opens_ends_the_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let control = Control::default();
    control.stop.request();
    let run = agent("reply").run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
}

/// Runs `script` with a listener, as the bridge does. `answer` answers each question.
fn run_with_game(
    agent: &AcpAgent,
    permission: Permission,
    answer: impl Fn(&bridge::agent::Question) -> Option<Option<usize>> + Send + 'static,
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
    let reply = agent.run(&job, &control).reply;
    drop(control);
    (reply, seen.join().unwrap())
}

#[test]
fn each_tool_call_becomes_a_progress_line() {
    let (reply, events) = run_with_game(&agent("steps"), Permission::AutoEdit, |_| None);
    assert_eq!(reply.unwrap(), "done");
    let lines: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            Event::Progress(line) => Some(line),
            Event::Question(_) => None,
        })
        .collect();
    assert_eq!(lines, ["edit src/main.rs", "$ cargo test"]);
}

#[test]
fn a_permission_request_goes_to_the_game_with_the_honest_text() {
    let (reply, events) = run_with_game(&agent("permission"), Permission::AutoEdit, |q| {
        assert_eq!(q.text, b"rm -rf build\nthe agent says: clean the build");
        let labels: Vec<&str> = q.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Allow", "Reject"],
            "allow always waits for SPEC 6.6.5"
        );
        Some(Some(0))
    });
    assert_eq!(reply.unwrap(), "chose yes");
    assert_eq!(events.len(), 1);
}

#[test]
fn the_answer_of_the_game_picks_the_option() {
    let (reply, _) = run_with_game(&agent("permission"), Permission::AutoEdit, |_| {
        Some(Some(1))
    });
    assert_eq!(reply.unwrap(), "chose no");
}

#[test]
fn an_unanswered_request_is_cancelled_after_the_permission_timeout() {
    let mut agent = agent("permission");
    agent.permission_timeout = Duration::from_millis(300);
    agent.timeout = Duration::from_secs(1);
    let start = Instant::now();
    let (reply, _) = run_with_game(&agent, Permission::AutoEdit, |_| None);
    assert_eq!(reply.unwrap(), "chose cancelled");
    assert!(start.elapsed() < Duration::from_secs(5));
}

fn attach(script: &str, fork: bool) -> (Result<String, String>, Option<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut job = job(&dir, Permission::Ask, "");
    job.work = Work::Attach {
        session: "a1".into(),
        fork,
    };
    let run = agent(script).run(&job, &Control::default());
    (run.reply, run.session)
}

#[test]
fn the_list_has_each_session_with_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = agent("sessions")
        .sessions(&dir.path().to_string_lossy())
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "a1");
    assert_eq!(sessions[0].cwd, "/w/app");
    assert_eq!(sessions[0].title, "Fix bugs");
    assert_eq!(sessions[0].updated, 1_790_318_781);
}

#[test]
fn an_agent_with_no_list_has_no_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = agent("reply")
        .sessions(&dir.path().to_string_lossy())
        .unwrap();
    assert!(sessions.is_empty());
}

#[test]
fn an_attach_returns_the_last_exchange_of_the_session() {
    let (reply, session) = attach("sessions", false);
    assert_eq!(reply.unwrap(), "fix the bugs\nAll fixed.");
    assert_eq!(session.as_deref(), Some("a1"));
}

#[test]
fn an_attach_with_fork_continues_a_copy() {
    let (reply, session) = attach("sessions", true);
    assert_eq!(reply.unwrap(), "fix the bugs\nAll fixed.");
    assert_eq!(session.as_deref(), Some("fork-of-a1"));
}

#[test]
fn an_attach_to_an_agent_with_no_load_keeps_the_session_and_shows_nothing() {
    let (reply, session) = attach("reply", true);
    assert_eq!(reply.unwrap(), "");
    assert_eq!(
        session.as_deref(),
        Some("a1"),
        "no fork, so the same session"
    );
}

#[test]
fn an_iso_time_becomes_unix_seconds_and_a_bad_one_is_none() {
    assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(unix_time("2024-02-29T23:59:59.999Z"), Some(1_709_251_199));
    assert_eq!(unix_time("2026-13-01T00:00:00Z"), None);
    assert_eq!(unix_time("yesterday"), None);
    assert_eq!(unix_time("1969-12-31T23:59:59Z"), None);
}

/// A live run against `claude-agent-acp` on `PATH`. `GNOMISH_LIVE_SESSION` names a
/// session to replay: `cargo test --test acp live -- --ignored --nocapture`.
#[test]
#[ignore = "needs claude-agent-acp and a Claude login"]
fn live_claude_lists_and_replays_sessions() {
    let mut claude = agent("");
    claude.command = vec!["claude-agent-acp".into()];
    let home = std::env::var("HOME").unwrap();
    let sessions = claude.sessions(&home).unwrap();
    for s in sessions.iter().take(5) {
        println!("{} {} {} {}", s.id, s.updated, s.cwd, s.title);
    }
    assert!(!sessions.is_empty());
    let Ok(id) = std::env::var("GNOMISH_LIVE_SESSION") else {
        return;
    };
    let session = sessions.iter().find(|s| s.id == id).unwrap();
    let mut job = job(&tempfile::tempdir().unwrap(), Permission::Ask, "");
    job.cwd.clone_from(&session.cwd);
    job.work = Work::Attach {
        session: id,
        fork: false,
    };
    let run = claude.run(&job, &Control::default());
    println!("{:?}\n{}", run.session, run.reply.unwrap());
}
