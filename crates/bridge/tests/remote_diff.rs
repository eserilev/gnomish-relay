//! A game half and a server half, with the real `gnomish-relay attach` between them and
//! no ssh (SPEC.md 11.7.11). The diff test sends the same strips to a plain desktop app,
//! and the slot files of both must be the same, apart from the times.
#![cfg(unix)]
// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bridge::agent::{Agent, Echo};
use bridge::config::{Permission, Policy, Remote};
use bridge::desktop::{Approvals, Prompt, Verdict};
use bridge::dirs::{Dirs, Os};
use bridge::folder_path::path_bytes;
use bridge::game_folders::GameFolders;
use bridge::ids::hex;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::remote::{RemoteLane, read_status};
use bridge::run::{Bridge, Paths, now};
use bridge::serve::{ServeLink, listen};
use bridge::slots::{BODY_FILE, LIVE_FILE, RESTORE_FILE, slot_name};
use common::{install_window, screenshot_png, signed_frame, strip_rows};
use protocol::apps::App;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const BIN: &str = env!("CARGO_BIN_EXE_gnomish-relay");

struct Game {
    addons: PathBuf,
    screenshots: PathBuf,
    accounts: PathBuf,
    state: PathBuf,
}

fn game(root: &Path, name: &str) -> Game {
    let base = root.join(name);
    let game = Game {
        addons: base.join("Interface/AddOns"),
        screenshots: base.join("Screenshots"),
        accounts: base.join("WTF/Account"),
        state: base.join("data"),
    };
    for dir in [&game.addons, &game.screenshots, &game.accounts, &game.state] {
        fs::create_dir_all(dir).unwrap();
    }
    install_window(&game.addons, App::Relay);
    game
}

fn paths(game: &Game) -> Paths {
    Paths {
        games: vec![GameFolders {
            name: "game".into(),
            addons: game.addons.clone(),
            screenshots: game.screenshots.clone(),
            accounts: game.accounts.clone(),
        }],
        state: game.state.clone(),
        config: game.state.join("config"),
    }
}

fn policy() -> Policy {
    let base = path_bytes(&std::env::temp_dir().canonicalize().unwrap());
    Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    }
}

fn relay_keys() -> KeySet {
    KeySet::new(vec![(App::Relay, StripKey::from_hex(&hex(KEY)).unwrap())]).unwrap()
}

fn agents() -> BTreeMap<String, Arc<dyn Agent>> {
    [("claude".to_owned(), Arc::new(Echo) as Arc<dyn Agent>)].into()
}

/// The server half reads its folders from the environment, as `attach` does.
struct Server {
    env: Vec<(String, PathBuf)>,
    data: PathBuf,
}

fn server(root: &Path) -> Server {
    let env: Vec<(String, PathBuf)> = vec![
        ("HOME".into(), root.join("server-home")),
        ("XDG_DATA_HOME".into(), root.join("server-data")),
        ("XDG_CONFIG_HOME".into(), root.join("server-config")),
    ];
    let var = |name: &str| {
        env.iter()
            .find(|(n, _)| n == name)
            .map(|(_, path)| path.clone())
    };
    let data = Dirs::of(Os::this(), &var).unwrap().data;
    fs::create_dir_all(&data).unwrap();
    Server { env, data }
}

fn server_half(server: &Server) -> Bridge {
    let paths = Paths {
        games: Vec::new(),
        state: server.data.clone(),
        config: server.data.join("config"),
    };
    let link = ServeLink::new(
        listen(&server.data).unwrap(),
        Approvals::new(&server.data, Prompt::Off),
    );
    Bridge::new(paths, policy(), KeySet::new(Vec::new()).unwrap(), agents())
        .unwrap()
        .with_link(link)
}

/// `serve` refuses a peer that descends from it (SPEC.md 11.7.8), and here `serve` runs
/// in the test. sshd is the parent of `attach` on a real server; `setsid -f` stands in.
fn detached() -> Vec<String> {
    if cfg!(target_os = "linux") {
        vec!["setsid".to_owned(), "-f".to_owned()]
    } else {
        Vec::new()
    }
}

/// `env` sets the folders of the server for `attach`, as a login shell of ssh would.
fn game_half(game: &Game, server: &Server) -> Bridge {
    let mut command = detached();
    command.push("env".to_owned());
    command.extend(
        server
            .env
            .iter()
            .map(|(name, path)| format!("{name}={}", path.display())),
    );
    command.extend([BIN.to_owned(), "attach".to_owned()]);
    let remote = Remote {
        host: "test-server".into(),
        command,
    };
    let paths = paths(game);
    let lane = RemoteLane::new(&remote, &paths.games, &paths.state, Prompt::Off);
    Bridge::without_relay(paths, relay_keys())
        .unwrap()
        .with_remote(lane)
}

fn plain(game: &Game) -> Bridge {
    Bridge::new(paths(game), policy(), relay_keys(), agents()).unwrap()
}

fn write_strip(game: &Game, file: &str, id: u32, text: &str) {
    let payload = format!("tok\x1fc1\x1f{id}\x1f\x1f\x1f\x1f{text}");
    let frame = signed_frame(now(), payload.as_bytes(), KEY);
    fs::write(
        game.screenshots.join(file),
        screenshot_png(&strip_rows(&frame)),
    )
    .unwrap();
}

fn slot_file(game: &Game, file: &str) -> String {
    fs::read_to_string(game.addons.join(slot_name(App::Relay, 1)).join(file)).unwrap_or_default()
}

fn step_all(bridges: &mut [&mut Bridge], limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        for bridge in bridges.iter_mut() {
            bridge.step();
        }
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Times and the ids of the frames differ between two runs. Everything else must not.
fn without_times(text: &str) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        out.push_str(if digits.len() >= 9 { "T" } else { &digits });
        digits.clear();
        out.push(c);
    }
    out.push_str(if digits.len() >= 9 { "T" } else { &digits });
    out
}

const LIMIT: Duration = Duration::from_mins(1);

#[test]
fn a_strip_on_the_game_computer_comes_back_as_an_echo_from_the_server() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let game = game(root.path(), "game");
    let mut serve = server_half(&server);
    let mut remote = game_half(&game, &server);
    write_strip(&game, "WoWScrnShot_1.png", 7, "hi from the game");

    let answered = step_all(&mut [&mut remote, &mut serve], LIMIT, || {
        slot_file(&game, BODY_FILE).contains("echo: hi from the game")
    });

    assert!(answered, "{}", slot_file(&game, BODY_FILE));
    assert!(!game.screenshots.join("WoWScrnShot_1.png").exists());
    let status = read_status(&game.state).unwrap();
    assert!(status.connected, "{status:?}");
}

#[test]
fn the_slot_files_of_a_server_match_those_of_a_plain_desktop_app() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let split = game(root.path(), "split");
    let alone = game(root.path(), "alone");
    let mut serve = server_half(&server);
    let mut remote = game_half(&split, &server);
    let mut local = plain(&alone);
    let messages = [(7, "first"), (8, "second\twith a tab"), (9, "third ✓")];

    for (i, (id, text)) in messages.iter().enumerate() {
        let file = format!("WoWScrnShot_{i}.png");
        write_strip(&split, &file, *id, text);
        write_strip(&alone, &file, *id, text);
        // The slot file escapes a tab and every byte past ASCII, so the first word is enough.
        let first_word = text.split(['\t', ' ']).next().unwrap();
        let echo = format!("echo: {first_word}");
        let both = step_all(&mut [&mut remote, &mut serve, &mut local], LIMIT, || {
            slot_file(&split, BODY_FILE).contains(&echo)
                && slot_file(&alone, BODY_FILE).contains(&echo)
        });
        assert!(both, "{}", slot_file(&split, BODY_FILE));
    }
    // One more step on each side, so both write the last state.
    step_all(
        &mut [&mut remote, &mut serve, &mut local],
        Duration::from_secs(2),
        || false,
    );

    for file in [BODY_FILE, RESTORE_FILE, LIVE_FILE] {
        assert_eq!(
            without_times(&slot_file(&split, file)),
            without_times(&slot_file(&alone, file)),
            "{file}"
        );
    }
}

#[test]
fn a_desktop_request_on_the_server_is_answered_on_the_game_computer() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let game = game(root.path(), "game");
    let mut serve = server_half(&server);
    let mut remote = game_half(&game, &server);
    let connected = step_all(&mut [&mut remote, &mut serve], LIMIT, || {
        read_status(&game.state).is_some_and(|s| s.connected)
    });
    assert!(connected);
    let on_server = Approvals::new(&server.data, Prompt::Off);
    let opened = on_server
        .open("claude", "/srv/code", "cargo test", "", now())
        .unwrap();
    let on_game = Approvals::new(&game.state, Prompt::Off);

    let copied = step_all(&mut [&mut remote, &mut serve], LIMIT, || {
        !on_game.list().is_empty()
    });
    assert!(copied);
    let copy = on_game.list().remove(0);
    assert_eq!(copy.id, opened.id);
    assert_eq!(copy.server.as_deref(), Some("test-server"));
    on_game.answer(&copy.id, Verdict::Approve).unwrap();

    let answered = step_all(&mut [&mut remote, &mut serve], LIMIT, || {
        on_server.answer_of(&opened.id).is_some()
    });
    assert!(answered);
    assert_eq!(on_server.answer_of(&opened.id), Some(Verdict::Approve));

    on_server.close(&opened.id);
    let closed = step_all(&mut [&mut remote, &mut serve], LIMIT, || {
        on_game.list().is_empty()
    });
    assert!(closed, "a closed request removes its copy");
}

#[test]
fn a_message_sent_while_the_server_is_down_runs_once_it_is_back() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let game = game(root.path(), "game");
    let mut remote = game_half(&game, &server);
    write_strip(&game, "WoWScrnShot_1.png", 7, "while down");
    let read = step_all(&mut [&mut remote], LIMIT, || {
        !game.screenshots.join("WoWScrnShot_1.png").exists()
    });
    assert!(read);

    let mut serve = server_half(&server);
    let answered = step_all(&mut [&mut remote, &mut serve], LIMIT * 2, || {
        slot_file(&game, BODY_FILE).contains("echo: while down")
    });

    assert!(answered, "{}", slot_file(&game, BODY_FILE));
}

/// The peer check of `serve`: a command of a run never takes the link.
#[cfg(target_os = "linux")]
#[test]
fn attach_from_a_child_of_serve_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let game = game(root.path(), "game");
    let mut serve = server_half(&server);
    let mut command = vec!["env".to_owned()];
    command.extend(
        server
            .env
            .iter()
            .map(|(name, path)| format!("{name}={}", path.display())),
    );
    command.extend([BIN.to_owned(), "attach".to_owned()]);
    let remote = Remote {
        host: "test-server".into(),
        command,
    };
    let paths = paths(&game);
    let lane = RemoteLane::new(&remote, &paths.games, &paths.state, Prompt::Off);
    let mut bridge = Bridge::without_relay(paths, relay_keys())
        .unwrap()
        .with_remote(lane);

    let refused = step_all(&mut [&mut bridge, &mut serve], LIMIT, || {
        read_status(&game.state).is_some_and(|s| s.problem.is_some())
    });

    assert!(refused);
    let status = read_status(&game.state).unwrap();
    assert!(!status.connected);
    assert_eq!(
        status.problem.as_deref(),
        Some("a run of an agent on this computer can't take the link")
    );
}

#[test]
fn a_failed_connect_names_the_server_and_the_last_lines_of_ssh() {
    let root = tempfile::tempdir().unwrap();
    let game = game(root.path(), "game");
    let remote = Remote {
        host: "me@box".into(),
        command: vec![
            "sh".into(),
            "-c".into(),
            "echo 'Permission denied (publickey).' >&2; exit 255".into(),
        ],
    };
    let paths = paths(&game);
    let lane = RemoteLane::new(&remote, &paths.games, &paths.state, Prompt::Off);
    let mut bridge = Bridge::without_relay(paths, relay_keys())
        .unwrap()
        .with_remote(lane);

    let failed = step_all(&mut [&mut bridge], LIMIT, || {
        read_status(&game.state).is_some_and(|s| s.problem.is_some())
    });

    assert!(failed);
    let status = read_status(&game.state).unwrap().line();
    assert!(
        status.starts_with("Server: can't reach me@box: Permission denied (publickey)."),
        "{status}"
    );
}

#[test]
fn banner_lines_before_the_hello_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let server = server(root.path());
    let game = game(root.path(), "game");
    let mut serve = server_half(&server);
    let env: Vec<String> = server
        .env
        .iter()
        .map(|(name, path)| format!("{name}={}", path.display()))
        .collect();
    let script = format!(
        "echo 'Welcome to box!'; echo 'Last login: today'; exec {} env {} {BIN} attach",
        detached().join(" "),
        env.join(" ")
    );
    let remote = Remote {
        host: "test-server".into(),
        command: vec!["sh".into(), "-c".into(), script],
    };
    let paths = paths(&game);
    let lane = RemoteLane::new(&remote, &paths.games, &paths.state, Prompt::Off);
    let mut bridge = Bridge::without_relay(paths, relay_keys())
        .unwrap()
        .with_remote(lane);

    let connected = step_all(&mut [&mut bridge, &mut serve], LIMIT, || {
        read_status(&game.state).is_some_and(|s| s.connected)
    });

    assert!(connected, "{:?}", read_status(&game.state));
}
