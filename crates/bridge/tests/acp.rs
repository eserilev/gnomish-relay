//! The ACP backend against a scripted fake agent (`src/bin/fake-acp-agent.rs`).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bridge::acp::AcpAgent;
use bridge::agent::Agent;
use bridge::config::Permission;
use bridge::relay::{ChatId, Job, MessageId, Session};

fn agent(script: &str) -> AcpAgent {
    AcpAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-acp-agent").into(), script.into()],
        env: Vec::new(),
        modes: BTreeMap::new(),
        timeout: Duration::from_secs(20),
    }
}

fn run(agent: &AcpAgent, permission: Permission, text: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    let job = Job {
        token: "tok".into(),
        chat: ChatId("c1".into()),
        id: MessageId(1),
        agent: "fake".into(),
        permission,
        cwd: dir.path().to_string_lossy().into_owned(),
        session: Session::New,
        text: text.into(),
    };
    agent.run(&job)
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
        reply.ends_with("Not allowed from the game: rm -rf build"),
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
