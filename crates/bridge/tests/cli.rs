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
        assert!(usage.starts_with("usage:\n"), "{flag}: {usage}");
        assert!(usage.contains("gnomish-relay rules "), "{usage}");
        assert!(usage.contains("gnomish-relay rules remove <id>"), "{usage}");
        assert!(usage.contains("gnomish-relay status "), "{usage}");
    }
}

#[test]
fn an_unknown_command_prints_the_usage_as_an_error() {
    let out = gnomish_relay(&["no-such-command"]);

    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("usage:"));
}
