//! The real bridge and the real `gnomish-relay hook` in a temp home, with no game
//! (SPEC.md 10.7). `hooks install` writes the hook commands, a shell runs them as Claude
//! Code does, and the Lua of the game reads the notices back from `Live.lua`.
#![cfg(unix)]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use bridge::hooks_merge::our_hooks;
use bridge::slots::{LIVE_FILE, slot_name};
use bridge::spool::Source;
use common::install_window;
use mlua::{Lua, Table};
use protocol::apps::App;
use serde_json::Value;

const PROGRAM: &str = env!("CARGO_BIN_EXE_gnomish-relay");
const CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"echo\"\n\
    [wow]\npath = \"~/wow\"\n[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n";
const USER_SETTINGS: &str = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "true"
          }
        ]
      }
    ]
  }
}
"#;

/// A home with the folders of the bridge and of the game, and nothing of the user.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn data(&self) -> PathBuf {
        self.path().join("data").join("gnomish-relay")
    }

    fn addons(&self) -> PathBuf {
        self.path().join("wow").join("Interface").join("AddOns")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(PROGRAM);
        command
            .args(args)
            .env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("XDG_DATA_HOME", self.path().join("data"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("GNOMISH_RELAY_JOB");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The config, the key, and the first slots, as `setup` makes them.
    fn set_up_bridge(&self) {
        fs::create_dir_all(self.path().join("Code")).unwrap();
        fs::create_dir_all(self.path().join("wow").join("Screenshots")).unwrap();
        let config = self.path().join("config").join("gnomish-relay");
        bridge::setup::write_config(&config, CONFIG, self.path()).unwrap();
        fs::write(config.join("strip.key"), "ab".repeat(32)).unwrap();
        fs::create_dir_all(self.addons()).unwrap();
        install_window(&self.addons(), App::Relay);
    }
}

/// Stops the bridge also when a test fails.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_bridge(home: &Home) -> Running {
    let child = home
        .command(&["run"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let running = Running(child);
    let spool = home.data().join("notices");
    assert!(
        wait_for(Duration::from_secs(30), || spool.is_dir()),
        "the bridge makes its spool folder at start"
    );
    running
}

fn wait_for(limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

/// The command of one event in the settings, as `hooks install` wrote it.
fn hook_command(settings: &Value, event: &str) -> String {
    settings["hooks"][event]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|g| {
            g["hooks"][0]["command"]
                .as_str()
                .filter(|c| c.contains(" hook "))
        })
        .unwrap()
        .to_owned()
}

/// Runs a hook command through a shell, as the agent does, with `input` on stdin.
fn run_hook(home: &Home, command: &str, input: &str, cwd: &Path) -> Output {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env_remove("GNOMISH_RELAY_JOB")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn event_input(event: &str, cwd: &Path, extra: &str) -> String {
    format!(
        r#"{{"session_id":"e2e-1","transcript_path":"/t.jsonl","cwd":"{}","hook_event_name":"{event}"{extra}}}"#,
        cwd.display()
    )
}

/// The kind, the repo, and the text of each notice.
type Rows = Vec<(String, String, String)>;

/// The `notices` table of the live file in slot 1, read by the Lua of the game.
fn live_notices(home: &Home) -> Option<(i64, i64, Rows)> {
    let path = home.addons().join(slot_name(App::Relay, 1)).join(LIVE_FILE);
    let text = fs::read(path).ok()?;
    let lua = Lua::new();
    lua.load(&text).exec().ok()?;
    let live: Table = lua.globals().get("GnomishRelay_Live").ok()?;
    let notices: Table = live.get("notices").ok()?;
    let list: Table = notices.get("list").ok()?;
    let rows = list
        .sequence_values::<Table>()
        .map(|n| {
            let n = n.unwrap();
            (
                n.get("kind").unwrap(),
                n.get("repo").unwrap(),
                n.get("text").unwrap(),
            )
        })
        .collect();
    Some((notices.get("busy").ok()?, notices.get("open").ok()?, rows))
}

fn wait_for_live(home: &Home, want: impl Fn(i64, i64, &Rows) -> bool) -> bool {
    wait_for(Duration::from_secs(10), || {
        live_notices(home).is_some_and(|(busy, open, rows)| want(busy, open, &rows))
    })
}

#[test]
fn install_merges_into_the_user_settings_and_the_hooks_reach_the_live_file() {
    let home = Home::new();
    home.set_up_bridge();
    let settings_path = home.path().join(".claude").join("settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(&settings_path, USER_SETTINGS).unwrap();

    let install = home.run(&["hooks", "install", "--claude"]);
    assert!(install.status.success(), "{install:?}");
    let settings: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(settings["model"], "opus");
    assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "true");
    for hook in our_hooks(Source::Claude) {
        assert!(hook_command(&settings, hook.event).ends_with(" hook claude"));
    }

    let _bridge = start_bridge(&home);
    let repo = home.path().join("Code").join("lighthouse");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let cwd = repo.join("src");
    fs::create_dir_all(&cwd).unwrap();
    let hook = |event: &str, extra: &str| {
        let out = run_hook(
            &home,
            &hook_command(&settings, event),
            &event_input(event, &cwd, extra),
            &cwd,
        );
        assert!(out.status.success(), "{event}: {out:?}");
        assert!(out.stdout.is_empty(), "a hook never prints");
    };

    hook("SessionStart", r#","source":"startup""#);
    hook("UserPromptSubmit", r#","prompt":"run the tests""#);
    hook(
        "Notification",
        r#","message":"Claude needs your permission to use Bash","notification_type":"permission_prompt""#,
    );
    assert!(wait_for_live(&home, |busy, open, rows| {
        busy == 1
            && open == 1
            && rows
                == &[(
                    "waiting".to_owned(),
                    "lighthouse".to_owned(),
                    "Claude needs your permission to use Bash".to_owned(),
                )]
    }));

    hook("Stop", r#","last_assistant_message":"All tests pass.""#);
    assert!(wait_for_live(&home, |busy, open, rows| {
        busy == 0
            && open == 1
            && rows.len() == 1
            && rows[0].0 == "finished"
            && rows[0].2 == "All tests pass."
    }));

    hook("SessionEnd", r#","reason":"exit""#);
    assert!(wait_for_live(&home, |_, open, rows| open == 0 && rows.is_empty()));
}

#[test]
fn a_hook_with_no_bridge_exits_0_at_once_with_an_empty_stdout() {
    let home = Home::new();
    let start = Instant::now();

    let mut child = home
        .command(&["hook", "claude"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = event_input("Stop", home.path(), "");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "{:?}",
        start.elapsed()
    );
    assert!(!home.data().join("notices").exists());
}

#[test]
fn a_hook_ends_itself_when_the_agent_never_closes_stdin() {
    let home = Home::new();
    let start = Instant::now();

    let mut child = home
        .command(&["hook", "codex"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let out = child.wait_with_output().unwrap();
    drop(stdin);

    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_hook_in_a_bridge_job_writes_nothing_to_a_running_spool() {
    let home = Home::new();
    let spool = home.data().join("notices");
    fs::create_dir_all(&spool).unwrap();

    let mut child = home
        .command(&["hook", "claude"])
        .env("GNOMISH_RELAY_JOB", "1")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let input = event_input("Stop", home.path(), "");
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    assert!(child.wait().unwrap().success());

    assert_eq!(fs::read_dir(&spool).unwrap().count(), 0);
}

#[test]
fn hooks_status_and_remove_speak_of_notifications() {
    let home = Home::new();
    let status = home.run(&["hooks", "status"]);
    assert_eq!(
        String::from_utf8(status.stdout).unwrap(),
        "Claude Code: notifications off\nCodex: notifications off\n\
         The relay is off, so no notification comes. Run: gnomish-relay setup --relay\n"
    );

    home.run(&["hooks", "install", "--codex"]);
    let status = home.run(&["hooks", "status"]);
    let removed = home.run(&["hooks", "remove"]);

    assert!(
        String::from_utf8(status.stdout)
            .unwrap()
            .contains("Codex: notifications on")
    );
    assert!(
        String::from_utf8(removed.stdout)
            .unwrap()
            .contains("Codex: notifications off.")
    );
    assert!(!home.run(&["hooks", "status", "--x"]).status.success());
}

/// A live run of the real Claude Code with the hooks in a temp settings file, so the real
/// `~/.claude` stays the same. It needs a login, so it runs only on request:
/// `cargo test --test notices_e2e -- --ignored`.
#[test]
#[ignore = "runs the real claude and needs its login"]
fn the_real_claude_fires_the_hooks_and_a_finished_notice_comes() {
    let data = tempfile::tempdir().unwrap();
    let spool = data.path().join("gnomish-relay").join("notices");
    fs::create_dir_all(&spool).unwrap();
    let settings =
        bridge::hooks_merge::install(serde_json::json!({}), Path::new(PROGRAM), Source::Claude)
            .unwrap();
    let settings_file = data.path().join("settings.json");
    fs::write(&settings_file, settings.to_string()).unwrap();

    let out = Command::new("claude")
        .args(["-p", "Reply with the word ready.", "--settings"])
        .arg(&settings_file)
        .env("XDG_DATA_HOME", data.path())
        .env_remove("GNOMISH_RELAY_JOB")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");

    let finished = || {
        let taken = bridge::spool::take_files(&spool, std::time::SystemTime::now());
        taken
            .files
            .into_iter()
            .any(|f| f.event == bridge::spool::SpoolEvent::Finished)
    };
    assert!(wait_for(Duration::from_secs(10), finished));
}
