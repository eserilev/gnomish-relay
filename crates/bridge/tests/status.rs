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

    assert!(line.starts_with("Agent: none. No agent set up."), "{line}");
}
