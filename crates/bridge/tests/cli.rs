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

/// The config folder that `in_home` gives the command: macOS has no XDG folders.
#[cfg(unix)]
fn config_dir_in(home: &std::path::Path) -> std::path::PathBuf {
    use bridge::dirs::{Dirs, Os};
    let var = |name: &str| match name {
        "HOME" => Some(home.to_owned()),
        "XDG_CONFIG_HOME" => Some(home.join("config")),
        "XDG_DATA_HOME" => Some(home.join("data")),
        _ => None,
    };
    Dirs::of(Os::this(), &var).unwrap().config
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

/// A setup that stopped after the keys, as in a fresh-install test before 0.3.1.
#[cfg(unix)]
#[test]
fn restart_and_run_with_only_the_keys_say_that_setup_did_not_finish() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = config_dir_in(home.path());
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("strip.key"), "ab".repeat(32)).unwrap();

    let restart = in_home(home.path(), &["restart"]);
    let run = in_home(home.path(), &["run"]);

    for out in [restart, run] {
        assert!(!out.status.success());
        let error = stderr(&out);
        assert!(
            error.contains("Setup didn't finish. Run gnomish-relay setup."),
            "{error}"
        );
        assert!(!error.contains("has an error"), "{error}");
    }
}

#[cfg(unix)]
#[test]
fn restart_with_a_config_that_does_not_parse_keeps_the_error_with_its_line() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let config_dir = config_dir_in(home.path());
    std::fs::create_dir_all(&config_dir).unwrap();
    let file = config_dir.join("config.toml");
    std::fs::write(&file, "allowed_rots = []\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let out = in_home(home.path(), &["restart"]);

    assert!(!out.status.success());
    let error = stderr(&out);
    assert!(error.contains("config.toml has an error"), "{error}");
    assert!(error.contains("line 1"), "{error}");
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

#[cfg(target_os = "linux")]
#[test]
fn setup_again_fills_empty_roots_with_the_code_folders_it_finds_now() {
    let home = tempfile::tempdir().unwrap();
    let (_, first) = relay_setup(home.path());
    assert!(first.contains("allowed_roots = []\n"), "{first}");
    std::fs::create_dir_all(home.path().join("Documents/Code/Personal/app/.git")).unwrap();

    let out = setup_in(home.path(), &[]);

    let config = config_of(home.path());
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(
        config.contains("allowed_roots = [\"~/Documents/Code\"]\ndefault_cwd = \"~\"\n"),
        "{config}"
    );
    assert!(
        stdout(&out).contains("\nAgents can work in ~/Documents/Code."),
        "{}",
        stdout(&out)
    );
}

/// Setup with the given arguments in a fresh home, with no agent and stdin closed.
#[cfg(target_os = "linux")]
fn setup_in(home: &std::path::Path, args: &[&str]) -> Output {
    let empty = home.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("setup")
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("PATH", &empty)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(target_os = "linux")]
fn config_of(home: &std::path::Path) -> String {
    std::fs::read_to_string(home.join("config/gnomish-relay/config.toml")).unwrap_or_default()
}

/// A WoW install in a Wine prefix, last played `age` seconds ago.
#[cfg(target_os = "linux")]
fn game_played(prefix: &std::path::Path, age: u64) -> std::path::PathBuf {
    let game = prefix.join("drive_c/Program Files (x86)/World of Warcraft/_classic_beta_");
    std::fs::create_dir_all(game.join("WTF")).unwrap();
    let saved = game.join("WTF/Config.wtf");
    std::fs::write(&saved, "SET x 1\n").unwrap();
    let time = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
    std::fs::File::options()
        .write(true)
        .open(&saved)
        .unwrap()
        .set_modified(time)
        .unwrap();
    game
}

#[cfg(target_os = "linux")]
#[test]
fn with_two_installs_setup_takes_the_one_played_last_and_asks_nothing() {
    let home = tempfile::tempdir().unwrap();
    game_played(&home.path().join(".wine"), 86_400);
    let newer = game_played(&home.path().join("Games/battlenet"), 60);

    let out = setup_in(home.path(), &[]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        stdout.starts_with(&format!(
            "Using WoW at {}. To use another one, run gnomish-relay setup --wow <folder>.\n",
            newer.display()
        )),
        "{stdout}"
    );
    let config = config_of(home.path());
    assert!(
        config.contains(&format!("path = \"{}\"", newer.display())),
        "{config}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_wow_flag_overrides_the_install_played_last_and_changes_the_config() {
    let home = tempfile::tempdir().unwrap();
    let older = game_played(&home.path().join(".wine"), 86_400);
    let newer = game_played(&home.path().join("Games/battlenet"), 60);
    setup_in(home.path(), &[]);

    let out = setup_in(home.path(), &["--wow", &older.to_string_lossy()]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        stdout.starts_with(&format!("WoW: {}\n", older.display())),
        "{stdout}"
    );
    let config = config_of(home.path());
    assert!(
        config.contains(&format!("path = \"{}\"", older.display())),
        "{config}"
    );
    assert!(!config.contains(&*newer.to_string_lossy()), "{config}");
}

#[cfg(target_os = "linux")]
#[test]
fn with_no_wow_setup_finishes_the_rest_and_ends_with_what_to_do() {
    let home = tempfile::tempdir().unwrap();

    let out = setup_in(home.path(), &[]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        stdout.ends_with("\nWoW not found. Start WoW once, then run gnomish-relay setup.\n"),
        "{stdout}"
    );
    let config = config_of(home.path());
    assert!(config.contains("allowed_roots = []\n"), "{config}");
    assert!(!config.contains("[wow]"), "{config}");
    assert!(home.path().join("config/gnomish-relay/strip.key").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn with_no_wow_the_desktop_app_ends_cleanly_and_status_and_restart_say_what_to_do() {
    let home = tempfile::tempdir().unwrap();
    setup_in(home.path(), &[]);

    let run = in_home(home.path(), &["run"]);
    let status = in_home(home.path(), &["status"]);
    let restart = in_home(home.path(), &["restart"]);

    let line = "WoW not found. Start WoW once, then run gnomish-relay setup.";
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run), format!("{line}\n"));
    assert!(status.status.success(), "{}", stderr(&status));
    assert!(
        stdout(&status).contains(&format!("\nConfig: OK\n{line}\n")),
        "{}",
        stdout(&status)
    );
    assert!(!restart.status.success());
    assert!(stderr(&restart).contains(line), "{}", stderr(&restart));
}

#[cfg(target_os = "linux")]
#[test]
fn a_setup_after_the_first_start_of_wow_adds_the_game_to_the_config() {
    let home = tempfile::tempdir().unwrap();
    setup_in(home.path(), &[]);
    let game = game_played(&home.path().join(".wine"), 60);

    let out = setup_in(home.path(), &[]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(!stdout.contains("WoW not found"), "{stdout}");
    let config = config_of(home.path());
    assert!(
        config.contains(&format!("[wow]\npath = \"{}\"\n", game.display())),
        "{config}"
    );
    assert!(game.join("Interface/AddOns/GnomishRelay_Key").is_dir());
}

/// A setup that stopped at a question after the keys and before `config.toml`.
#[cfg(target_os = "linux")]
#[test]
fn the_next_setup_completes_a_config_folder_with_only_the_keys() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let game = home.path().join("wow");
    std::fs::create_dir_all(&game).unwrap();
    let config_dir = home.path().join("config/gnomish-relay");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (file, key) in [("strip.key", "ab"), ("timeways.key", "cd")] {
        std::fs::write(config_dir.join(file), key.repeat(32)).unwrap();
        std::fs::set_permissions(
            config_dir.join(file),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }

    let out = setup_in(home.path(), &["--wow", &game.to_string_lossy()]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        config_of(home.path()).contains("allowed_roots = []\n"),
        "{}",
        config_of(home.path())
    );
    let strip_key = std::fs::read_to_string(config_dir.join("strip.key")).unwrap();
    assert_eq!(strip_key, "ab".repeat(32), "the key stays");
    let key_addon =
        std::fs::read_to_string(game.join("Interface/AddOns/GnomishRelay_Key/Key.lua")).unwrap();
    assert!(key_addon.contains(&"ab".repeat(32)), "{key_addon}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_second_setup_keeps_the_wow_folder_of_the_config() {
    let home = tempfile::tempdir().unwrap();
    let older = game_played(&home.path().join(".wine"), 86_400);
    setup_in(home.path(), &["--wow", &older.to_string_lossy()]);
    game_played(&home.path().join("Games/battlenet"), 60);

    let out = setup_in(home.path(), &[]);

    let stdout = stdout(&out);
    assert!(out.status.success(), "{stdout}{}", stderr(&out));
    assert!(
        stdout.starts_with(&format!("WoW: {}\n", older.display())),
        "{stdout}"
    );
}

#[test]
fn dev_takes_only_the_end_flag() {
    let out = gnomish_relay(&["dev", "--now"]);

    assert!(!out.status.success());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.contains("usage: cargo run -- dev [--end]"), "{error}");
}

#[cfg(unix)]
#[test]
fn dev_end_with_no_session_says_so() {
    let home = tempfile::tempdir().unwrap();

    let out = in_home(home.path(), &["dev", "--end"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "Dev mode isn't on.\n");
}

/// A game with the addon as the `CurseForge` app installs it: a real folder.
#[cfg(target_os = "linux")]
fn game_with_release_addon(home: &std::path::Path) -> std::path::PathBuf {
    let game = game_played(&home.join(".wine"), 60);
    let addon = game.join("Interface/AddOns/GnomishRelay");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(addon.join("GnomishRelay.toc"), "release").unwrap();
    setup_in(home, &[]);
    addon
}

/// `dev` in the background, with its lines on a channel. Stderr lines start with `stderr: `.
#[cfg(target_os = "linux")]
fn start_dev(home: &std::path::Path) -> (std::process::Child, std::sync::mpsc::Receiver<String>) {
    use std::io::BufRead;
    let mut child = Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .arg("dev")
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("PATH", home.join("empty"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (send, lines) = std::sync::mpsc::channel();
    let out = std::io::BufReader::new(child.stdout.take().unwrap());
    let err = std::io::BufReader::new(child.stderr.take().unwrap());
    let send_err = send.clone();
    std::thread::spawn(move || {
        for line in out.lines().map_while(Result::ok) {
            let _ = send.send(line);
        }
    });
    std::thread::spawn(move || {
        for line in err.lines().map_while(Result::ok) {
            let _ = send_err.send(format!("stderr: {line}"));
        }
    });
    (child, lines)
}

/// Waits for a line that starts with `start`, for at most 60 seconds.
#[cfg(target_os = "linux")]
fn wait_for_line(lines: &std::sync::mpsc::Receiver<String>, start: &str) -> Vec<String> {
    let mut seen = Vec::new();
    while let Ok(line) = lines.recv_timeout(std::time::Duration::from_mins(1)) {
        let found = line.starts_with(start);
        seen.push(line);
        if found {
            return seen;
        }
    }
    panic!("no line {start:?}: {seen:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn dev_links_the_checkout_and_ctrl_c_puts_the_release_addon_back() {
    let home = tempfile::tempdir().unwrap();
    let addon = game_with_release_addon(home.path());
    let checkout = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../addon/GnomishRelay")
        .canonicalize()
        .unwrap();
    let (mut dev, lines) = start_dev(home.path());

    wait_for_line(&lines, "Press Ctrl-C");
    let during = std::fs::read_link(&addon).unwrap();
    let interrupted = Command::new("kill")
        .args(["-INT", &dev.id().to_string()])
        .status()
        .unwrap();
    let ended = wait_for_line(&lines, "Dev mode is off");
    let status = dev.wait().unwrap();

    assert!(interrupted.success());
    assert_eq!(during, checkout);
    assert!(status.success(), "{ended:?}");
    assert!(
        !std::fs::symlink_metadata(&addon)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(addon.join("GnomishRelay.toc")).unwrap(),
        "release"
    );
    assert!(
        !addon
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("GnomishRelay.dev-backup")
            .exists()
    );
    assert!(
        !home
            .path()
            .join("data/gnomish-relay/dev-mode.json")
            .exists()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_second_dev_while_one_runs_is_refused_and_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let addon = game_with_release_addon(home.path());
    let (mut first, lines) = start_dev(home.path());
    wait_for_line(&lines, "Press Ctrl-C");

    let second = in_home(home.path(), &["dev"]);
    let end = in_home(home.path(), &["dev", "--end"]);
    let still_linked = std::fs::symlink_metadata(&addon)
        .unwrap()
        .file_type()
        .is_symlink();
    Command::new("kill")
        .args(["-INT", &first.id().to_string()])
        .status()
        .unwrap();
    first.wait().unwrap();

    assert!(!second.status.success());
    assert!(
        stderr(&second).contains("already on in another terminal"),
        "{}",
        stderr(&second)
    );
    assert!(!end.status.success());
    assert!(still_linked);
}
