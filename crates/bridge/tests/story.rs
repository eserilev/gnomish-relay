//! The story program and its life cycle, with the fake story program (SPEC.md 9.8 and
//! 9.7, decision 16). A message is a batch of the Timeways addon: JSON lines, game events
//! first, and at most one question last. These tests run with no sandbox;
//! `story_sandbox.rs` tests the sandbox.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fake_model;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bridge::lane::{ChatId, MessageId};
use bridge::model::{ModelChoice, ModelSpec};
use bridge::model_local::LocalModel;
use bridge::story::{
    BAD_CHARACTER, NO_ANSWER, NO_SANDBOX, OUT_OF_ORDER, Reply, STOPPED, Story, StorySpec, TOO_LONG,
    UPDATE_BRIDGE, UPDATE_TIMEWAYS,
};
use bridge::story_sandbox::{Sandbox, Walls};
use bridge::timeways::StoryMessage;
use fake_model::Answer;
use serde_json::Value;

const EVENTS: &str = "{\"type\":\"zone_entered\",\"at\":100,\"zone\":\"Elwynn Forest\",\"subzone\":\"Goldshire\"}\n\
                      {\"type\":\"npc_met\",\"at\":101,\"name\":\"Marshal Dughan\"}";

const JOURNAL: &str = "{\"type\":\"journal_asked\",\"page\":3}";
const CHARACTER: &str =
    "{\"type\":\"character_entered\",\"realm\":\"Stormrage\",\"name\":\"Anduin\"}";

fn question(text: &str) -> String {
    format!("{{\"type\":\"lore_asked\",\"at\":102,\"question\":\"{text}\"}}")
}

fn spec(script: &str, dir: &Path, timeout: Duration) -> StorySpec {
    StorySpec {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-story")),
        args: vec![script.into()],
        walls: Walls {
            folder: dir.join("story"),
            hidden: Vec::new(),
            readable: Vec::new(),
        },
        sandbox: Sandbox::None,
        timeout,
        model: ModelSpec::none(),
    }
}

fn story(script: &str, dir: &Path) -> Story {
    Story::new(spec(script, dir, Duration::from_secs(20)))
}

fn message(id: u32, text: &str) -> StoryMessage {
    StoryMessage {
        token: "tok".into(),
        chat: ChatId("story".into()),
        id: MessageId(id),
        name: String::new(),
        text: text.into(),
    }
}

/// Steps until `count` answers came, for at most 30 seconds.
fn answers(story: &mut Story, count: usize) -> Vec<Reply> {
    let start = Instant::now();
    let mut all = Vec::new();
    while all.len() < count && start.elapsed() < Duration::from_secs(30) {
        story.step();
        all.extend(story.take_replies());
        std::thread::sleep(Duration::from_millis(10));
    }
    all
}

fn wait_until_ready(story: &mut Story) {
    let start = Instant::now();
    while !story.is_ready() {
        assert!(start.elapsed() < Duration::from_secs(30), "never ready");
        story.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The reply of a lore answer, as JSON.
fn reply(answer: &Reply) -> Value {
    let text = answer.1.as_ref().unwrap();
    serde_json::from_str(text).unwrap_or_else(|_| panic!("not JSON: {text}"))
}

/// The reply to a batch of game events that the story program saw.
fn assert_events_seen(answer: &Reply) {
    assert_eq!(reply(answer)["type"], "events_seen");
}

fn error(answer: &Reply) -> &str {
    answer.1.as_ref().unwrap_err()
}

fn seen(dir: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join("story/seen.txt")).unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_batch_that_ends_with_a_question_gets_one_lore_answer_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(
        7,
        &format!("{EVENTS}\n{}", question("open the portal")),
    ));

    let answers = answers(&mut story, 1);

    assert_eq!(answers[0].0.id, MessageId(7));
    let reply = reply(&answers[0]);
    assert_eq!(reply["type"], "lore_answer");
    assert_eq!(reply["text"], "story: open the portal");
    assert_eq!(
        reply["passages"][0]["source"],
        "https://example.test/portal"
    );
    assert!(
        reply.get("id").is_none(),
        "the id stays between the bridge and the story"
    );
}

#[test]
fn each_line_of_a_batch_reaches_the_story_program_with_the_id_of_its_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(
        7,
        &format!("{CHARACTER}\n{EVENTS}\n{}", question("why?")),
    ));
    story.send(message(8, &format!("{CHARACTER}\n{EVENTS}")));

    answers(&mut story, 2);

    let seen = seen(dir.path());
    let types: Vec<&str> = seen.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        [
            "character_entered",
            "zone_entered",
            "npc_met",
            "lore_asked",
            "character_entered",
            "zone_entered",
            "npc_met"
        ]
    );
    let ids: Vec<u64> = seen.iter().map(|l| l["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [1, 1, 1, 1, 2, 2, 2]);
    assert_eq!(seen[0]["realm"], "Stormrage");
    assert_eq!(seen[1]["subzone"], "Goldshire");
}

#[test]
fn a_character_line_that_is_not_first_refuses_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, &format!("{EVENTS}\n{CHARACTER}")));
    story.send(message(8, &format!("{CHARACTER}\n{CHARACTER}")));

    let answers = answers(&mut story, 2);

    assert!(error(&answers[0]).starts_with(OUT_OF_ORDER));
    assert!(error(&answers[1]).starts_with(OUT_OF_ORDER));
    assert!(seen(dir.path()).is_empty());
}

#[test]
fn a_character_name_over_48_bytes_refuses_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    let line = |name: &str| {
        serde_json::json!({ "type": "character_entered", "realm": "Stormrage", "name": name })
            .to_string()
    };
    story.send(message(7, &line(&"n".repeat(49))));
    story.send(message(8, &line(&"n".repeat(48))));

    let answers = answers(&mut story, 2);

    assert!(error(&answers[0]).starts_with(BAD_CHARACTER));
    assert_events_seen(&answers[1]);
    assert_eq!(seen(dir.path()).len(), 1);
}

#[test]
fn a_batch_of_game_events_only_gets_the_events_seen_of_the_story_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, EVENTS));

    let answers = answers(&mut story, 1);

    assert_events_seen(&answers[0]);
    assert_eq!(reply(&answers[0])["companion"], Value::Null);
    assert_eq!(seen(dir.path()).len(), 2);
}

#[test]
fn only_a_batch_with_no_reply_line_gets_a_batch_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, &format!("{EVENTS}\n{}", question("why?"))));
    story.send(message(8, EVENTS));

    answers(&mut story, 2);

    let ends = std::fs::read_to_string(dir.path().join("story/ends.txt")).unwrap();
    assert_eq!(ends, "2\n", "the batch of events is id 2");
}

#[test]
fn a_second_answer_with_the_same_id_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("twice", dir.path());
    story.send(message(7, &question("x")));

    let mut replies = answers(&mut story, 1);
    std::thread::sleep(Duration::from_millis(200));
    story.step();
    replies.extend(story.take_replies());
    story.send(message(8, &question("y")));
    let next = answers(&mut story, 1);

    assert_eq!(replies.len(), 1);
    assert_eq!(reply(&replies[0])["text"], "first");
    assert_eq!(next[0].0.id, MessageId(8), "the program goes on");
}

#[test]
fn a_talk_request_gets_the_talk_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    let talk =
        "{\"type\":\"talk_asked\",\"at\":3,\"npc\":\"Marshal Dughan\",\"text\":\"Any work?\"}";
    story.send(message(7, &format!("{CHARACTER}\n{EVENTS}\n{talk}")));

    let answers = answers(&mut story, 1);

    let reply = reply(&answers[0]);
    assert_eq!(reply["type"], "talk_answer");
    assert_eq!(reply["npc"], "Marshal Dughan");
    assert_eq!(reply["text"], "Marshal Dughan: Any work?");
}

#[test]
fn with_no_model_calls_with_no_waiting_batch_each_fail_at_once_and_give_no_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("bard", dir.path());
    story.send(message(7, EVENTS));

    let mut replies = answers(&mut story, 1);
    let start = Instant::now();
    while seen(dir.path()).len() < 5 && start.elapsed() < Duration::from_secs(10) {
        story.step();
        replies.extend(story.take_replies());
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(replies.len(), 1, "a bard call gives no reply");
    assert_events_seen(&replies[0]);
    let calls: Vec<Value> = seen(dir.path())[2..].to_vec();
    let failed: Vec<Value> = (1..=3)
        .map(|call| serde_json::json!({ "type": "model_failed", "call": call }))
        .collect();
    assert_eq!(calls, failed, "the third open call fails at once too");
}

#[test]
fn events_seen_with_a_companion_line_shows_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("companion", dir.path());
    story.send(message(7, EVENTS));
    story.send(message(8, &question("x")));

    let answers = answers(&mut story, 2);

    assert_eq!(reply(&answers[0])["companion"], "A wolf howls.");
    assert_eq!(reply(&answers[1])["companion"], "A wolf howls.");
    assert_eq!(reply(&answers[1])["type"], "lore_answer");
}

#[test]
fn a_companion_line_that_is_too_long_is_dropped_and_the_rest_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("long-companion", dir.path());
    story.send(message(7, EVENTS));
    story.send(message(8, &question("x")));

    let answers = answers(&mut story, 2);

    assert_events_seen(&answers[0]);
    assert_eq!(reply(&answers[0])["companion"], Value::Null);
    assert_eq!(reply(&answers[1])["text"], "story");
    assert_eq!(reply(&answers[1])["companion"], Value::Null);
}

#[test]
fn a_batch_of_events_with_no_events_seen_ends_done_and_empty_at_its_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = Story::new(spec("missing", dir.path(), Duration::from_secs(1)));
    story.send(message(7, EVENTS));

    let answers = answers(&mut story, 1);

    assert_eq!(answers[0].1, Ok(String::new()), "never an error");
}

#[test]
fn a_late_events_seen_is_dropped_and_the_program_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = Story::new(spec("late", dir.path(), Duration::from_secs(1)));
    story.send(message(7, EVENTS));
    let first = answers(&mut story, 1);
    // The fake program answers after 2 seconds, so its events_seen comes in here.
    std::thread::sleep(Duration::from_millis(1500));
    story.step();
    let late = story.take_replies();

    story.send(message(8, &question("x")));
    let second = answers(&mut story, 1);

    assert_eq!(first[0].1, Ok(String::new()));
    assert!(
        late.is_empty(),
        "the late events_seen gives no second reply"
    );
    assert_eq!(second[0].0.id, MessageId(8));
    assert_eq!(reply(&second[0])["text"], "story: x");
}

#[test]
fn a_waiting_batch_does_not_hold_up_the_next_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("missing", dir.path());
    story.send(message(7, EVENTS));
    story.send(message(8, &question("next")));

    let answers = answers(&mut story, 1);

    assert_eq!(answers[0].0.id, MessageId(8));
    assert_eq!(reply(&answers[0])["text"], "story: next");
}

#[test]
fn a_bad_line_of_the_addon_is_dropped_and_the_rest_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    let batch = format!(
        "not json\n{{\"type\":\"npc_met\",\"at\":1,\"name\":\"x\",\"id\":9}}\n{{\"type\":\"Npc\"}}\n{EVENTS}"
    );
    story.send(message(7, &batch));

    let answers = answers(&mut story, 1);

    assert_events_seen(&answers[0]);
    let seen = seen(dir.path());
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["type"], "zone_entered");
}

#[test]
fn a_new_kind_of_event_reaches_the_story_program_as_checked_json() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    let defeated = "{ \"type\": \"npc_defeated\", \"at\": 5, \"name\": \"Hogger\" }";
    story.send(message(7, &format!("{CHARACTER}\n{defeated}")));

    let answers = answers(&mut story, 1);

    assert_events_seen(&answers[0]);
    let seen = seen(dir.path());
    assert_eq!(
        seen[1],
        serde_json::json!({ "type": "npc_defeated", "at": 5, "name": "Hogger", "id": 1 })
    );
}

#[test]
fn a_batch_with_a_reply_line_that_is_not_last_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, &format!("{}\n{EVENTS}", question("first"))));
    story.send(message(8, &format!("{}\n{JOURNAL}", question("first"))));
    story.send(message(9, EVENTS));

    let answers = answers(&mut story, 3);

    assert!(error(&answers[0]).starts_with(OUT_OF_ORDER));
    assert!(error(&answers[1]).starts_with(OUT_OF_ORDER));
    assert_events_seen(&answers[2]);
    assert_eq!(
        seen(dir.path()).len(),
        2,
        "only the lines of the good batch"
    );
}

#[test]
fn a_journal_request_gets_the_journal_with_its_chapters_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, &format!("{EVENTS}\n{JOURNAL}")));

    let answers = answers(&mut story, 1);

    let reply = reply(&answers[0]);
    assert_eq!(reply["type"], "journal");
    assert_eq!(reply["page"], 3);
    assert_eq!(reply["places"][0]["within"], "Elwynn Forest");
    assert_eq!(reply["deeds"][0]["kind"], "level");
    let chapter = &reply["chapters"][0];
    let kinds: Vec<&str> = (0..3)
        .map(|n| chapter["deeds"][n]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["level", "defeated", "died"]);
    assert_eq!(chapter["prose"], "The road to Goldshire || began.");
    assert_eq!(seen(dir.path())[2]["id"], 1);
}

#[test]
fn an_answer_line_over_the_limit_gets_an_error_never_a_cut_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("too-long", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(TOO_LONG));
}

#[test]
fn with_no_sandbox_only_the_first_answer_carries_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("echo", dir.path());
    story.send(message(7, &question("a")));
    story.send(message(8, &question("b")));

    let answers = answers(&mut story, 2);

    assert_eq!(reply(&answers[0])["note"], NO_SANDBOX);
    assert!(reply(&answers[1]).get("note").is_none());
}

#[test]
fn an_answer_with_no_text_keeps_its_passages() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("null-text", dir.path());
    story.send(message(7, &question("why?")));

    let answers = answers(&mut story, 1);

    let reply = reply(&answers[0]);
    assert_eq!(reply["text"], Value::Null);
    assert_eq!(reply["passages"][0]["text"], "The portal hums.");
}

#[test]
fn with_no_model_a_model_call_fails_at_once_and_the_story_program_still_answers() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("model", dir.path());
    story.send(message(7, &question("why?")));

    let answers = answers(&mut story, 1);

    assert_eq!(reply(&answers[0])["text"], Value::Null);
    let failed = seen(dir.path()).pop().unwrap();
    assert_eq!(
        failed,
        serde_json::json!({ "type": "model_failed", "call": 1 })
    );
}

#[test]
fn a_crash_ends_the_waiting_question_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("crash", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(STOPPED));
}

#[test]
fn a_question_that_waits_for_a_restart_is_sent_once_the_program_is_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("crash-once", dir.path());
    story.send(message(7, &question("first")));
    let first = answers(&mut story, 1);

    story.send(message(8, &question("second")));
    wait_until_ready(&mut story);
    let second = answers(&mut story, 1);

    assert!(error(&first[0]).starts_with(STOPPED));
    assert_eq!(second[0].0.id, MessageId(8));
    assert_eq!(reply(&second[0])["text"], "story: second");
}

#[test]
fn malformed_lines_are_skipped_and_the_answer_still_comes() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("garbage", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert_eq!(reply(&answers[0])["text"], "story: x");
}

#[test]
fn a_line_over_the_size_limit_is_skipped_and_the_answer_still_comes() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("huge", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert_eq!(reply(&answers[0])["text"], "story: x");
}

#[test]
fn too_many_bad_lines_stop_the_story_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("flood", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(STOPPED));
}

#[test]
fn answers_for_unknown_ids_are_dropped_and_many_stop_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("strangers", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert_eq!(answers.len(), 1, "no answer for a stranger");
    assert!(error(&answers[0]).starts_with(STOPPED));
}

#[test]
fn an_answer_for_another_id_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("wrong-id", dir.path());
    story.send(message(7, &question("x")));

    let mut answers = answers(&mut story, 1);
    std::thread::sleep(Duration::from_millis(200));
    story.step();
    answers.extend(story.take_replies());

    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0.id, MessageId(7));
    assert_eq!(reply(&answers[0])["text"], "story: x");
}

#[test]
fn a_program_that_does_not_answer_in_time_gets_the_timeout_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = Story::new(spec("hang", dir.path(), Duration::from_secs(1)));
    story.send(message(7, &question("x")));

    let started = Instant::now();
    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(NO_ANSWER));
    assert!(started.elapsed() >= Duration::from_secs(1));
}

#[test]
fn a_program_with_no_hello_never_gets_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = Story::new(spec("no-hello", dir.path(), Duration::from_secs(1)));
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(NO_ANSWER));
    assert!(seen(dir.path()).is_empty());
}

#[test]
fn a_newer_story_program_asks_to_update_the_desktop_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("newer", dir.path());
    story.send(message(7, &question("x")));
    let first = answers(&mut story, 1);

    story.send(message(8, &question("y")));
    story.send(message(9, EVENTS));
    let later = story.take_replies();

    assert!(error(&first[0]).starts_with(UPDATE_BRIDGE));
    assert_eq!(error(&later[0]), UPDATE_BRIDGE);
    assert_eq!(later[1].1, Ok(String::new()), "never an error for events");
}

#[test]
fn an_older_story_program_asks_to_update_timeways() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("older", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(UPDATE_TIMEWAYS));
}

#[test]
fn the_story_program_gets_only_the_environment_of_the_allowlist() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = story("env", dir.path());
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    let reply = reply(&answers[0]);
    let names: Vec<&str> = reply["text"].as_str().unwrap().split(',').collect();
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!names.contains(&"CARGO_MANIFEST_DIR"));
    // The coverage build of the fake program sets this one inside its own process.
    for name in names.iter().filter(|n| !n.starts_with("__LLVM_PROFILE")) {
        assert!(
            bridge::process::BASE_ENV.contains(name),
            "{name} is not in the allowlist"
        );
    }
}

/// The fake story program starts a child that sleeps. A hang kills the whole group.
#[cfg(target_os = "linux")]
#[test]
fn a_hang_kills_the_process_group_of_the_story_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut story = Story::new(spec("fork", dir.path(), Duration::from_secs(3)));
    story.send(message(7, &question("x")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(NO_ANSWER));
    let pid = std::fs::read_to_string(dir.path().join("story/child.pid")).unwrap();
    let start = Instant::now();
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the child of the story program still runs"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn with_model(script: &str, dir: &Path, choice: ModelChoice, timeout: Duration) -> Story {
    let mut spec = spec(script, dir, Duration::from_secs(20));
    spec.model = ModelSpec {
        choice,
        timeout,
        budget_window_minutes: 20,
    };
    Story::new(spec)
}

fn local_model(server: &fake_model::Server) -> ModelChoice {
    ModelChoice::Local(LocalModel {
        url: server.url.clone(),
        model: "llama3.2".into(),
    })
}

/// Steps until the story program saw `count` lines, for at most 20 seconds.
fn wait_for_seen(story: &mut Story, dir: &Path, count: usize) -> Vec<Reply> {
    let mut replies = Vec::new();
    let start = Instant::now();
    while seen(dir).len() < count && start.elapsed() < Duration::from_secs(20) {
        story.step();
        replies.extend(story.take_replies());
        std::thread::sleep(Duration::from_millis(10));
    }
    replies
}

#[test]
fn a_model_call_gets_the_text_of_the_model_by_its_call() {
    let dir = tempfile::tempdir().unwrap();
    let server = fake_model::start(Answer::Normal);
    let choice = local_model(&server);
    let mut story = with_model("model", dir.path(), choice, Duration::from_secs(10));
    story.send(message(7, &question("why?")));

    let answers = answers(&mut story, 1);

    assert_eq!(reply(&answers[0])["text"], "heard: tell a story");
    assert_eq!(
        seen(dir.path()).pop().unwrap(),
        serde_json::json!({ "type": "model_answered", "call": 1, "text": "heard: tell a story" })
    );
}

#[test]
fn a_third_open_model_call_fails_at_once_and_the_two_open_ones_answer_later() {
    let dir = tempfile::tempdir().unwrap();
    let server = fake_model::start(Answer::Slow(Duration::from_secs(1)));
    let choice = local_model(&server);
    let mut story = with_model("bard", dir.path(), choice, Duration::from_secs(10));
    story.send(message(7, EVENTS));

    let replies = wait_for_seen(&mut story, dir.path(), 5);

    assert_eq!(replies.len(), 1, "a bard call gives no reply");
    let calls = &seen(dir.path())[2..];
    assert_eq!(
        calls[0],
        serde_json::json!({ "type": "model_failed", "call": 3 })
    );
    let answered: Vec<(&str, u64)> = calls[1..]
        .iter()
        .map(|c| (c["type"].as_str().unwrap(), c["call"].as_u64().unwrap()))
        .collect();
    assert!(answered.contains(&("model_answered", 1)), "{answered:?}");
    assert!(answered.contains(&("model_answered", 2)), "{answered:?}");
}

#[test]
fn a_model_call_that_times_out_gets_model_failed() {
    let dir = tempfile::tempdir().unwrap();
    let server = fake_model::start(Answer::Slow(Duration::from_secs(30)));
    let choice = local_model(&server);
    let mut story = with_model("model", dir.path(), choice, Duration::from_secs(1));
    story.send(message(7, &question("why?")));

    let answers = answers(&mut story, 1);

    assert_eq!(reply(&answers[0])["text"], Value::Null);
    assert_eq!(
        seen(dir.path()).pop().unwrap(),
        serde_json::json!({ "type": "model_failed", "call": 1 })
    );
}

/// The model is the fake `claude`, which writes its process id and hangs. The story
/// program crashes while the call is open, and the call ends with it.
#[cfg(target_os = "linux")]
#[test]
fn a_story_program_that_stops_ends_its_model_calls() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("claude.pid");
    let command = vec![
        env!("CARGO_BIN_EXE_fake-claude").to_owned(),
        "hang-pid".to_owned(),
        pid_file.to_string_lossy().into_owned(),
    ];
    let choice = ModelChoice::Claude {
        command,
        model: None,
    };
    let mut story = with_model("model-crash", dir.path(), choice, Duration::from_mins(1));
    story.send(message(7, &question("why?")));

    let answers = answers(&mut story, 1);

    assert!(error(&answers[0]).starts_with(STOPPED));
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let start = Instant::now();
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the model call still runs"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
