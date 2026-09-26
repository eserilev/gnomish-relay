//! The sandbox of the story program on Linux, with a real `bwrap` (SPEC.md 6.6.4 and
//! 14.5). With no working `bwrap` the tests skip, unless `GNOMISH_REQUIRE_BWRAP` is set,
//! as in CI.
#![cfg(target_os = "linux")]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bridge::lane::{ChatId, MessageId};
use bridge::process::BASE_ENV;
use bridge::story::{Story, StorySpec};
use bridge::story_sandbox::{self, Sandbox, Walls};
use bridge::timeways::StoryMessage;

/// A home with a key, a config, a replay store, and an ssh key. It is not under `/tmp`,
/// because the sandbox hides all of `/tmp`, and the tests must see each wall on its own.
struct Machine {
    _root: tempfile::TempDir,
    home: PathBuf,
    config: PathBuf,
    data: PathBuf,
    folder: PathBuf,
    pack: PathBuf,
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = root.path().join("home");
    let config = home.join(".config/gnomish-relay");
    let data = home.join(".local/share/gnomish-relay");
    let folder = data.join("timeways/story");
    for dir in [&config, &folder, &home.join(".ssh")] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(config.join("timeways.key"), "secret key").unwrap();
    fs::write(config.join("config.toml"), "allowed_roots = []").unwrap();
    fs::write(data.join("state.json"), "{}").unwrap();
    fs::write(home.join(".ssh/id_ed25519"), "private key").unwrap();
    fs::write(home.join("notes.txt"), "plain notes").unwrap();
    // Inside the hidden data folder, so only the read-only bind shows it.
    let pack = data.join("lore.sqlite");
    fs::write(&pack, "lore").unwrap();
    Machine {
        _root: root,
        home,
        config,
        data,
        folder,
        pack,
    }
}

/// `None` skips the test on a computer with no working `bwrap`.
fn bwrap() -> Option<Sandbox> {
    let sandbox = story_sandbox::detect();
    if let Sandbox::Bwrap(_) = sandbox {
        return Some(sandbox);
    }
    assert!(
        std::env::var_os("GNOMISH_REQUIRE_BWRAP").is_none(),
        "bwrap is missing or cannot make namespaces, and GNOMISH_REQUIRE_BWRAP is set"
    );
    eprintln!("skipped: bwrap is missing or cannot make namespaces");
    None
}

fn walls(h: &Machine) -> Walls {
    story_sandbox::walls(&h.folder, &h.config, &h.data, &h.home, &[&h.pack])
}

/// The text of the lore answer of the story program to each question, in order. A
/// question is at most 1 KiB, so each one goes in a batch of its own.
fn ask_each(script: &str, sandbox: Sandbox, walls: Walls, questions: &[String]) -> Vec<String> {
    let mut story = Story::new(StorySpec {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-story")),
        args: vec![script.into()],
        walls,
        sandbox,
        timeout: Duration::from_secs(20),
        model: bridge::model::ModelSpec::none(),
    });
    for (id, text) in (1..).zip(questions) {
        let question = serde_json::json!({ "type": "lore_asked", "at": 1, "question": text });
        story.send(StoryMessage {
            token: "tok".into(),
            chat: ChatId("story".into()),
            id: MessageId(id),
            name: String::new(),
            text: question.to_string(),
        });
    }
    let mut replies = Vec::new();
    let start = Instant::now();
    while replies.len() < questions.len() {
        assert!(start.elapsed() < Duration::from_secs(30), "no answer");
        story.step();
        replies.extend(story.take_replies());
        std::thread::sleep(Duration::from_millis(10));
    }
    replies.sort_by_key(|(message, _)| message.id);
    replies
        .into_iter()
        .map(|(_, reply)| {
            let line = reply.unwrap_or_else(|e| panic!("error answer: {e}"));
            let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
            reply["text"].as_str().unwrap_or_default().to_owned()
        })
        .collect()
}

fn ask(script: &str, sandbox: Sandbox, walls: Walls, text: &str) -> String {
    ask_each(script, sandbox, walls, &[text.to_owned()]).remove(0)
}

fn probe(sandbox: Sandbox, h: &Machine, checks: &[String]) -> Vec<String> {
    ask_each("probe", sandbox, walls(h), checks)
}

fn check(what: &str, path: &Path) -> String {
    format!("{what}:{}", path.display())
}

#[test]
fn the_sandboxed_story_program_gets_only_the_environment_of_the_allowlist() {
    let Some(sandbox) = bwrap() else { return };
    let h = machine();

    let names = ask("env", sandbox, walls(&h), "");

    // bwrap sets PWD to the story folder after its `--chdir`. The coverage build of the
    // fake program sets the profile variable inside its own process.
    let own = |n: &&str| *n == "PWD" || n.starts_with("__LLVM_PROFILE");
    for name in names.split(',').filter(|n| !own(n)) {
        assert!(BASE_ENV.contains(&name), "{name} is not in the allowlist");
    }
    assert!(names.contains("PATH"), "{names}");
    assert!(!names.contains("CARGO_MANIFEST_DIR"), "{names}");
}

#[test]
fn the_sandboxed_story_program_writes_only_its_own_folder() {
    let Some(sandbox) = bwrap() else { return };
    let h = machine();
    let checks = [
        check("write", &h.folder.join("notes.txt")),
        check("write", &h.data.join("state.json")),
        check("write", &h.data.join("timeways/state.json")),
        check("write", &h.config.join("config.toml")),
        check("write", &h.home.join("notes.txt")),
        check("write", &h.pack),
    ];

    let results = probe(sandbox, &h, &checks);

    assert_eq!(
        results,
        ["ok", "denied", "denied", "denied", "denied", "denied"]
    );
    assert_eq!(fs::read_to_string(&h.pack).unwrap(), "lore");
    assert_eq!(fs::read_to_string(h.data.join("state.json")).unwrap(), "{}");
    assert_eq!(
        fs::read_to_string(h.home.join("notes.txt")).unwrap(),
        "plain notes"
    );
    assert!(h.folder.join("notes.txt").is_file());
}

#[test]
fn the_sandboxed_story_program_reads_its_lore_pack_but_not_the_keys_the_config_or_ssh() {
    let Some(sandbox) = bwrap() else { return };
    let h = machine();
    let checks = [
        check("read", &h.home.join("notes.txt")),
        check("read", &h.pack),
        check("read", &h.config.join("timeways.key")),
        check("read", &h.config.join("config.toml")),
        check("read", &h.data.join("state.json")),
        check("read", &h.home.join(".ssh/id_ed25519")),
    ];

    let results = probe(sandbox, &h, &checks);

    assert_eq!(
        results,
        ["ok", "ok", "denied", "denied", "denied", "denied"]
    );
}

#[test]
fn the_sandboxed_story_program_cannot_connect_to_the_network() {
    let Some(sandbox) = bwrap() else { return };
    let h = machine();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let connect = [format!("connect:{}", listener.local_addr().unwrap())];

    let outside = probe(Sandbox::None, &h, &connect);
    let inside = probe(sandbox, &h, &connect);

    assert_eq!(outside, ["ok"], "the check works with no sandbox");
    assert_eq!(inside, ["denied"]);
}
