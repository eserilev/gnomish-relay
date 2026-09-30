//! `gnomish-relay check-agent` on temp folders with the fake agents.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use bridge::check_agent::check_agent;
use bridge::dirs::Dirs;
use bridge::setup;

fn dirs(home: &Path) -> Dirs {
    Dirs {
        home: home.to_owned(),
        config: home.join("config"),
        data: home.join("data"),
    }
}

/// A config whose default agent is `agent`, with the lines of `table`.
fn computer(agent: &str, table: &str) -> (tempfile::TempDir, Dirs) {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("Code")).unwrap();
    let dirs = dirs(home.path());
    fs::create_dir_all(&dirs.data).unwrap();
    let text = format!(
        "allowed_roots = [\"~/Code\"]\ndefault_agent = \"{agent}\"\n[wow]\npath = \"~/wow\"\n\
         [agents.{agent}]\n{table}permission = \"auto-edit\"\n"
    );
    setup::write_config(&dirs.config, &text, home.path()).unwrap();
    (home, dirs)
}

fn fake_claude(script: &str) -> String {
    let fake = Path::new(env!("CARGO_BIN_EXE_fake-claude"));
    let quoted = |text: &str| format!("\"{}\"", text.replace('\\', "\\\\"));
    format!(
        "kind = \"claude\"\ncommand = [{}, {}]\n",
        quoted(&fake.to_string_lossy()),
        quoted(script)
    )
}

fn check(dirs: &Dirs, name: &str) -> (anyhow::Result<()>, String) {
    let mut out = Vec::new();
    let result = check_agent(dirs, name, &mut out);
    (result, String::from_utf8(out).unwrap())
}

#[test]
fn a_working_agent_shows_its_version_and_modes_and_ends_with_ok() {
    let (_home, dirs) = computer("claude", &fake_claude("reply"));

    let (result, out) = check(&dirs, "claude");

    result.unwrap();
    assert!(out.starts_with("claude: Claude Code 9.9.9\n"), "{out}");
    assert!(out.contains("resumes sessions: yes\n"), "{out}");
    assert!(out.ends_with("ok\n"), "{out}");
}

#[test]
fn an_agent_that_the_config_does_not_name_is_an_error() {
    let (_home, dirs) = computer("claude", &fake_claude("reply"));

    let (result, out) = check(&dirs, "gemini");

    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("the config has no [agents.gemini]"),
        "{error}"
    );
    assert!(out.is_empty());
}

#[test]
fn the_echo_agent_starts_nothing_to_check() {
    let (_home, dirs) = computer("echo", "kind = \"echo\"\n");

    let (result, _) = check(&dirs, "echo");

    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("is the echo agent"), "{error}");
}
