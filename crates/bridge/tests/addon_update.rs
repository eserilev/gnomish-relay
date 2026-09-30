//! Players get the `GnomishRelay` folder only from `CurseForge`, and its app replaces the
//! whole folder at each update. The key addon and the slots live next to it, so the relay
//! keeps working (SPEC.md 7.3.2 and 11.3). The fake game loads the files from the disk, as
//! WoW does.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;

use bridge::app_files::key_addon_name;
use bridge::install::{self, ADDON, KEY_FILE};
use bridge::slots::{BODY_FILE, LIVE_FILE, RESTORE_FILE, publish_reply, slot_name};
use common::{addon_file, fake_game, game_lua, install_window, measured, repo_file, start_addon};
use mlua::{Lua, Table, Value};
use protocol::apps::App;
use protocol::slot::{Reply, Status};

const KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// What the desktop app writes: the key addon, the slots, and one reply.
fn desktop_install(addons: &Path, now: u32) {
    install::write_relay_keys(addons, KEY_HEX).unwrap();
    install_window(addons, App::Relay);
    let reply = Reply {
        chat: b"c1".to_vec(),
        id: 1,
        status: Status::Done,
        text: b"hello".to_vec(),
    };
    publish_reply(addons, reply, 1, now).unwrap();
}

const TOC: &str = "GnomishRelay.toc";

/// The files of the addon that its TOC does not list: WoW reads `Bindings.xml` by itself,
/// the transcript loads the font, and the font license goes with the font.
const UNLISTED_FILES: [&str; 3] = [
    "Bindings.xml",
    "JetBrainsMono-Regular.ttf",
    "JetBrainsMono-OFL.txt",
];

fn toc_files() -> Vec<String> {
    let toc = repo_file(&format!("addon/{ADDON}/{TOC}"));
    let is_file = |line: &&str| !line.is_empty() && !line.starts_with('#');
    toc.lines().filter(is_file).map(str::to_owned).collect()
}

/// An update of the `CurseForge` app: the folder goes away, and the files of the TOC come back.
fn curseforge_update(addons: &Path) {
    let dir = addons.join(ADDON);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join(TOC), repo_file(&format!("addon/{ADDON}/{TOC}"))).unwrap();
    for name in toc_files() {
        fs::write(dir.join(&name), addon_file(ADDON, &name)).unwrap();
    }
}

fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

fn read(dir: &Path, file: &str) -> String {
    fs::read_to_string(dir.join(file)).unwrap()
}

/// The fake game gets the key addon and slot 1 from the disk.
fn give_disk_to_game(wow: &Table, addons: &Path) {
    let key_addon = addons.join(key_addon_name(App::Relay));
    let keys: Table = wow.get("keyAddons").unwrap();
    keys.set(key_addon_name(App::Relay), read(&key_addon, KEY_FILE))
        .unwrap();
    let slot = addons.join(slot_name(App::Relay, 1));
    wow.set("body", read(&slot, BODY_FILE)).unwrap();
    wow.set("restore", read(&slot, RESTORE_FILE)).unwrap();
    wow.set("live", read(&slot, LIVE_FILE)).unwrap();
}

/// Runs each Lua file of the TOC on the disk, in its order.
fn load_addon_from_disk(lua: &Lua, ns: &Table, dir: &Path) {
    let toc = read(dir, &format!("{ADDON}.toc"));
    let is_lua = |line: &&str| Path::new(line).extension().is_some_and(|e| e == "lua");
    for file in toc.lines().filter(is_lua) {
        lua.load(read(dir, file))
            .set_name(file)
            .call::<()>((ADDON, ns.clone()))
            .unwrap();
    }
}

fn start_game_from_disk(addons: &Path, now: u32) -> (Lua, Table) {
    let fake = measured();
    let lua = game_lua();
    let wow = fake_game(&lua);
    wow.set("epoch", now).unwrap();
    give_disk_to_game(&wow, addons);
    let ns = lua.create_table().unwrap();
    start_addon(&lua, &wow, &fake, ADDON, None, || {
        load_addon_from_disk(&lua, &ns, &addons.join(ADDON));
    });
    (lua, ns)
}

#[test]
fn an_update_that_replaces_the_addon_folder_keeps_the_key_and_the_slots() {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("AddOns");
    fs::create_dir(&addons).unwrap();
    let now = 1_790_211_079;
    desktop_install(&addons, now);

    curseforge_update(&addons);
    let (lua, ns) = start_game_from_disk(&addons, now);
    let online = || -> bool {
        lua.load("local ns = ... return ns.Transport.Online()")
            .call(&ns)
            .unwrap()
    };
    let before = online();
    lua.load("local ns = ... ns.Transport.Poll()")
        .call::<()>(&ns)
        .unwrap();

    let key: mlua::String = ns.get("key").unwrap();
    assert_eq!(bridge::ids::hex(&key.as_bytes()), KEY_HEX);
    assert!(!before);
    assert!(online(), "slot 1 loaded, and its body is fresh");
    let global: Value = lua.globals().get("GnomishRelayKey").unwrap();
    assert!(global.is_nil());
}

#[test]
fn a_desktop_install_after_a_curseforge_update_changes_no_file_of_the_addon() {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("AddOns");
    fs::create_dir(&addons).unwrap();
    install::write_relay_keys(&addons, KEY_HEX).unwrap();
    curseforge_update(&addons);
    let before = files_in(&addons.join(ADDON));

    let again = install::write_relay_keys(&addons, KEY_HEX).unwrap();

    assert_eq!(again, install::Installed::Unchanged);
    assert_eq!(files_in(&addons.join(ADDON)), before);
}

/// The zip for `CurseForge` holds what the addon needs, and no key, slot, or self-test.
#[cfg(unix)]
#[test]
fn the_curseforge_package_holds_exactly_the_files_that_the_addon_needs() {
    let out = tempfile::tempdir().unwrap();
    let script = common::repo_path("scripts/package-addon.sh");

    let made = std::process::Command::new("bash")
        .arg(script)
        .arg(out.path())
        .status()
        .unwrap();

    assert!(made.success());
    let dir = out.path().join(ADDON);
    let mut needed = toc_files();
    needed.extend(UNLISTED_FILES.map(str::to_owned));
    needed.extend([TOC.to_owned(), ".pkgmeta".to_owned()]);
    needed.sort();
    let packaged = files_in(&dir);
    let names: Vec<&str> = packaged.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, needed);
    for (name, content) in &packaged {
        let shared = common::repo_path(&format!("addon/transport/{name}"));
        let source = if shared.exists() {
            shared
        } else {
            common::repo_path(&format!("addon/{ADDON}/{name}"))
        };
        assert_eq!(content, &fs::read(source).unwrap(), "{name}");
    }
    let toc = read(&dir, TOC);
    assert!(toc.contains("\n## X-Curse-Project-ID: "), "{toc}");
    assert!(!toc.lines().any(|line| line == KEY_FILE), "{toc}");
}
