//! The lines of `gnomish-relay status` and of setup, on temp folders with the fake agents.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use bridge::config::Config;
use bridge::desktop::Prompt;
use bridge::gate::{Gate, Places};
use bridge::setup;
use bridge::status;

struct Computer {
    home: tempfile::TempDir,
}

impl Computer {
    fn new() -> Computer {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("Code")).unwrap();
        fs::create_dir_all(home.path().join("data")).unwrap();
        Computer { home }
    }

    fn config_dir(&self) -> std::path::PathBuf {
        self.home.path().join("config")
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.home.path().join("data")
    }

    /// A config whose default agent is `agent`, with the lines of `table`.
    fn write_config(&self, agent: &str, table: &str) -> Config {
        let text = format!(
            "allowed_roots = [\"~/Code\"]\ndefault_agent = \"{agent}\"\n[wow]\npath = \"~/wow\"\n\
             [agents.{agent}]\n{table}permission = \"auto-edit\"\n"
        );
        setup::write_config(&self.config_dir(), &text, self.home.path()).unwrap()
    }

    fn gate(&self, config: &Config) -> Gate {
        let (config_dir, data_dir) = (self.config_dir(), self.data_dir());
        let places = Places {
            config_dir: &config_dir,
            data_dir: &data_dir,
            home: self.home.path(),
        };
        Gate::new(config.relay.as_ref().unwrap(), &places, Prompt::Off)
    }
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

#[test]
fn the_agent_line_shows_the_name_and_version_of_the_agent() {
    let computer = Computer::new();
    let config = computer.write_config("claude", &fake_claude("reply"));

    let line = status::agent_line(config.relay.as_ref().unwrap(), &computer.gate(&config));

    assert_eq!(line, "Agent: claude (Claude Code 9.9.9)");
}

#[test]
fn the_agent_line_of_a_missing_login_says_how_to_log_in() {
    let computer = Computer::new();
    let config = computer.write_config("claude", &fake_claude("nologin"));

    let line = status::agent_line(config.relay.as_ref().unwrap(), &computer.gate(&config));

    assert_eq!(line, "Agent: claude needs a login. Run: claude");
}

#[test]
fn the_agent_line_of_the_echo_agent_says_how_to_set_up_an_agent() {
    let computer = Computer::new();
    let config = computer.write_config("echo", "kind = \"echo\"\n");

    let line = status::agent_line(config.relay.as_ref().unwrap(), &computer.gate(&config));

    assert!(line.starts_with("Agent: none. No agent yet."), "{line}");
}

impl Computer {
    fn status(&self) -> Vec<String> {
        let (config_dir, data_dir) = (self.config_dir(), self.data_dir());
        let places = Places {
            config_dir: &config_dir,
            data_dir: &data_dir,
            home: self.home.path(),
        };
        let path = std::env::var_os("PATH").unwrap_or_default();
        status::status_lines(&places, &path, bridge::run::now())
    }
}

#[test]
fn status_with_no_config_says_the_bridge_is_stopped_and_how_to_set_up() {
    let computer = Computer::new();

    let lines = computer.status();

    assert_eq!(lines[0], "Bridge: stopped. Start it: gnomish-relay restart");
    assert_eq!(lines[1], "Last strip: none yet. Send a message in the game");
    assert!(lines[2].starts_with("Config: does not load."), "{lines:?}");
    assert!(lines[2].contains("gnomish-relay setup"), "{lines:?}");
    assert_eq!(lines.len(), 3);
}

#[test]
fn status_of_a_config_with_a_typo_shows_the_line_of_the_error() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));
    let file = computer.config_dir().join("config.toml");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("allowed_roots", "allowed_rots")).unwrap();

    let lines = computer.status();

    assert!(lines[2].contains("allowed_rots"), "{lines:?}");
    assert!(lines[2].contains("line 1"), "{lines:?}");
}

#[test]
fn status_of_a_working_setup_shows_the_sandbox_and_the_agent() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));
    status::mark_strip(&computer.data_dir(), bridge::run::now()).unwrap();

    let lines = computer.status();

    assert!(lines[1].starts_with("Last strip: "), "{lines:?}");
    assert!(lines[1].ends_with(" s ago"), "{lines:?}");
    assert_eq!(lines[2], "Config: loads");
    assert!(lines[3].starts_with("Sandbox: "), "{lines:?}");
    assert_eq!(lines[4], "Agent: claude (Claude Code 9.9.9)");
}
