//! The bridge on real folders: a screenshot in, the echo agent, a slot file out.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bridge::agent::Echo;
use bridge::receive::StripKey;
use bridge::relay::Folders;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{self, BODY_FILE, slot_name};
use common::{hex, screenshot_png, signed_frame, strip_rows};

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

struct Dirs {
    _root: tempfile::TempDir,
    addons: std::path::PathBuf,
    screenshots: std::path::PathBuf,
}

fn folders() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("Interface/AddOns");
    let screenshots = root.path().join("Screenshots");
    fs::create_dir_all(&addons).unwrap();
    fs::create_dir_all(&screenshots).unwrap();
    slots::install(&addons, b"GnomishRelay_SlotData = nil\n").unwrap();
    Dirs {
        _root: root,
        addons,
        screenshots,
    }
}

fn bridge(f: &Dirs) -> Bridge {
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
    };
    let folders = Folders {
        roots: vec![b"/home/x".to_vec()],
        base: b"/home/x".to_vec(),
    };
    Bridge::new(
        paths,
        folders,
        StripKey::from_hex(&hex(KEY)).unwrap(),
        Arc::new(Echo),
    )
}

fn strip_png(key: &[u8], text: &str) -> Vec<u8> {
    let payload = format!("tok\x1fc1\x1f7\x1f\x1f\x1f\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), key)))
}

fn slot_body(addons: &Path) -> String {
    fs::read_to_string(addons.join(slot_name(1)).join(BODY_FILE)).unwrap()
}

/// Steps until `done` holds, for at most two seconds of agent time.
fn step_until(bridge: &mut Bridge, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
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

    step_until(&mut bridge, || false);
    assert!(user.exists());
    assert!(forged.exists());
    assert!(!slot_body(&f.addons).contains("rm -rf"));
}
