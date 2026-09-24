//! The bridge on real folders: a screenshot in, the echo agent, a slot file out.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bridge::agent::{Agent, Echo};
use bridge::config::{Permission, Policy};
use bridge::receive::StripKey;
use bridge::relay::Folders;
use bridge::relay::Job;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{self, BODY_FILE, slot_name};
use common::{hex, screenshot_png, signed_frame, strip_rows};
use std::sync::atomic::{AtomicUsize, Ordering};

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

struct Dirs {
    _root: tempfile::TempDir,
    addons: std::path::PathBuf,
    screenshots: std::path::PathBuf,
    accounts: std::path::PathBuf,
    state: std::path::PathBuf,
}

fn folders() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("Interface/AddOns");
    let screenshots = root.path().join("Screenshots");
    fs::create_dir_all(&addons).unwrap();
    let accounts = root.path().join("WTF/Account");
    let state = root.path().join("data");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&screenshots).unwrap();
    fs::create_dir_all(&accounts).unwrap();
    slots::install(
        &addons,
        b"GnomishRelay_SlotData = nil\n",
        b"GnomishRelay_Restore = nil\n",
    )
    .unwrap();
    Dirs {
        _root: root,
        addons,
        screenshots,
        accounts,
        state,
    }
}

fn policy() -> Policy {
    Policy {
        folders: Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    }
}

fn bridge(f: &Dirs) -> Bridge {
    bridge_with(f, Arc::new(Echo))
}

fn bridge_with(f: &Dirs, agent: Arc<dyn Agent>) -> Bridge {
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
        accounts: f.accounts.clone(),
        state: f.state.clone(),
    };
    let folders = policy();
    Bridge::new(
        paths,
        folders,
        StripKey::from_hex(&hex(KEY)).unwrap(),
        agent,
    )
    .unwrap()
}

fn frame(key: &[u8], text: &str) -> Vec<u8> {
    let payload = format!("tok\x1fc1\x1f7\x1f\x1f\x1f\x1f{text}");
    signed_frame(now(), payload.as_bytes(), key)
}

fn strip_png(key: &[u8], text: &str) -> Vec<u8> {
    screenshot_png(&strip_rows(&frame(key, text)))
}

fn write_saved_variables(f: &Dirs, frame: &[u8]) {
    let dir = f.accounts.join("ACCOUNT1/SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let text = format!(
        "GnomishRelayDB = {{\n\t[\"outbox\"] = {{\n\t\t{{\n\t\t\t[\"frame\"] = \"{}\",\n\t\t}},\n\t}},\n}}\n",
        hex(frame)
    );
    fs::write(dir.join("GnomishRelay.lua"), text).unwrap();
}

fn slot_body(addons: &Path) -> String {
    fs::read_to_string(addons.join(slot_name(1)).join(BODY_FILE)).unwrap()
}

/// Steps until `done` holds. A publish syncs 60 files, which is slow on Windows.
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
fn a_strip_screenshot_comes_back_as_an_echo_in_the_slots() {
    let f = folders();
    let mut bridge = bridge(&f);
    let strip = f.screenshots.join("WoWScrnShot_1.png");
    fs::write(&strip, strip_png(KEY, "hi from the game")).unwrap();

    let answered = step_until(&mut bridge, || {
        slot_body(&f.addons).contains("echo: hi from the game")
    });
    assert!(answered, "{}", slot_body(&f.addons));
    assert!(!strip.exists(), "a valid strip is deleted");
}

#[test]
fn a_normal_screenshot_and_a_strip_with_a_bad_tag_stay_untouched() {
    let f = folders();
    let mut bridge = bridge(&f);
    let user = f.screenshots.join("WoWScrnShot_user.png");
    let forged = f.screenshots.join("WoWScrnShot_forged.png");
    fs::write(&user, screenshot_png(&[])).unwrap();
    fs::write(
        &forged,
        strip_png(b"another key, 32 bytes long......", "rm -rf ~"),
    )
    .unwrap();

    step_a_while(&mut bridge);
    assert!(user.exists());
    assert!(forged.exists());
    assert!(!slot_body(&f.addons).contains("rm -rf"));
}

#[test]
fn an_outbox_frame_in_the_saved_variables_comes_back_as_an_echo() {
    let f = folders();
    let mut bridge = bridge(&f);
    write_saved_variables(&f, &frame(KEY, "sent by reload"));

    let answered = step_until(&mut bridge, || {
        slot_body(&f.addons).contains("echo: sent by reload")
    });
    assert!(answered, "{}", slot_body(&f.addons));
}

#[test]
fn an_outbox_frame_with_a_bad_tag_never_runs() {
    let f = folders();
    let mut bridge = bridge(&f);
    write_saved_variables(&f, &frame(b"another key, 32 bytes long......", "rm -rf ~"));

    step_a_while(&mut bridge);
    assert!(!slot_body(&f.addons).contains("rm -rf"));
}

struct Counting(AtomicUsize);

impl Agent for Counting {
    fn run(&self, job: &Job) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(format!("echo: {}", job.text))
    }
}

#[test]
fn a_restarted_bridge_never_runs_an_outbox_frame_again() {
    let f = folders();
    let runs = Arc::new(Counting(AtomicUsize::new(0)));
    write_saved_variables(&f, &frame(KEY, "only once"));
    let mut first = bridge_with(&f, runs.clone());
    assert!(step_until(&mut first, || slot_body(&f.addons)
        .contains("echo: only once")));
    drop(first);

    let mut second = bridge_with(&f, runs.clone());
    step_a_while(&mut second);
    assert_eq!(runs.0.load(Ordering::SeqCst), 1);
    assert!(slot_body(&f.addons).contains("echo: only once"));
}

#[test]
fn a_damaged_state_file_stops_the_bridge_at_start() {
    let f = folders();
    fs::write(f.state.join("state.json"), "{").unwrap();
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
        accounts: f.accounts.clone(),
        state: f.state.clone(),
    };
    let folders = policy();
    let key = StripKey::from_hex(&hex(KEY)).unwrap();
    assert!(Bridge::new(paths, folders, key, Arc::new(Echo)).is_err());
}
