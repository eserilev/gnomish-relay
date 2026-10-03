//! The bridge serves two games at once, such as WoW: Forever and TBC Anniversary
//! (SPEC.md 7.9): a strip from either game, the slots of both, and an account name that
//! both games have.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bridge::agent::{Agent, Echo};
use bridge::config::{Permission, Policy};
use bridge::folder_path::path_bytes;
use bridge::game_folders::GameFolders;
use bridge::ids::hex;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{BODY_FILE, slot_name};
use common::{install_window, screenshot_png, signed_frame, strip_rows};
use protocol::apps::App;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

struct TwoGames {
    _root: tempfile::TempDir,
    forever: GameFolders,
    anniversary: GameFolders,
    state: std::path::PathBuf,
}

fn game(root: &Path, name: &str) -> GameFolders {
    let game = GameFolders::of(&root.join(name));
    for dir in [&game.addons, &game.screenshots, &game.accounts] {
        fs::create_dir_all(dir).unwrap();
    }
    install_window(&game.addons, App::Relay);
    game
}

fn two_games() -> TwoGames {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("data");
    fs::create_dir_all(&state).unwrap();
    TwoGames {
        forever: game(root.path(), "_classic_beta_"),
        anniversary: game(root.path(), "_anniversary_"),
        state,
        _root: root,
    }
}

fn bridge(g: &TwoGames) -> Bridge {
    let base = path_bytes(&std::env::temp_dir().canonicalize().unwrap());
    let policy = Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    };
    let paths = Paths {
        games: vec![g.forever.clone(), g.anniversary.clone()],
        state: g.state.clone(),
        config: g.state.join("config"),
    };
    let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
    let agents = [("claude".to_owned(), Arc::new(Echo) as Arc<dyn Agent>)].into();
    Bridge::new(paths, policy, keys, agents).unwrap()
}

fn strip(token: &str, chat: &str, id: u32, flags: &str, text: &str) -> Vec<u8> {
    let payload = format!("{token}\x1f{chat}\x1f{id}\x1f\x1f{flags}\x1f\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), KEY)))
}

/// The saved variables of `account` in `game` hold `token`, written `later_secs` from now.
fn write_account(game: &GameFolders, account: &str, token: &str, later_secs: u64) {
    let dir = game.accounts.join(account).join("SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("GnomishRelay.lua");
    fs::write(
        &file,
        format!("GnomishRelayDB = {{\n\t[\"token\"] = \"{token}\",\n}}\n"),
    )
    .unwrap();
    let time = std::time::SystemTime::now() + Duration::from_secs(later_secs);
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn body(game: &GameFolders) -> String {
    fs::read_to_string(game.addons.join(slot_name(App::Relay, 1)).join(BODY_FILE))
        .unwrap_or_default()
}

/// A publish syncs 60 files, and on the Windows runner of CI one publish took 14 seconds.
fn step_until(bridge: &mut Bridge, done: impl Fn() -> bool) -> bool {
    common::step_until_within(bridge, Duration::from_secs(90), done)
}

#[test]
fn a_strip_in_the_second_game_comes_back_in_the_slots_of_both_games() {
    let g = two_games();
    let mut bridge = bridge(&g);
    let shot = g.anniversary.screenshots.join("WoWScrnShot_1.png");
    fs::write(&shot, strip("tbc", "c1", 7, "", "hi from tbc")).unwrap();

    let answered = step_until(&mut bridge, || {
        body(&g.anniversary).contains("echo: hi from tbc")
    });

    assert!(answered, "{}", body(&g.anniversary));
    assert!(body(&g.forever).contains("echo: hi from tbc"));
    assert!(!shot.exists(), "a valid strip is deleted");
}

#[test]
fn the_same_account_name_in_two_games_keeps_the_chats_of_both() {
    let g = two_games();
    write_account(&g.forever, "ACCOUNT1", "forever", 0);
    let mut bridge = bridge(&g);
    let shots = [
        (&g.forever, strip("forever", "c1", 7, "", "from forever")),
        (&g.anniversary, strip("tbc", "c2", 8, "", "from tbc")),
    ];
    for (n, (game, shot)) in shots.iter().enumerate() {
        fs::write(game.screenshots.join(format!("WoWScrnShot_{n}.png")), shot).unwrap();
        bridge.step();
    }
    assert!(step_until(&mut bridge, || {
        let both = body(&g.forever);
        both.contains("echo: from forever") && both.contains("echo: from tbc")
    }));

    write_account(&g.anniversary, "ACCOUNT1", "tbc", 60);
    bridge.step();
    bridge.step();

    assert!(
        body(&g.forever).contains("echo: from forever"),
        "a token of another game is not a wipe"
    );
}
