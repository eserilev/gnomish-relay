//! The native Claude Code backend against a scripted fake `claude`
//! (`src/bin/fake-claude.rs`).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use bridge::agent::{Agent, Control, Event, Events, Question, StopSignal};
use bridge::claude::ClaudeAgent;
use bridge::config::Permission;
use bridge::desktop::{Approvals, Notice};
use bridge::gate::Gate;
use bridge::relay::{ChatId, Job, MessageId, Session, Work};

const OLD: &str = "0b6ad9d2-1f2e-4c55-9a7e-2b1f4e6c8d01";

fn agent(script: &str, projects: &Path) -> ClaudeAgent {
    ClaudeAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-claude").into(), script.into()],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(20),
        permission_timeout: Duration::from_secs(20),
        projects: projects.to_owned(),
        gate: gate(),
    }
}

/// Every tempdir of the tests is inside the temp folder, so it is the one root.
fn gate() -> Gate {
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    Gate {
        roots: vec![tmp.clone()],
        config_dir: tmp.join("gnomish-relay-test-config"),
        allow: std::sync::Arc::default(),
        approvals: Approvals::new(&tmp.join("gnomish-relay-test-data"), Notice::Off),
    }
}

fn job(dir: &tempfile::TempDir, permission: Permission, text: &str) -> Job {
    Job {
        token: "tok".into(),
        chat: ChatId("c1".into()),
        id: MessageId(1),
        agent: "claude".into(),
        permission,
        cwd: dir.path().to_string_lossy().into_owned(),
        session: Session::New,
        resume: None,
        text: text.into(),
        work: Work::Prompt,
    }
}

fn run(script: &str, permission: Permission) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    agent(script, dir.path())
        .run(&job(&dir, permission, "hello"), &Control::default())
        .reply
}

/// A session file of Claude Code with one exchange.
fn saved_session(projects: &Path, id: &str) {
    let folder = projects.join("-w-app");
    fs::create_dir_all(&folder).unwrap();
    let lines = [
        r#"{"type":"user","uuid":"u1","parentUuid":null,"cwd":"/w/app","sessionId":"x","message":{"role":"user","content":"fix the bugs"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"x","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"All fixed."}]}}"#,
    ];
    fs::write(folder.join(format!("{id}.jsonl")), lines.join("\n") + "\n").unwrap();
}

#[test]
fn the_reply_comes_back_and_the_agent_sees_only_allowed_variables() {
    let reply = run("reply", Permission::AutoEdit).unwrap();
    assert_eq!(
        reply,
        "you said: hello [mode=acceptEdits resume=none tool=stdio secret=hidden job=1]"
    );
}

#[test]
fn a_variable_in_the_env_list_reaches_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = agent("reply", dir.path());
    claude.env = vec!["CARGO_MANIFEST_DIR".into()];
    let reply = claude
        .run(&job(&dir, Permission::Ask, "hi"), &Control::default())
        .reply
        .unwrap();
    assert!(reply.contains("secret=leaked"), "{reply}");
}

#[test]
fn each_level_gets_its_permission_mode() {
    let modes = |level| run("reply", level).unwrap();
    assert!(modes(Permission::Ask).contains("mode=plan"));
    assert!(modes(Permission::AutoEdit).contains("mode=acceptEdits"));
    assert!(modes(Permission::FullAuto).contains("mode=acceptEdits"));
}

#[test]
fn the_modes_table_of_the_config_replaces_the_default_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = agent("reply", dir.path());
    claude.modes.insert(Permission::Ask, "manual".into());
    let reply = claude
        .run(&job(&dir, Permission::Ask, "hi"), &Control::default())
        .reply
        .unwrap();
    assert!(reply.contains("mode=manual"), "{reply}");
}

#[test]
fn a_new_session_comes_back_so_the_next_message_can_resume_it() {
    let dir = tempfile::tempdir().unwrap();
    let run =
        agent("reply", dir.path()).run(&job(&dir, Permission::Ask, "hi"), &Control::default());
    assert_eq!(run.session.as_deref(), Some("s1"));
}

#[test]
fn a_saved_session_resumes_with_its_id() {
    let dir = tempfile::tempdir().unwrap();
    saved_session(dir.path(), OLD);
    let mut job = job(&dir, Permission::Ask, "again");
    job.resume = Some(OLD.into());
    let run = agent("reply", dir.path()).run(&job, &Control::default());
    assert!(run.reply.unwrap().contains(&format!("resume={OLD}")));
    assert_eq!(run.session.as_deref(), Some(OLD));
}

#[test]
fn a_session_with_no_file_starts_again_and_the_reply_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut job = job(&dir, Permission::Ask, "again");
    job.resume = Some(OLD.into());
    let run = agent("reply", dir.path()).run(&job, &Control::default());
    let reply = run.reply.unwrap();
    assert!(reply.starts_with("(New session:"), "{reply}");
    assert!(reply.contains("resume=none"), "{reply}");
    assert_eq!(run.session.as_deref(), Some("s1"));
}

#[test]
fn below_full_auto_with_no_game_a_tool_call_is_denied_and_named_in_the_reply() {
    let reply = run("permission", Permission::AutoEdit).unwrap();
    assert!(
        reply.starts_with("deny same=false rules=false Not allowed from the game."),
        "{reply}"
    );
    assert!(
        reply.ends_with("Not allowed from the game: Bash: clean the build"),
        "{reply}"
    );
}

#[test]
fn at_full_auto_a_tool_call_is_allowed_with_its_input_and_no_rules() {
    let reply = run("permission", Permission::FullAuto).unwrap();
    assert_eq!(reply, "allow same=true rules=false ");
}

#[test]
fn a_control_request_that_the_bridge_does_not_serve_gets_an_error() {
    let reply = run("hook", Permission::Ask).unwrap();
    assert_eq!(reply, "hook answer error");
}

#[test]
fn a_hung_agent_times_out_and_the_run_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = agent("hang", dir.path());
    claude.timeout = Duration::from_millis(500);
    let start = Instant::now();
    let reply = claude
        .run(&job(&dir, Permission::Ask, "hi"), &Control::default())
        .reply;
    assert_eq!(reply.unwrap_err(), "Timed out.");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_crash_reports_the_last_line_of_stderr() {
    let error = run("crash", Permission::Ask).unwrap_err();
    assert_eq!(error, "The agent stopped: boom: not logged in");
}

#[test]
fn a_line_that_is_not_json_ends_the_run() {
    let error = run("garbage", Permission::Ask).unwrap_err();
    assert!(error.contains("not JSON"), "{error}");
}

#[test]
fn a_message_over_the_size_limit_ends_the_run() {
    let error = run("huge", Permission::Ask).unwrap_err();
    assert!(error.contains("size limit"), "{error}");
}

#[test]
fn a_failed_turn_shows_the_text_of_the_agent() {
    let error = run("failed", Permission::Ask).unwrap_err();
    assert_eq!(
        error,
        "The agent stopped: Invalid API key · Please run /login"
    );
}

#[test]
fn a_failed_initialize_ends_the_run() {
    let error = run("badinit", Permission::Ask).unwrap_err();
    assert_eq!(error, "The agent failed at init1: no hooks here");
}

#[test]
fn a_missing_program_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = agent("reply", dir.path());
    claude.command = vec!["no-such-claude-gnomish".into()];
    let error = claude
        .run(&job(&dir, Permission::Ask, "hi"), &Control::default())
        .reply
        .unwrap_err();
    assert!(
        error.starts_with("Cannot start no-such-claude-gnomish"),
        "{error}"
    );
}

#[test]
fn stop_interrupts_the_turn_and_keeps_the_session() {
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
    let run = agent("slow", dir.path()).run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
    assert_eq!(run.session.as_deref(), Some("s1"));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn stop_before_the_prompt_ends_the_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let control = Control::default();
    control.stop.request();
    let start = Instant::now();
    let run = agent("reply", dir.path()).run(&job(&dir, Permission::Ask, "hi"), &control);
    assert_eq!(run.reply.unwrap_err(), "Stopped.");
    assert!(start.elapsed() < Duration::from_secs(5));
}

/// Runs `script` with a listener, as the bridge does. `answer` answers each question.
fn run_with_game(
    claude: &ClaudeAgent,
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
    let reply = claude.run(&job, &control).reply;
    drop(control);
    (reply, seen.join().unwrap())
}

#[test]
fn each_tool_call_becomes_a_progress_line_and_the_text_is_the_reply() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, events) =
        run_with_game(&agent("steps", dir.path()), Permission::AutoEdit, |_| None);
    assert_eq!(reply.unwrap(), "Let me look. ");
    let lines: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            Event::Progress(line) => Some(line),
            Event::Question(_) => None,
        })
        .collect();
    assert_eq!(lines, ["$ cargo test", "Edit src/main.rs"]);
}

#[test]
fn a_tool_call_goes_to_the_game_with_the_honest_text_and_no_always() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, events) = run_with_game(
        &agent("permission", dir.path()),
        Permission::AutoEdit,
        |q| {
            assert_eq!(
                q.text,
                b"rm -rf build\nthe agent says: Bash: clean the build"
            );
            let labels: Vec<&str> = q.choices.iter().map(|c| c.label.as_str()).collect();
            assert_eq!(labels, ["Allow", "Deny"]);
            Some(Some(0))
        },
    );
    assert_eq!(reply.unwrap(), "allow same=true rules=false ");
    assert_eq!(events.len(), 1);
}

#[test]
fn a_deny_in_the_game_reaches_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let (reply, _) = run_with_game(
        &agent("permission", dir.path()),
        Permission::AutoEdit,
        |_| Some(Some(1)),
    );
    assert_eq!(
        reply.unwrap(),
        "deny same=false rules=false Denied in the game."
    );
}

#[test]
fn an_unanswered_question_is_denied_after_the_permission_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let mut claude = agent("permission", dir.path());
    claude.permission_timeout = Duration::from_millis(300);
    let start = Instant::now();
    let (reply, _) = run_with_game(&claude, Permission::AutoEdit, |_| None);
    assert_eq!(
        reply.unwrap(),
        "deny same=false rules=false No answer from the game.\n\nNot allowed from the game: Bash: clean the build"
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

fn attach(projects: &Path, id: &str, fork: bool) -> (Result<String, String>, Option<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut job = job(&dir, Permission::Ask, "");
    job.work = Work::Attach {
        session: id.into(),
        fork,
    };
    let run = agent("hang", projects).run(&job, &Control::default());
    (run.reply, run.session)
}

#[test]
fn an_attach_reads_the_last_exchange_with_no_process() {
    let projects = tempfile::tempdir().unwrap();
    saved_session(projects.path(), OLD);
    let (reply, session) = attach(projects.path(), OLD, false);
    assert_eq!(reply.unwrap(), "fix the bugs\nAll fixed.");
    assert_eq!(session.as_deref(), Some(OLD));
}

#[test]
fn an_attach_with_fork_continues_a_new_copy_next_to_the_old_file() {
    let projects = tempfile::tempdir().unwrap();
    saved_session(projects.path(), OLD);
    let (reply, session) = attach(projects.path(), OLD, true);
    let copy = session.unwrap();
    assert_ne!(copy, OLD);
    assert_eq!(reply.unwrap(), "fix the bugs\nAll fixed.");
    let copied =
        fs::read_to_string(projects.path().join("-w-app").join(format!("{copy}.jsonl"))).unwrap();
    assert!(
        copied.contains(&format!("\"sessionId\":\"{copy}\"")),
        "{copied}"
    );
}

#[test]
fn an_attach_to_a_missing_session_is_an_error() {
    let projects = tempfile::tempdir().unwrap();
    let (reply, session) = attach(projects.path(), OLD, false);
    assert_eq!(reply.unwrap_err(), "The session is gone.");
    assert_eq!(session, None);
}

#[test]
fn the_list_reads_the_session_files() {
    let projects = tempfile::tempdir().unwrap();
    saved_session(projects.path(), OLD);
    let sessions = agent("hang", projects.path()).sessions("/w").unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        (sessions[0].id.as_str(), sessions[0].cwd.as_str()),
        (OLD, "/w/app")
    );
    assert_eq!(sessions[0].title, "fix the bugs");
}

#[test]
fn check_shows_the_version_and_needs_a_login() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_string_lossy();
    let report = agent("reply", dir.path()).check(&cwd).unwrap();
    assert_eq!(report.version, "9.9.9");
    assert!(report.modes.contains(&"plan".to_owned()));
    let error = agent("nologin", dir.path()).check(&cwd).unwrap_err();
    assert!(bridge::install::needs_login(&error), "{error}");
}

/// A live run against the real `claude` on `PATH`, with a read-only prompt in an empty
/// folder: `cargo test --test claude live -- --ignored --nocapture`.
#[test]
#[ignore = "needs claude and a Claude login"]
fn live_claude_answers_lists_forks_and_resumes() {
    let home = std::env::var("HOME").unwrap();
    let projects = Path::new(&home).join(".claude/projects");
    let mut claude = agent("", &projects);
    claude.command = vec!["claude".into()];
    let report = claude.check(&home).unwrap();
    println!("claude {}", report.version);
    let dir = tempfile::tempdir().unwrap();
    let run = claude.run(
        &job(&dir, Permission::Ask, "Reply with the single word: pong"),
        &Control::default(),
    );
    println!("{:?} {:?}", run.session, run.reply);
    assert!(run.reply.unwrap().to_lowercase().contains("pong"));
    let session = run.session.unwrap();
    let sessions = claude.sessions(&home).unwrap();
    assert!(
        sessions.iter().any(|s| s.id == session),
        "the new session is in the list"
    );
    let mut attach = job(&dir, Permission::Ask, "");
    attach.work = Work::Attach {
        session,
        fork: true,
    };
    let forked = claude.run(&attach, &Control::default());
    println!("{:?} {:?}", forked.session, forked.reply);
    let mut next = job(
        &dir,
        Permission::Ask,
        "Which single word did you reply before? Reply with only that word.",
    );
    next.resume = forked.session.clone();
    let run = claude.run(&next, &Control::default());
    println!("{:?} {:?}", run.session, run.reply);
    assert!(
        run.reply.unwrap().to_lowercase().contains("pong"),
        "the copy has the history"
    );
    assert_eq!(run.session, forked.session, "the copy resumes");
}
