//! `gnomish-relay report` as a user runs it, in a fresh home (SPEC.md 8.5).

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use bridge::dirs::{Dirs, Os};

const STRIP_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn dirs_in(home: &Path) -> Dirs {
    let var = |name: &str| match name {
        "HOME" => Some(home.to_owned()),
        "XDG_CONFIG_HOME" => Some(home.join("config")),
        "XDG_DATA_HOME" => Some(home.join("data")),
        _ => None,
    };
    Dirs::of(Os::this(), &var).unwrap()
}

/// The command in a fresh home, so it never reads the folders of the user.
fn report_in(home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("report")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("PATH", "")
        .output()
        .unwrap()
}

fn report_path(out: &Output) -> PathBuf {
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    let line = stdout.lines().next().unwrap();
    PathBuf::from(line.trim_start_matches("Saved your bug report: "))
}

#[test]
fn report_saves_one_file_and_says_how_to_attach_it() {
    let home = tempfile::tempdir().unwrap();
    let dirs = dirs_in(home.path());

    let out = report_in(home.path());

    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(stdout.contains("Nothing was uploaded."), "{stdout}");
    assert!(
        stdout.contains(
            "To report a bug, attach this file to a new issue: https://github.com/eserilev/gnomish-relay/issues"
        ),
        "{stdout}"
    );
    let path = report_path(&out);
    assert!(path.starts_with(dirs.data.join("reports")), "{path:?}");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("== Status =="), "{text}");
}

#[test]
fn the_report_of_a_broken_config_holds_no_key_or_token() {
    let home = tempfile::tempdir().unwrap();
    let dirs = dirs_in(home.path());
    std::fs::create_dir_all(&dirs.config).unwrap();
    std::fs::write(dirs.config.join("strip.key"), STRIP_KEY).unwrap();
    std::fs::write(
        dirs.config.join("config.toml"),
        "github_token = \"ghp_fromtheconfig\"\nno_such_key = \"sk-ant-typo\"\n",
    )
    .unwrap();

    let out = report_in(home.path());

    assert!(out.status.success(), "{out:?}");
    let text = std::fs::read_to_string(report_path(&out)).unwrap();
    for secret in [STRIP_KEY, "ghp_fromtheconfig", "sk-ant-typo"] {
        assert!(!text.contains(secret), "{secret} in:\n{text}");
    }
    assert!(!text.contains(&home.path().display().to_string()), "{text}");
}
