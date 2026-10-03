//! The lines of `gnomish-relay status` and of setup, on temp folders with the fake agents.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;

use bridge::config::Config;
use bridge::desktop::Prompt;
use bridge::gate::{Gate, Places};
use bridge::line_choice::{self, LineChoice, Reason};
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

    assert_eq!(line, "Agent: claude isn't logged in. Run claude");
}

#[test]
fn the_agent_line_of_the_echo_agent_says_how_to_set_up_an_agent() {
    let computer = Computer::new();
    let config = computer.write_config("echo", "kind = \"echo\"\n");

    let line = status::agent_line(config.relay.as_ref().unwrap(), &computer.gate(&config));

    assert!(
        line.starts_with("Agent: none. Install Claude Code or Codex"),
        "{line}"
    );
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
fn status_with_no_config_says_the_bridge_is_stopped_and_setup_did_not_finish() {
    let computer = Computer::new();

    let lines = computer.status();

    assert_eq!(
        lines[0],
        "Desktop app: stopped. To start it, run gnomish-relay restart"
    );
    assert_eq!(
        lines[1],
        "Last message from WoW: none yet. Send one in the game to test"
    );
    assert_eq!(lines[3], "Setup didn't finish. Run gnomish-relay setup.");
    assert_eq!(lines.len(), 4);
}

#[test]
fn status_says_when_the_colored_bar_is_not_measured_yet() {
    let computer = Computer::new();

    let lines = computer.status();

    assert_eq!(
        lines[2],
        "Colored bar: not measured yet. Your next message from the game measures it."
    );
}

#[test]
fn status_says_why_the_colored_bar_stays_full_size() {
    let computer = Computer::new();
    let blurred = LineChoice {
        mode: 0,
        width: 2560,
        height: 1440,
        reason: Some(Reason::Blur),
    };
    line_choice::remember(&computer.data_dir(), blurred).unwrap();

    let lines = computer.status();

    assert_eq!(
        lines[2],
        "Colored bar: full size, because your screen blurs 1-px lines (anti-aliasing, render scale, or an upscaler). Messages still get through."
    );
}

#[test]
fn status_of_a_config_with_a_typo_shows_the_line_of_the_error() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));
    let file = computer.config_dir().join("config.toml");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("allowed_roots", "allowed_rots")).unwrap();

    let lines = computer.status();

    assert!(lines[3].contains("allowed_rots"), "{lines:?}");
    assert!(lines[3].contains("line 1"), "{lines:?}");
}

#[test]
fn status_of_a_working_setup_shows_the_sandbox_and_the_agent() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));
    status::mark_strip(&computer.data_dir(), bridge::run::now()).unwrap();

    let lines = computer.status();

    assert!(lines[1].starts_with("Last message from WoW: "), "{lines:?}");
    assert!(lines[1].ends_with(" s ago"), "{lines:?}");
    assert_eq!(lines[3], "Config: OK");
    assert!(lines[4].starts_with("Sandbox: "), "{lines:?}");
    assert_eq!(lines[5], "Agent: claude (Claude Code 9.9.9)");
}

#[test]
fn status_with_no_relay_addon_says_to_get_it_on_curseforge() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));

    let lines = computer.status();

    assert_eq!(
        lines[6],
        "Addon: missing. Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW."
    );
}

#[test]
fn status_with_the_relay_addon_of_this_repo_says_it_is_ok() {
    let computer = Computer::new();
    computer.write_config("claude", &fake_claude("reply"));
    let addon = computer
        .home
        .path()
        .join("wow/Interface/AddOns/GnomishRelay");
    fs::create_dir_all(&addon).unwrap();
    let app_lua =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addon/GnomishRelay/App.lua"))
            .unwrap();
    fs::write(addon.join("App.lua"), app_lua).unwrap();

    let lines = computer.status();

    assert_eq!(lines[6], "Addon: OK");
}

/// A Timeways config: a story program and a lore pack that does not exist yet.
fn write_story_config(computer: &Computer) {
    let text = "[wow]\npath = \"~/wow\"\n[story]\nprogram = \"~/bin/timeways-story\"\n\
                lore_pack = \"~/timeways/lore.sqlite\"\n";
    setup::write_config(&computer.config_dir(), text, computer.home.path()).unwrap();
}

fn lore_line(lines: &[String]) -> &str {
    lines
        .iter()
        .find(|line| line.starts_with("Timeways lore:"))
        .expect("a lore line")
}

#[test]
fn status_says_the_lore_is_not_built_before_the_first_build() {
    let computer = Computer::new();
    write_story_config(&computer);

    let lines = computer.status();

    assert_eq!(
        lore_line(&lines),
        "Timeways lore: not built yet. The desktop app builds it while it runs."
    );
}

#[test]
fn status_shows_the_state_of_the_lore_build() {
    let computer = Computer::new();
    write_story_config(&computer);
    fs::write(computer.data_dir().join("lore-state"), "downloading 45").unwrap();

    let lines = computer.status();

    assert_eq!(lore_line(&lines), "Timeways lore: downloading (45 MB)");
}

#[test]
fn status_calls_a_pack_from_an_older_setup_ready() {
    let computer = Computer::new();
    write_story_config(&computer);
    let pack = computer.home.path().join("timeways/lore.sqlite");
    fs::create_dir_all(pack.parent().unwrap()).unwrap();
    fs::write(&pack, "lore").unwrap();

    let lines = computer.status();

    assert_eq!(lore_line(&lines), "Timeways lore: ready");
}
