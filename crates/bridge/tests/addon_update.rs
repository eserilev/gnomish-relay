//! An addon app such as `CurseForge` replaces the whole `GnomishRelay` folder at each
//! update. The key addon and the slots live next to it, so the relay keeps working
//! (SPEC.md 7.3.2). The fake game loads the files from the disk, as WoW does.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;

use bridge::app_files::key_addon_name;
use bridge::install::{self, ADDON, ADDON_FILES, KEY_FILE};
use bridge::slots::{BODY_FILE, LIVE_FILE, RESTORE_FILE, publish_reply, slot_name};
use common::{fake_game, game_lua, install_window, measured, start_addon};
use mlua::{Lua, Table, Value};
use protocol::apps::App;
use protocol::slot::{Reply, Status};

const KEY_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// What the desktop app writes: the addon, the key addon, the slots, and one reply.
fn desktop_install(addons: &Path, now: u32) {
    install::install_relay(addons, KEY_HEX).unwrap();
    install_window(addons, App::Relay);
    let reply = Reply {
        chat: b"c1".to_vec(),
        id: 1,
        status: Status::Done,
        text: b"hello".to_vec(),
    };
    publish_reply(addons, reply, 1, now).unwrap();
}

/// An update of an addon app: the folder goes away, and the packaged files come back.
fn addon_app_update(addons: &Path) {
    let dir = addons.join(ADDON);
    fs::remove_dir_all(&dir).unwrap();
    fs::create_dir(&dir).unwrap();
    for (name, content) in ADDON_FILES {
        fs::write(dir.join(name), content).unwrap();
    }
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

    addon_app_update(&addons);
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
fn a_second_desktop_install_after_an_update_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("AddOns");
    fs::create_dir(&addons).unwrap();
    install::install_relay(&addons, KEY_HEX).unwrap();

    addon_app_update(&addons);

    assert_eq!(
        install::install_relay(&addons, KEY_HEX).unwrap(),
        install::Installed::Unchanged
    );
}

/// The zip for `CurseForge` holds the addon that the desktop app installs, and no key or slot.
#[cfg(unix)]
#[test]
fn the_curseforge_package_is_the_built_in_addon_and_nothing_else() {
    let out = tempfile::tempdir().unwrap();
    let script = common::repo_path("scripts/package-addon.sh");

    let made = std::process::Command::new("bash")
        .arg(script)
        .arg(out.path())
        .status()
        .unwrap();

    assert!(made.success());
    let dir = out.path().join(ADDON);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut built: Vec<String> = ADDON_FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
    built.push(".pkgmeta".to_owned());
    built.sort();
    assert_eq!(names, built);
    for (name, content) in ADDON_FILES {
        assert_eq!(fs::read(dir.join(name)).unwrap(), content, "{name}");
    }
    let toc = read(&dir, &format!("{ADDON}.toc"));
    assert!(toc.contains("\n## X-Curse-Project-ID: "), "{toc}");
}
