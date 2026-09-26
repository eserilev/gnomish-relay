//! The sandbox of the commands of a game run, with the real tool of the OS (SPEC.md 6.6.4
//! and 14.5): `bwrap` on Linux and `sandbox-exec` on macOS. Each test runs a command
//! through `gnomish-relay --sandbox-run`, as Claude Code does. With no working tool the
//! tests skip, unless `GNOMISH_REQUIRE_BWRAP` or `GNOMISH_REQUIRE_SANDBOX_EXEC` is set,
//! as in CI.
#![cfg(any(target_os = "linux", target_os = "macos"))]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use bridge::command_sandbox::{self, CommandSandbox, Guarded, RunWalls, WALLS_VAR};
use bridge::story_sandbox::{self, Sandbox};

const REQUIRE: &str = if cfg!(target_os = "macos") {
    "GNOMISH_REQUIRE_SANDBOX_EXEC"
} else {
    "GNOMISH_REQUIRE_BWRAP"
};

/// `None` skips the test on a computer with no working sandbox.
fn tool() -> Option<Sandbox> {
    let tool = story_sandbox::detect();
    if tool != Sandbox::None {
        return Some(tool);
    }
    assert!(
        std::env::var_os(REQUIRE).is_none(),
        "the sandbox tool is missing or does not work, and {REQUIRE} is set"
    );
    eprintln!("skipped: the sandbox tool is missing or does not work");
    None
}

/// A home with the keys and the state of the bridge, an ssh key, a chat folder, and a
/// second project. It is not under `/tmp`, because the sandbox hides all of `/tmp`. The
/// name of the home has a quote, a backslash, and a space, so each path of the Seatbelt
/// profile needs its escape (S32).
struct Machine {
    _root: tempfile::TempDir,
    home: PathBuf,
    config: PathBuf,
    data: PathBuf,
    chat: PathBuf,
    other: PathBuf,
}

/// The target folder, unless a part of its path is hidden, as in a worktree under
/// `.claude/`. The home cache folder then takes its place.
fn test_root() -> PathBuf {
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    if !target.components().any(|c| c.as_os_str() == ".claude") {
        return target;
    }
    let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache");
    fs::create_dir_all(&cache).unwrap();
    cache
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(test_root()).unwrap();
    let home = root.path().canonicalize().unwrap().join("ho\"me \\ x");
    let config = home.join(".config/gnomish-relay");
    let data = home.join(".local/share/gnomish-relay");
    let chat = home.join("Code/app");
    let other = home.join("Code/other");
    for dir in [
        &config,
        &data,
        &chat.join(".git/hooks"),
        &other,
        &home.join(".ssh"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(config.join("strip.key"), "secret strip key").unwrap();
    fs::write(data.join("state.json"), "secret state").unwrap();
    fs::write(home.join(".ssh/id_ed25519"), "secret ssh key").unwrap();
    fs::write(chat.join(".env"), "secret env").unwrap();
    fs::write(chat.join(".git/hooks/pre-commit"), "old hook").unwrap();
    fs::write(chat.join("notes.txt"), "plain notes").unwrap();
    Machine {
        _root: root,
        home,
        config,
        data,
        chat,
        other,
    }
}

fn walls(m: &Machine, tool: Sandbox) -> RunWalls {
    let sandbox = CommandSandbox::new(
        tool,
        PathBuf::from(env!("CARGO_BIN_EXE_gnomish-relay")),
        Some(m.home.clone()),
    );
    let guarded = Guarded {
        config_dir: &m.config,
        data_dir: &m.data,
    };
    command_sandbox::prepare(&sandbox, &guarded, &m.chat).unwrap()
}

struct Ran {
    ok: bool,
    out: String,
}

/// Runs `command` in `cwd` as Claude Code does with `CLAUDE_CODE_SHELL_PREFIX`.
fn run_in(run: &RunWalls, cwd: &Path, command: &str) -> Ran {
    let file = run
        .claude_vars(Path::new(env!("CARGO_BIN_EXE_gnomish-relay")))
        .into_iter()
        .find(|(name, _)| name == WALLS_VAR)
        .unwrap()
        .1;
    let output = Command::new(env!("CARGO_BIN_EXE_gnomish-relay"))
        .args(["--sandbox-run", command])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env(WALLS_VAR, file)
        .output()
        .unwrap();
    Ran {
        ok: output.status.success(),
        out: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn run(run: &RunWalls, m: &Machine, command: &str) -> Ran {
    run_in(run, &m.chat, command)
}

#[test]
fn a_write_inside_the_chat_folder_and_the_temp_folder_works() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(
        &w,
        &m,
        "echo made > made.txt && echo temp > \"$TMPDIR/t\" && cat notes.txt",
    );

    assert!(ran.ok, "{}", ran.out);
    assert_eq!(
        fs::read_to_string(m.chat.join("made.txt")).unwrap(),
        "made\n"
    );
    assert!(ran.out.contains("plain notes"));
    assert!(w.wrapper_ran());
}

#[test]
fn a_write_outside_the_chat_folder_fails() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    for target in [
        "../other/x",
        "$HOME/x",
        "/tmp/gnomish-relay-escape",
        "/var/tmp/gnomish-relay-escape",
    ] {
        let ran = run(&w, &m, &format!("echo x > \"{target}\""));
        assert!(!ran.ok, "{target}: {}", ran.out);
    }
    assert!(!m.other.join("x").exists());
    assert!(!m.home.join("x").exists());
    assert!(!Path::new("/tmp/gnomish-relay-escape").exists());
}

#[test]
fn a_read_of_a_hidden_path_shows_no_secret() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    for path in [
        m.config.join("strip.key"),
        m.data.join("state.json"),
        m.home.join(".ssh/id_ed25519"),
        m.chat.join(".env"),
    ] {
        let ran = run(
            &w,
            &m,
            &format!(
                "cat '{}'",
                path.display().to_string().replace('\'', "'\\''")
            ),
        );
        assert!(
            !ran.out.contains("secret"),
            "{}: {}",
            path.display(),
            ran.out
        );
    }
}

#[test]
fn a_command_cannot_change_a_git_hook() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(&w, &m, "echo evil > .git/hooks/pre-commit");

    assert!(!ran.ok, "{}", ran.out);
    assert_eq!(
        fs::read_to_string(m.chat.join(".git/hooks/pre-commit")).unwrap(),
        "old hook"
    );
}

#[test]
fn a_child_process_stays_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(&w, &m, "sh -c 'bash -c \"echo x > ../other/child\"'");

    assert!(!ran.ok, "{}", ran.out);
    assert!(!m.other.join("child").exists());
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_chat_folder_does_not_escape() {
    let Some(tool) = tool() else { return };
    let m = machine();
    std::os::unix::fs::symlink(&m.other, m.chat.join("out")).unwrap();
    std::os::unix::fs::symlink(m.home.join(".ssh"), m.chat.join("keys")).unwrap();
    let w = walls(&m, tool);

    let write = run(&w, &m, "echo x > out/x");
    let read = run(&w, &m, "cat keys/id_ed25519");

    assert!(!write.ok, "{}", write.out);
    assert!(!m.other.join("x").exists());
    assert!(!read.out.contains("secret"), "{}", read.out);
}

#[test]
fn a_command_reaches_no_network() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let ran = run(&w, &m, &format!("echo hi > /dev/tcp/127.0.0.1/{port}"));

    assert!(!ran.ok, "{}", ran.out);
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "a connection came in");
}

#[test]
fn quotes_line_breaks_and_substitutions_run_inside_the_sandbox() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run(
        &w,
        &m,
        "echo \"it's\"\necho $(echo inner) `echo tick`\necho x > ../other/q",
    );

    assert!(ran.out.contains("it's"), "{}", ran.out);
    assert!(ran.out.contains("inner tick"), "{}", ran.out);
    assert!(!m.other.join("q").exists());
}

#[test]
fn a_command_in_a_hidden_folder_does_not_start() {
    let Some(tool) = tool() else { return };
    let m = machine();
    let w = walls(&m, tool);

    let ran = run_in(&w, &m.home.join(".ssh"), "cat id_ed25519");

    assert!(!ran.ok, "{}", ran.out);
    assert!(!ran.out.contains("secret"), "{}", ran.out);
}
