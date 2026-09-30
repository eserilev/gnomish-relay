//! The `gnomish-relay` command itself, for the parts that need no config.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::{Command, Output};

fn gnomish_relay(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_prints_the_usage_on_stdout_and_exits_with_success() {
    for flag in ["help", "--help", "-h"] {
        let out = gnomish_relay(&[flag]);

        assert!(out.status.success(), "{flag}");
        let usage = String::from_utf8(out.stdout).unwrap();
        assert!(
            usage.starts_with("Usage: gnomish-relay <command>\n"),
            "{flag}: {usage}"
        );
        assert!(usage.contains("\n  rules "), "{usage}");
        assert!(usage.contains("\n  rules remove <id>"), "{usage}");
        assert!(usage.contains("\n  status "), "{usage}");
    }
}

#[test]
fn an_unknown_command_prints_the_usage_as_an_error() {
    let out = gnomish_relay(&["no-such-command"]);

    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("Usage:"));
}

/// The command in a fresh home, so it never reads the folders of the user.
#[cfg(unix)]
fn in_home(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .output()
        .unwrap()
}

#[cfg(unix)]
fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[cfg(unix)]
fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[cfg(unix)]
#[test]
fn approve_and_rules_with_nothing_open_say_so() {
    let home = tempfile::tempdir().unwrap();

    let approve = in_home(home.path(), &["approve"]);
    let rules = in_home(home.path(), &["rules"]);

    assert!(approve.status.success());
    assert_eq!(stdout(&approve), "Nothing is waiting for your approval.\n");
    assert!(rules.status.success());
    assert_eq!(stdout(&rules), "No Always allow rules yet.\n");
}

/// `update` runs this step in the new program, so the new version range checks the
/// new Timeways release (SPEC.md 11.4).
#[cfg(unix)]
#[test]
fn the_timeways_step_of_update_with_no_timeways_does_nothing() {
    let home = tempfile::tempdir().unwrap();

    let out = in_home(home.path(), &["update", "--timeways-only"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[cfg(unix)]
#[test]
fn an_answer_or_a_removal_of_something_unknown_is_an_error() {
    let home = tempfile::tempdir().unwrap();

    let deny = in_home(home.path(), &["deny", "a1b2c3d4e5f6"]);
    let remove = in_home(home.path(), &["rules", "remove", "zz"]);

    assert!(!deny.status.success());
    assert!(
        stderr(&deny).contains("no request a1b2c3d4e5f6 is waiting"),
        "{}",
        stderr(&deny)
    );
    assert!(!remove.status.success());
    assert!(
        stderr(&remove).contains("no rule has the id zz"),
        "{}",
        stderr(&remove)
    );
}

#[cfg(unix)]
#[test]
fn say_needs_a_number_for_the_message_id() {
    let home = tempfile::tempdir().unwrap();

    let out = in_home(home.path(), &["say", "c1", "seven", "hi"]);

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("the message id is a number"),
        "{}",
        stderr(&out)
    );
}

#[cfg(unix)]
#[test]
fn status_in_a_fresh_home_says_the_bridge_is_stopped() {
    let home = tempfile::tempdir().unwrap();

    let out = in_home(home.path(), &["status"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("Desktop app: stopped."),
        "{}",
        stdout(&out)
    );
}

/// Players get the Timeways addon from `CurseForge`, so `setup --timeways` can come first.
/// No `curl` on the `PATH`, so nothing downloads.
#[cfg(target_os = "linux")]
#[test]
fn setup_for_timeways_before_its_addon_writes_its_key_addon_and_ends_with_the_curseforge_step() {
    let home = tempfile::tempdir().unwrap();
    let addons = home.path().join("wow/Interface/AddOns");
    std::fs::create_dir_all(&addons).unwrap();
    let empty = home.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("setup")
        .arg(home.path().join("wow"))
        .arg("--timeways")
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("PATH", &empty)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        stdout.ends_with("\nGet the Timeways addon on CurseForge, then restart WoW.\n"),
        "{stdout}"
    );
    assert!(addons.join("Timeways_Key/Key.lua").is_file());
    assert!(addons.join("Timeways_S0001").is_dir());
    assert!(!addons.join("Timeways").exists());
    assert!(
        !addons.join("GnomishRelay_Key").exists(),
        "no relay with no terminal"
    );
}

/// Setup with the relay in a fresh home, with no agent and no terminal.
#[cfg(target_os = "linux")]
fn relay_setup(home: &std::path::Path) -> (Output, String) {
    std::fs::create_dir_all(home.join("wow/Interface/AddOns")).unwrap();
    let empty = home.join("empty");
    std::fs::create_dir(&empty).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("setup")
        .arg(home.join("wow"))
        .arg("--relay")
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("PATH", &empty)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let config =
        std::fs::read_to_string(home.join("config/gnomish-relay/config.toml")).unwrap_or_default();
    (out, config)
}

#[cfg(target_os = "linux")]
#[test]
fn setup_trusts_the_code_folders_it_finds_and_asks_no_folder_question() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("code/app/.git")).unwrap();

    let (out, config) = relay_setup(home.path());

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        config.contains("allowed_roots = [\"~/code\"]\n"),
        "{config}"
    );
    assert!(
        stdout
            .contains("\nAgents can work in ~/code. To add another folder, pick it in the game.\n"),
        "{stdout}"
    );
    assert!(!stdout.contains("Folders"), "{stdout}");
}

#[cfg(target_os = "linux")]
#[test]
fn setup_with_no_code_folder_trusts_no_folder_and_says_to_pick_one_in_the_game() {
    let home = tempfile::tempdir().unwrap();

    let (out, config) = relay_setup(home.path());

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(config.contains("allowed_roots = []\n"), "{config}");
    assert!(
        stdout.contains("\nPick a project folder in the game to get started.\n"),
        "{stdout}"
    );
}
