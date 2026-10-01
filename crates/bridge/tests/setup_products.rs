//! Gnomish Relay and Timeways are two products with one desktop app, and each setup sets
//! up only its own product (SPEC.md 9.7, decision 15). Each test runs the real command in
//! a fresh home, with a fake `curl` and no terminal, so nothing downloads.
#![cfg(target_os = "linux")]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A `curl` that writes its arguments to `curl.log`. The Timeways release answers 404,
/// as before its first publish, and every other address has no connection.
const FAKE_CURL: &str = r#"#!/bin/sh
echo "$@" >> "$HOME/curl.log"
case "$*" in
  *timeways*)
    echo "curl: (22) The requested URL returned error: 404" >&2
    exit 22
    ;;
esac
echo "curl: (7) Failed to connect" >&2
exit 7
"#;

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    /// A home with a game, the addon folders of `addons`, and the fake `curl`.
    fn new(addons: &[&str]) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let home = Home { dir };
        fs::create_dir_all(home.addons()).unwrap();
        for addon in addons {
            let folder = home.addons().join(addon);
            fs::create_dir(&folder).unwrap();
            fs::write(folder.join(format!("{addon}.toc")), "## Title: x\n").unwrap();
        }
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("curl"), FAKE_CURL).unwrap();
        fs::set_permissions(bin.join("curl"), fs::Permissions::from_mode(0o755)).unwrap();
        home
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn addons(&self) -> PathBuf {
        self.path().join("wow/Interface/AddOns")
    }

    fn config_dir(&self) -> PathBuf {
        self.path().join("config/gnomish-relay")
    }

    fn config(&self) -> String {
        fs::read_to_string(self.config_dir().join("config.toml")).unwrap_or_default()
    }

    fn has(&self, addon: &str) -> bool {
        self.addons().join(addon).exists()
    }

    fn curl_calls(&self) -> String {
        fs::read_to_string(self.path().join("curl.log")).unwrap_or_default()
    }

    fn bridge_log(&self) -> String {
        fs::read_to_string(self.path().join("data/gnomish-relay/bridge.log")).unwrap_or_default()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
            .args(args)
            .env_clear()
            .env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("XDG_DATA_HOME", self.path().join("data"))
            .env("PATH", self.path().join("bin"))
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    /// `setup` with the game of this home and `args`.
    fn setup(&self, args: &[&str]) -> String {
        let wow = self.path().join("wow");
        let mut all = vec!["setup", "--wow", wow.to_str().unwrap()];
        all.extend_from_slice(args);
        let out = self.run(&all);
        let stdout = String::from_utf8(out.stdout).unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(out.status.success(), "{stdout}{stderr}");
        assert!(!stderr.contains("curl"), "no raw curl error: {stderr}");
        stdout
    }
}

#[test]
fn plain_setup_with_a_timeways_folder_writes_nothing_of_timeways_and_prints_no_timeways_line() {
    let home = Home::new(&["Timeways"]);

    let stdout = home.setup(&[]);

    assert!(!stdout.to_lowercase().contains("timeways"), "{stdout}");
    assert!(!stdout.to_lowercase().contains("story"), "{stdout}");
    assert!(!home.config_dir().join("timeways.key").exists());
    assert!(!home.has("Timeways_Key"));
    assert!(!home.has("Timeways_S0001"));
    assert!(!home.config().contains("[story]"), "{}", home.config());
    assert!(home.config().contains("allowed_roots"), "{}", home.config());
    assert!(home.has("GnomishRelay_Key"));
    assert!(home.has("GnomishRelay_S0001"));
}

#[test]
fn plain_setup_with_a_timeways_folder_downloads_nothing_and_reaches_only_the_loopback() {
    let home = Home::new(&["Timeways"]);

    home.setup(&[]);

    let calls = home.curl_calls();
    assert!(!calls.contains("timeways"), "{calls}");
    assert!(!calls.contains("wowpedia"), "{calls}");
    for call in calls.lines() {
        assert!(call.contains("http://127.0.0.1:"), "{call}");
    }
}

#[test]
fn timeways_setup_sets_up_only_timeways_and_asks_no_relay_question() {
    let home = Home::new(&[]);

    let stdout = home.setup(&["--timeways"]);

    for line in stdout.lines() {
        assert!(
            line.contains("imeways") || line.starts_with("WoW: "),
            "a line that isn't about Timeways: {line}\n{stdout}"
        );
    }
    assert!(!stdout.contains("Also set up"), "{stdout}");
    assert!(home.config_dir().join("timeways.key").is_file());
    assert!(home.has("Timeways_Key"));
    assert!(home.has("Timeways_S0001"));
    assert!(!home.has("GnomishRelay_Key"));
    assert!(!home.has("GnomishRelay_S0001"));
    assert!(home.config().contains("[story]"), "{}", home.config());
    assert!(
        !home.config().contains("allowed_roots"),
        "{}",
        home.config()
    );
}

#[test]
fn a_failed_download_of_the_story_program_prints_the_plain_line_and_logs_the_details() {
    let home = Home::new(&["Timeways"]);

    let stdout = home.setup(&["--timeways"]);

    assert!(
        stdout.contains(
            "\nTimeways: couldn't download the story program (the release isn't published yet). \
             To try again later, run gnomish-relay setup --timeways\n"
        ),
        "{stdout}"
    );
    assert!(!stdout.contains("curl"), "{stdout}");
    let log = home.bridge_log();
    assert!(log.contains("timeways-manifest.json"), "{log}");
    assert!(log.contains("404"), "{log}");
}

#[test]
fn timeways_setup_with_the_relay_flag_stops_before_the_first_file() {
    let home = Home::new(&[]);

    let out = home.run(&["setup", "--timeways", "--relay"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("Set up one app at a time"), "{stderr}");
    assert!(!home.config_dir().exists());
}

#[test]
fn relay_setup_then_timeways_setup_keeps_both_parts() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);
    let relay_config = home.config();
    let relay_key = fs::read_to_string(home.config_dir().join("strip.key")).unwrap();

    let stdout = home.setup(&["--timeways"]);

    let config = home.config();
    assert!(config.starts_with(&relay_config), "{config}");
    assert!(config.contains("[story]"), "{config}");
    assert_eq!(
        fs::read_to_string(home.config_dir().join("strip.key")).unwrap(),
        relay_key
    );
    assert!(home.has("GnomishRelay_Key") && home.has("GnomishRelay_S0001"));
    assert!(home.has("Timeways_Key") && home.has("Timeways_S0001"));
    assert!(!stdout.contains("Agent"), "{stdout}");
}

#[test]
fn timeways_setup_then_relay_setup_keeps_both_parts() {
    let home = Home::new(&["Timeways"]);
    home.setup(&["--timeways"]);
    let timeways_config = home.config();
    let timeways_key = fs::read_to_string(home.config_dir().join("timeways.key")).unwrap();

    let stdout = home.setup(&[]);

    let config = home.config();
    assert!(config.contains(&timeways_config), "{config}");
    assert!(config.contains("allowed_roots"), "{config}");
    assert_eq!(
        fs::read_to_string(home.config_dir().join("timeways.key")).unwrap(),
        timeways_key
    );
    assert!(home.has("GnomishRelay_Key") && home.has("GnomishRelay_S0001"));
    assert!(home.has("Timeways_Key") && home.has("Timeways_S0001"));
    assert!(!stdout.to_lowercase().contains("timeways"), "{stdout}");
}

#[test]
fn a_new_key_in_one_setup_keeps_the_key_of_the_other_product() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);
    home.setup(&["--timeways"]);
    let relay_key = fs::read_to_string(home.config_dir().join("strip.key")).unwrap();
    let timeways_key = fs::read_to_string(home.config_dir().join("timeways.key")).unwrap();

    home.setup(&["--new-key"]);

    let new_relay_key = fs::read_to_string(home.config_dir().join("strip.key")).unwrap();
    assert_ne!(new_relay_key, relay_key);
    assert_eq!(
        fs::read_to_string(home.config_dir().join("timeways.key")).unwrap(),
        timeways_key
    );
}

#[test]
fn status_with_no_timeways_part_prints_no_timeways_line() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);

    let out = home.run(&["status"]);

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.to_lowercase().contains("timeways"), "{stdout}");
    assert!(!stdout.to_lowercase().contains("story"), "{stdout}");
}

#[test]
fn status_of_timeways_alone_prints_no_relay_line() {
    let home = Home::new(&["Timeways"]);
    home.setup(&["--timeways"]);

    let out = home.run(&["status"]);

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("Coding agents"), "{stdout}");
    assert!(!stdout.contains("Addon:"), "{stdout}");
}

#[test]
fn update_of_timeways_with_no_timeways_part_downloads_nothing() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);
    let before = home.curl_calls();

    let out = home.run(&["update", "--timeways-only"]);

    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "");
    assert_eq!(home.curl_calls(), before);
}

#[test]
fn install_with_no_timeways_key_makes_no_timeways_addon_files() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);

    let out = home.run(&["install"]);

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!home.has("Timeways_S0001"));
    assert!(home.has("GnomishRelay_S0001"));
}

/// The path of the player: no project folder at the first setup, one at a later setup.
/// The later setup adds the root and `default_cwd = "~"`.
#[test]
fn timeways_setup_after_a_relay_setup_that_added_a_root_to_the_home_folder_base() {
    let home = Home::new(&["Timeways"]);
    home.setup(&[]);
    fs::create_dir_all(home.path().join("Documents/Code/app/.git")).unwrap();
    home.setup(&[]);
    let config = home.config();
    assert!(
        config.contains("allowed_roots = [\"~/Documents/Code\"]"),
        "{config}"
    );
    assert!(config.contains("default_cwd = \"~\""), "{config}");
    let status = String::from_utf8(home.run(&["status"]).stdout).unwrap();
    assert!(status.contains("Config: OK"), "{status}");

    let stdout = home.setup(&["--timeways"]);

    assert!(home.config().contains("[story]"), "{}", home.config());
    assert!(home.has("Timeways_Key") && home.has("Timeways_S0001"));
    assert!(!stdout.contains("allowed_roots"), "{stdout}");
}

/// A relay setup with `~/Documents/Code` as its root, and then `top_line` as the first line.
fn relay_config_with(home: &Home, top_line: &str) {
    fs::create_dir_all(home.path().join("Documents/Code")).unwrap();
    home.setup(&["--roots", "~/Documents/Code"]);
    let path = home.config_dir().join("config.toml");
    fs::write(&path, format!("{top_line}\n{}", home.config())).unwrap();
}

const REPAIRED: &str =
    "Fixed config.toml: default_cwd / isn't in allowed_roots, so it's now ~ (your home folder).";

#[test]
fn relay_setup_repairs_a_default_folder_outside_the_roots() {
    let home = Home::new(&[]);
    relay_config_with(&home, "default_cwd = \"/\"");

    let stdout = home.setup(&[]);

    assert!(stdout.contains(REPAIRED), "{stdout}");
    let config = home.config();
    assert!(config.starts_with("default_cwd = \"~\"\n"), "{config}");
    assert!(!config.contains("\"/\""), "{config}");
    let status = String::from_utf8(home.run(&["status"]).stdout).unwrap();
    assert!(status.contains("Config: OK"), "{status}");
}

#[test]
fn timeways_setup_repairs_a_default_folder_outside_the_roots() {
    let home = Home::new(&["Timeways"]);
    relay_config_with(&home, "default_cwd = \"/\"");

    let stdout = home.setup(&["--timeways"]);

    assert!(stdout.contains(REPAIRED), "{stdout}");
    assert!(home.config().contains("default_cwd = \"~\"\n"));
    assert!(home.config().contains("[story]"), "{}", home.config());
}

#[test]
fn relay_setup_repairs_a_default_folder_that_does_not_exist() {
    let home = Home::new(&[]);
    relay_config_with(&home, "default_cwd = \"~/Documents/Code/gone\"");

    let stdout = home.setup(&[]);

    assert!(
        stdout.contains(
            "Fixed config.toml: default_cwd ~/Documents/Code/gone doesn't exist, so it's now ~ \
             (your home folder)."
        ),
        "{stdout}"
    );
    assert!(home.config().starts_with("default_cwd = \"~\"\n"));
}

#[test]
fn timeways_setup_goes_on_with_an_error_in_the_relay_part_and_says_so_in_one_line() {
    let home = Home::new(&["Timeways"]);
    relay_config_with(&home, "max_parallel_runs = 0");

    let stdout = home.setup(&["--timeways"]);

    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains("config.toml"))
        .collect();
    assert_eq!(
        lines,
        [
            "Gnomish Relay: config.toml has an error, and the desktop app won't start until it's \
          fixed: max_parallel_runs must be 1 to 16"
        ],
        "{stdout}"
    );
    assert!(home.config().starts_with("max_parallel_runs = 0\n"));
    assert!(home.config().contains("[story]"), "{}", home.config());
    assert!(home.has("Timeways_Key") && home.has("Timeways_S0001"));
}

#[test]
fn relay_setup_still_stops_on_an_error_in_the_relay_part() {
    let home = Home::new(&[]);
    relay_config_with(&home, "max_parallel_runs = 0");

    let out = home.run(&["setup"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("max_parallel_runs must be 1 to"),
        "{stderr}"
    );
}
