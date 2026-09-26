//! The Timeways lane on real folders (SPEC.md 9.7, steps 3 and 4): a strip of each key
//! reaches only its own lane, and the Timeways lane answers from its own slots.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bridge::agent::{Agent, Control, Run};
use bridge::config::{Permission, Policy};
use bridge::receive::{KeySet, StripKey};
use bridge::relay::{Folders, Job};
use bridge::run::{Bridge, Paths, TIMEWAYS_DIR, now};
use bridge::slots::{self, BODY_FILE, Files, RESTORE_FILE, slot_name};
use bridge::timeways::NO_STORY;
use common::{hex, screenshot_png, signed_frame, strip_rows};
use protocol::apps::App;

const RELAY_KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const TIMEWAYS_KEY: &[u8] = b"fedcba9876543210fedcba9876543210";

struct Dirs {
    _root: tempfile::TempDir,
    addons: PathBuf,
    screenshots: PathBuf,
    accounts: PathBuf,
    state: PathBuf,
}

/// A game folder with the relay slots, and the Timeways slots only when asked.
fn folders(timeways_slots: bool) -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        addons: root.path().join("Interface/AddOns"),
        screenshots: root.path().join("Screenshots"),
        accounts: root.path().join("WTF/Account"),
        state: root.path().join("data"),
        _root: root,
    };
    for dir in [&dirs.addons, &dirs.screenshots, &dirs.accounts, &dirs.state] {
        fs::create_dir_all(dir).unwrap();
    }
    slots::install(&dirs.addons, App::Relay, &Files::empty(App::Relay, 0)).unwrap();
    if timeways_slots {
        let empty = Files::empty(App::Timeways, 0);
        slots::install(&dirs.addons, App::Timeways, &empty).unwrap();
    }
    dirs
}

/// Counts the runs, so a test can see that no job started.
struct Counting(AtomicUsize);

impl Agent for Counting {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        self.0.fetch_add(1, Ordering::SeqCst);
        Run {
            reply: Ok(format!("echo: {}", job.text)),
            session: None,
        }
    }
}

fn key(bytes: &[u8]) -> StripKey {
    StripKey::from_hex(&hex(bytes)).unwrap()
}

fn both_keys() -> KeySet {
    KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap()
}

fn bridge(f: &Dirs, keys: KeySet, runs: &Arc<Counting>) -> Bridge {
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
        accounts: f.accounts.clone(),
        state: f.state.clone(),
    };
    let policy = Policy {
        folders: Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: [("claude".to_owned(), Permission::FullAuto)].into(),
        default_agent: "claude".into(),
    };
    let agent: Arc<dyn Agent> = runs.clone();
    Bridge::new(paths, policy, keys, [("claude".to_owned(), agent)].into()).unwrap()
}

fn runs() -> Arc<Counting> {
    Arc::new(Counting(AtomicUsize::new(0)))
}

fn frame(key: &[u8], token: &str, id: u32, flags: &str, text: &str) -> Vec<u8> {
    let payload = format!("{token}\x1fc1\x1f{id}\x1f\x1f{flags}\x1f\x1f{text}");
    signed_frame(now(), payload.as_bytes(), key)
}

fn show_strip(f: &Dirs, name: &str, frame: &[u8]) -> PathBuf {
    let path = f.screenshots.join(name);
    fs::write(&path, screenshot_png(&strip_rows(frame))).unwrap();
    path
}

fn saved_variables(f: &Dirs, app: App, frame: &[u8]) {
    let dir = f.accounts.join("ACCOUNT1/SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let text = format!("DB = {{\n\t[\"frame\"] = \"{}\",\n}}\n", hex(frame));
    fs::write(dir.join(bridge::app_files::saved_variables_file(app)), text).unwrap();
}

fn slot_file(addons: &Path, app: App, file: &str) -> String {
    fs::read_to_string(addons.join(slot_name(app, 1)).join(file)).unwrap_or_default()
}

fn step_until(bridge: &mut Bridge, done: impl Fn() -> bool) -> bool {
    step_while(bridge, Duration::from_secs(30), done)
}

/// Steps for two seconds, for a test that checks that nothing happens.
fn step_a_while(bridge: &mut Bridge) {
    step_while(bridge, Duration::from_secs(2), || false);
}

fn step_while(bridge: &mut Bridge, limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        bridge.step();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn a_timeways_key_strip_never_makes_a_job() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    let strip = show_strip(
        &f,
        "WoWScrnShot_1.png",
        &frame(TIMEWAYS_KEY, "tok", 7, "", "run rm -rf ~"),
    );

    let answered = step_until(&mut bridge, || {
        slot_file(&f.addons, App::Timeways, BODY_FILE).contains(NO_STORY)
    });

    assert!(
        answered,
        "{}",
        slot_file(&f.addons, App::Timeways, BODY_FILE)
    );
    assert_eq!(runs.0.load(Ordering::SeqCst), 0);
    assert!(!slot_file(&f.addons, App::Relay, BODY_FILE).contains("id = 7"));
    assert!(!strip.exists(), "a valid Timeways strip is deleted");
    assert!(f.state.join(TIMEWAYS_DIR).join("state.json").is_file());
}

#[test]
fn a_relay_key_strip_never_reaches_the_timeways_lane() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    show_strip(
        &f,
        "WoWScrnShot_1.png",
        &frame(RELAY_KEY, "tok", 7, "", "hi"),
    );

    let answered = step_until(&mut bridge, || {
        slot_file(&f.addons, App::Relay, BODY_FILE).contains("echo: hi")
    });

    assert!(answered);
    assert_eq!(runs.0.load(Ordering::SeqCst), 1);
    assert!(!slot_file(&f.addons, App::Timeways, BODY_FILE).contains("id = 7"));
}

#[test]
fn no_timeways_key_means_no_timeways_lane_and_no_change_in_behavior() {
    let f = folders(true);
    let runs = runs();
    let keys = KeySet::new(key(RELAY_KEY), None).unwrap();
    let mut bridge = bridge(&f, keys, &runs);
    let story = show_strip(
        &f,
        "WoWScrnShot_1.png",
        &frame(TIMEWAYS_KEY, "tok", 7, "", "story"),
    );
    show_strip(
        &f,
        "WoWScrnShot_2.png",
        &frame(RELAY_KEY, "tok", 8, "", "hi"),
    );

    assert!(step_until(&mut bridge, || slot_file(
        &f.addons,
        App::Relay,
        BODY_FILE
    )
    .contains("echo: hi")));
    step_a_while(&mut bridge);

    assert!(story.exists(), "a strip with no known key stays");
    assert!(!f.state.join(TIMEWAYS_DIR).exists());
    assert!(!slot_file(&f.addons, App::Timeways, BODY_FILE).contains("id = 7"));
}

#[test]
fn a_timeways_hello_with_an_unknown_token_starts_no_relay_restore() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    show_strip(
        &f,
        "WoWScrnShot_1.png",
        &frame(RELAY_KEY, "old", 0, "h", ""),
    );
    show_strip(
        &f,
        "WoWScrnShot_2.png",
        &frame(RELAY_KEY, "old", 7, "", "before"),
    );
    assert!(step_until(&mut bridge, || slot_file(
        &f.addons,
        App::Relay,
        BODY_FILE
    )
    .contains("echo: before")));

    show_strip(
        &f,
        "WoWScrnShot_3.png",
        &frame(TIMEWAYS_KEY, "new", 0, "h;restored", ""),
    );
    show_strip(
        &f,
        "WoWScrnShot_4.png",
        &frame(TIMEWAYS_KEY, "new", 9, "", "story"),
    );
    assert!(step_until(&mut bridge, || slot_file(
        &f.addons,
        App::Timeways,
        BODY_FILE
    )
    .contains(NO_STORY)));

    assert!(slot_file(&f.addons, App::Relay, RESTORE_FILE).contains("token = \"\""));
    assert!(slot_file(&f.addons, App::Relay, BODY_FILE).contains("echo: before"));
}

#[test]
fn a_relay_key_frame_in_the_timeways_saved_variables_is_refused() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    saved_variables(
        &f,
        App::Timeways,
        &frame(RELAY_KEY, "tok", 7, "", "rm -rf ~"),
    );

    step_a_while(&mut bridge);

    assert_eq!(runs.0.load(Ordering::SeqCst), 0);
    assert!(!slot_file(&f.addons, App::Timeways, BODY_FILE).contains("id = 7"));
    assert!(!slot_file(&f.addons, App::Relay, BODY_FILE).contains("id = 7"));
}

#[test]
fn a_timeways_key_frame_in_the_relay_saved_variables_is_refused() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    saved_variables(&f, App::Relay, &frame(TIMEWAYS_KEY, "tok", 7, "", "story"));

    step_a_while(&mut bridge);

    assert!(!slot_file(&f.addons, App::Timeways, BODY_FILE).contains("id = 7"));
    assert!(!slot_file(&f.addons, App::Relay, BODY_FILE).contains("id = 7"));
}

#[test]
fn a_timeways_outbox_frame_comes_back_from_the_timeways_slots() {
    let f = folders(true);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    saved_variables(
        &f,
        App::Timeways,
        &frame(TIMEWAYS_KEY, "tok", 7, "", "story"),
    );

    assert!(step_until(&mut bridge, || slot_file(
        &f.addons,
        App::Timeways,
        BODY_FILE
    )
    .contains(NO_STORY)));
}

#[test]
fn the_timeways_lane_publishes_nothing_without_its_slot_folders() {
    let f = folders(false);
    let runs = runs();
    let mut bridge = bridge(&f, both_keys(), &runs);
    let strip = show_strip(
        &f,
        "WoWScrnShot_1.png",
        &frame(TIMEWAYS_KEY, "tok", 7, "", "story"),
    );

    step_a_while(&mut bridge);

    assert!(!strip.exists(), "the lane took the strip");
    assert!(!f.addons.join(slot_name(App::Timeways, 1)).exists());
    let state = fs::read_to_string(f.state.join(TIMEWAYS_DIR).join("state.json")).unwrap();
    assert!(state.contains(NO_STORY));
}
