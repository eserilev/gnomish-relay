//! A SIGTERM to the desktop app ends every program that it started, also a child of a
//! child, with no sandbox to end them (SPEC.md 9.4).
#![cfg(target_os = "linux")]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PROGRAM: &str = env!("CARGO_BIN_EXE_gnomish-relay");

/// Stops the desktop app also when a test fails.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A story program whose background child writes a beat every 100 ms. It never answers
/// the hello, so it never gets ready, and that is enough here.
fn story_program(beat: &Path) -> String {
    format!(
        "#!/bin/sh\n(while true; do echo x >> '{}'; sleep 0.1; done) &\nwhile read -r line; do :; done\n",
        beat.display()
    )
}

/// A Timeways setup with no relay: the story program starts with the desktop app.
fn set_up(home: &Path, beat: &Path) {
    let config = home.join("config/gnomish-relay");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(home.join("wow/Interface/AddOns")).unwrap();
    fs::create_dir_all(home.join("wow/Screenshots")).unwrap();
    fs::create_dir_all(home.join("bin")).unwrap();
    let program = home.join("bin/timeways-story");
    fs::write(&program, story_program(beat)).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(home.join("lore.sqlite"), "").unwrap();
    let text = "[wow]\npath = \"~/wow\"\n[story]\nprogram = \"~/bin/timeways-story\"\n\
                lore_pack = \"~/lore.sqlite\"\n";
    bridge::setup::write_config(&config, text, home).unwrap();
    fs::write(config.join("strip.key"), "ab".repeat(32)).unwrap();
    fs::write(config.join("timeways.key"), "cd".repeat(32)).unwrap();
}

/// A `PATH` with `sleep` and no `bwrap`, so the story program runs with no sandbox.
fn path_with_sleep(home: &Path) -> String {
    let tools = home.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for tool in ["sleep", "sh"] {
        let out = Command::new("sh")
            .args(["-c", &format!("command -v {tool}")])
            .output()
            .unwrap();
        let found = String::from_utf8(out.stdout).unwrap();
        std::os::unix::fs::symlink(found.trim(), tools.join(tool)).unwrap();
    }
    tools.display().to_string()
}

fn wait_for(limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn exits_within(child: &mut Child, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if let Ok(Some(_)) = child.try_wait() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn beat_stops(beat: &Path) -> bool {
    thread::sleep(Duration::from_millis(500));
    let before = fs::read_to_string(beat).unwrap().len();
    thread::sleep(Duration::from_millis(600));
    before == fs::read_to_string(beat).unwrap().len()
}

#[test]
fn a_sigterm_to_the_desktop_app_ends_its_children_and_their_children() {
    let home = tempfile::tempdir().unwrap();
    let beat = home.path().join("beat");
    set_up(home.path(), &beat);
    let child = Command::new(PROGRAM)
        .arg("run")
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("PATH", path_with_sleep(home.path()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    assert!(
        wait_for(Duration::from_secs(30), || beat.exists()),
        "the story program never started"
    );

    let pid = running.0.id().to_string();
    let sent = Command::new("kill").args(["-TERM", &pid]).status().unwrap();

    assert!(sent.success());
    assert!(
        exits_within(&mut running.0, Duration::from_secs(10)),
        "the desktop app didn't exit"
    );
    assert!(beat_stops(&beat), "a child of the story program still runs");
}
