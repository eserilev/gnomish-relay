//! The shared Lua transport (`addon/transport`) with the names of each app, and two
//! addons in one fake game (SPEC.md 9.7, decisions 5 and 14). The second addon is a
//! small test addon, not the real Timeways addon.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use bridge::receive::{KeySet, StripKey, receive};
use bridge::run::{Bridge, now};
use bridge::slots::{self, BODY_FILE, Files, LIVE_FILE, RESTORE_FILE};
use bridge::story::{STORY_DIR, StorySpec};
use bridge::story_sandbox::{Sandbox, Walls};
use bridge::strip::{Image, read_with};
use bridge::timeways::NO_STORY;
use common::{Bits, hex, load_addon, lua, repo_file, screenshot_png};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::live::{Progress, live_body, prepare_progress};
use protocol::record::Record;
use protocol::restore::{Chat, Entry, Role, prepare_restore, restore_body};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const RELAY_KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const TIMEWAYS_KEY: &[u8] = b"fedcba9876543210fedcba9876543210";
const SHARED: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Strip.lua",
    "Slots.lua",
];
const RELAY_FILES: &[&str] = &[
    "App.lua",
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Store.lua",
    "Health.lua",
    "Strip.lua",
    "Slots.lua",
    "Transport.lua",
    "Blocks.lua",
    "Transcript.lua",
    "Window.lua",
    "Popup.lua",
    "Core.lua",
];
/// The App.lua of the test addon: the Timeways names of SPEC.md 9.7, decision 5.
const TIMEWAYS_APP: &str = r#"
local _, ns = ...
ns.App = {
	slotPrefix = "Timeways_S%04d",
	slotData = "Timeways_SlotData",
	restore = "Timeways_Restore",
	live = "Timeways_Live",
	strip = "TimewaysStrip",
	saved = "TimewaysDB",
}
"#;
/// The same names as the App.lua of the relay addon.
const RELAY_APP: &str = r#"
local _, ns = ...
ns.App = {
	slotPrefix = "GnomishRelay_S%04d",
	slotData = "GnomishRelay_SlotData",
	restore = "GnomishRelay_Restore",
	live = "GnomishRelay_Live",
	strip = "GnomishRelayStrip",
	saved = "GnomishRelayDB",
}
"#;

/// The parameters of one app for the shared transport.
struct Names {
    app: App,
    addon: &'static str,
    app_file: &'static str,
    strip: &'static str,
    key: &'static [u8],
}

const RELAY: Names = Names {
    app: App::Relay,
    addon: "GnomishRelay",
    app_file: RELAY_APP,
    strip: "GnomishRelayStrip",
    key: RELAY_KEY,
};
const TIMEWAYS: Names = Names {
    app: App::Timeways,
    addon: "Timeways",
    app_file: TIMEWAYS_APP,
    strip: "TimewaysStrip",
    key: TIMEWAYS_KEY,
};

struct Game {
    lua: Lua,
    wow: Table,
}

impl Game {
    /// A fake game with no addon yet. Screenshots read the strip of both apps.
    fn new() -> Game {
        let lua = lua(Bits::Unsigned);
        let api: Table = lua.load(repo_file("addon/tests/api.lua")).call(()).unwrap();
        let wow: Table = lua
            .load(repo_file("addon/tests/wow.lua"))
            .call(api)
            .unwrap();
        let strips = lua
            .create_sequence_from([RELAY.strip, TIMEWAYS.strip])
            .unwrap();
        wow.set("strips", strips).unwrap();
        Game { lua, wow }
    }

    /// Loads only the shared transport with the names of `names`.
    fn shared(&self, names: &Names) -> Table {
        let ns = self.lua.create_table().unwrap();
        ns.set("key", self.lua.create_string(names.key).unwrap())
            .unwrap();
        self.lua
            .load(names.app_file)
            .call::<()>((names.addon, ns.clone()))
            .unwrap();
        load_addon(&self.lua, names.addon, &ns, SHARED);
        ns
    }

    /// Loads the whole relay addon and logs in.
    fn relay(&self) -> Table {
        let ns = self.lua.create_table().unwrap();
        ns.set("key", self.lua.create_string(RELAY_KEY).unwrap())
            .unwrap();
        load_addon(&self.lua, "GnomishRelay", &ns, RELAY_FILES);
        self.fire("ADDON_LOADED", "GnomishRelay");
        self.fire("PLAYER_LOGIN", ());
        ns
    }

    fn fire(&self, event: &str, args: impl mlua::IntoLuaMulti) {
        let fire: Function = self.wow.get("Fire").unwrap();
        let mut all = vec![Value::String(self.lua.create_string(event).unwrap())];
        all.extend(args.into_lua_multi(&self.lua).unwrap());
        fire.call::<()>(mlua::MultiValue::from_vec(all)).unwrap();
    }

    fn advance(&self, seconds: f64) {
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(seconds).unwrap();
    }

    /// Sets the Unix time of the game, so the bridge finds its frames fresh.
    fn set_time(&self, unix: u32) {
        self.wow.set("epoch", unix).unwrap();
    }

    fn now(&self) -> u32 {
        self.lua.load("return time()").eval().unwrap()
    }

    fn global(&self, name: &str) -> Value {
        self.lua.globals().get(name).unwrap()
    }

    /// Signs one record with the key of the addon and shows it as a strip.
    fn show(&self, ns: &Table, text: &str) {
        let show: Function = self
            .lua
            .load(
                r#"local ns, text = ...
                local record = { token = "tok", chat = "c1", id = 7, flags = "", text = text }
                local frame = ns.Codec.Frame(time(), 7, ns.Codec.Payload({ record }), ns.key)
                assert(ns.Strip.Show(frame, function() end))"#,
            )
            .into_function()
            .unwrap();
        show.call::<()>((ns.clone(), text)).unwrap();
        self.advance(1.0);
    }

    /// The cells of each screenshot of one strip frame, calibration rows first.
    fn shots(&self, strip: &str) -> Vec<Vec<Vec<u8>>> {
        let shots: Table = self
            .wow
            .get::<Table>("shotsOf")
            .unwrap()
            .get(strip)
            .unwrap();
        (1..=shots.raw_len())
            .map(|n| {
                let rows: Table = shots.get(n).unwrap();
                (1..=rows.raw_len())
                    .map(|row| rows.get(row).unwrap())
                    .collect()
            })
            .collect()
    }

    fn last_shot(&self, strip: &str) -> Vec<Vec<u8>> {
        self.shots(strip).pop().unwrap()
    }

    /// Puts the slot files of `app` where its slot addons read them.
    fn put_slot_files(&self, app: App, body: Vec<u8>, restore: Vec<u8>, live: Vec<u8>) {
        let files = self.lua.create_table().unwrap();
        files
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
        files
            .set("restore", self.lua.create_string(restore).unwrap())
            .unwrap();
        files
            .set("live", self.lua.create_string(live).unwrap())
            .unwrap();
        match app {
            App::Relay => {
                for key in ["body", "restore", "live"] {
                    self.wow.set(key, files.get::<Value>(key).unwrap()).unwrap();
                }
            }
            App::Timeways => {
                let all: Table = self.wow.get("files").unwrap();
                all.set("Timeways", files).unwrap();
            }
        }
    }
}

/// `Slots.Load(1)` of the addon: loaded, body, restore, and live.
fn load_slot(ns: &Table) -> (bool, Value, Value, Value) {
    let slots: Table = ns.get("Slots").unwrap();
    slots
        .get::<Function>("Load")
        .unwrap()
        .call::<(bool, Value, Value, Value)>(1)
        .unwrap()
}

fn key(bytes: &[u8]) -> StripKey {
    StripKey::from_hex(&hex(bytes)).unwrap()
}

/// The strip goes through a PNG as WoW writes it, then the bridge reader and `receive`.
fn receive_png(rows: &[Vec<u8>], key_bytes: &[u8], now: u32) -> Option<Vec<Record>> {
    let keys = KeySet::new(key(key_bytes), None).ok()?;
    let image = Image::from_png(&screenshot_png(rows)).ok()?;
    let bytes = read_with(&image, |bytes| receive(bytes, &keys, now).is_ok())?;
    receive(&bytes, &keys, now).ok().map(|(_, records)| records)
}

fn text_of(value: &Value) -> Vec<u8> {
    let Value::Table(data) = value else {
        panic!("no table: {value:?}");
    };
    let reply: Table = data.get::<Table>("replies").unwrap().get(1).unwrap();
    reply
        .get::<mlua::String>("text")
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn body(app: App, text: &[u8]) -> Vec<u8> {
    let reply = Reply {
        chat: b"c1".to_vec(),
        id: 7,
        status: Status::Done,
        text: text.to_vec(),
    };
    slot_body(app, 1_790_211_079, &prepare_replies(&[reply]))
}

fn restore(app: App, token: &[u8], text: &[u8]) -> Vec<u8> {
    let chat = Chat {
        id: b"c1".to_vec(),
        name: b"name".to_vec(),
        agent: b"claude".to_vec(),
        cwd: Vec::new(),
        history: vec![Entry {
            role: Role::User,
            id: 7,
            text: text.to_vec(),
        }],
    };
    restore_body(app, token, &prepare_restore(&[chat]))
}

fn live(app: App, line: &[u8]) -> Vec<u8> {
    let progress = Progress {
        chat: b"c1".to_vec(),
        id: 7,
        lines: vec![line.to_vec()],
    };
    live_body(app, &prepare_progress(&[progress]), &[])
}

/// Bytes that stress the Lua literals: quotes, a fake end of the table, and bad UTF-8.
const HOSTILE: &[u8] = b"\"}} Timeways_SlotData = nil GnomishRelay_SlotData = 1 --\n\xff\x00]]";

#[test]
fn the_lua_strip_of_each_app_reads_back_in_rust_through_a_png() {
    for names in [RELAY, TIMEWAYS] {
        let game = Game::new();
        let ns = game.shared(&names);
        for text in ["", "hi", &"x".repeat(3000)] {
            game.show(&ns, text);
            let records = receive_png(&game.last_shot(names.strip), names.key, game.now())
                .unwrap_or_else(|| panic!("{} strip of {} bytes", names.addon, text.len()));
            assert_eq!(records[0].text, text.as_bytes());
        }
    }
}

#[test]
fn a_strip_of_one_app_fails_the_tag_under_the_key_of_the_other() {
    let game = Game::new();
    let ns = game.shared(&TIMEWAYS);
    game.show(&ns, "story");
    let rows = game.last_shot(TIMEWAYS.strip);
    assert!(receive_png(&rows, RELAY_KEY, game.now()).is_none());
    assert!(receive_png(&rows, TIMEWAYS_KEY, game.now()).is_some());
}

#[test]
fn the_rust_slot_files_of_each_app_read_back_through_the_lua_poll() {
    for names in [RELAY, TIMEWAYS] {
        let game = Game::new();
        let ns = game.shared(&names);
        game.put_slot_files(
            names.app,
            body(names.app, HOSTILE),
            restore(names.app, b"tok", HOSTILE),
            live(names.app, HOSTILE),
        );

        let (loaded, data, restore, live) = load_slot(&ns);

        assert!(loaded);
        assert_eq!(text_of(&data), HOSTILE, "{}", names.addon);
        let Value::Table(restore) = restore else {
            panic!("no restore of {}", names.addon);
        };
        assert_eq!(restore.get::<String>("token").unwrap(), "tok");
        let Value::Table(live) = live else {
            panic!("no live file of {}", names.addon);
        };
        let entry: Table = live.get::<Table>("progress").unwrap().get(1).unwrap();
        let step: mlua::String = entry.get::<Table>("lines").unwrap().get(1).unwrap();
        assert_eq!(&*step.as_bytes(), HOSTILE);
    }
}

#[test]
fn two_addons_in_one_game_draw_separate_strip_frames() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.shared(&TIMEWAYS);

    game.show(&timeways, "from timeways");
    let ours = receive_png(&game.last_shot(TIMEWAYS.strip), TIMEWAYS_KEY, game.now()).unwrap();
    let send: Function = relay.get::<Table>("Window").unwrap().get("Send").unwrap();
    send.call::<()>("from the relay").unwrap();
    game.advance(5.0);
    let theirs: Vec<Record> = game
        .shots(RELAY.strip)
        .iter()
        .filter_map(|rows| receive_png(rows, RELAY_KEY, game.now()))
        .flatten()
        .collect();

    assert_eq!(ours[0].text, b"from timeways");
    assert!(theirs.iter().any(|r| r.text == b"from the relay"));
    let (Value::Table(a), Value::Table(b)) =
        (game.global(RELAY.strip), game.global(TIMEWAYS.strip))
    else {
        panic!("each strip frame has its global name");
    };
    assert_ne!(a, b);
}

#[test]
fn two_addons_in_one_game_load_their_own_slots_and_globals() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.shared(&TIMEWAYS);
    game.put_slot_files(
        App::Relay,
        body(App::Relay, b"relay reply"),
        restore(App::Relay, b"", b""),
        live(App::Relay, b"relay step"),
    );
    game.put_slot_files(
        App::Timeways,
        body(App::Timeways, b"story reply"),
        restore(App::Timeways, b"tw", b"story"),
        live(App::Timeways, b"story step"),
    );

    let (loaded, data, restore, _) = load_slot(&timeways);
    let transport: Table = relay.get("Transport").unwrap();
    transport
        .get::<Function>("Poll")
        .unwrap()
        .call::<()>(())
        .unwrap();

    assert!(loaded);
    assert_eq!(text_of(&data), b"story reply");
    let Value::Table(restore) = restore else {
        panic!("no restore");
    };
    assert_eq!(restore.get::<String>("token").unwrap(), "tw");
    let loaded: Table = game.wow.get("loaded").unwrap();
    assert!(loaded.get::<bool>("Timeways_S0001").unwrap());
    assert!(loaded.get::<bool>("GnomishRelay_S0001").unwrap());
    let stats: Table = transport
        .get::<Function>("Stats")
        .unwrap()
        .call(())
        .unwrap();
    assert_eq!(stats.get::<u32>("nextSlot").unwrap(), 2);
    for global in [
        "GnomishRelay_SlotData",
        "GnomishRelay_Restore",
        "GnomishRelay_Live",
        "Timeways_SlotData",
        "Timeways_Restore",
        "Timeways_Live",
    ] {
        assert!(game.global(global).is_nil(), "{global} is left set");
    }
}

#[test]
fn a_slot_load_of_one_app_never_touches_the_globals_of_the_other() {
    let game = Game::new();
    let relay = game.shared(&RELAY);
    let timeways = game.shared(&TIMEWAYS);
    game.put_slot_files(
        App::Relay,
        body(App::Relay, b"relay"),
        restore(App::Relay, b"", b""),
        live(App::Relay, b""),
    );
    game.put_slot_files(
        App::Timeways,
        body(App::Timeways, b"story"),
        restore(App::Timeways, b"", b""),
        live(App::Timeways, b""),
    );
    game.lua
        .load("GnomishRelay_SlotData = 'left'; Timeways_Live = 'left'")
        .exec()
        .unwrap();

    let (_, story, _, _) = load_slot(&timeways);
    assert_eq!(text_of(&story), b"story");
    assert_eq!(
        game.global("GnomishRelay_SlotData").as_str().unwrap(),
        "left"
    );

    game.lua.load("Timeways_SlotData = 'left'").exec().unwrap();
    let (_, data, _, _) = load_slot(&relay);
    assert_eq!(text_of(&data), b"relay");
    assert_eq!(game.global("Timeways_SlotData").as_str().unwrap(), "left");
}

#[test]
fn two_addons_in_one_game_keep_separate_saved_variables() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.shared(&TIMEWAYS);

    let saved: Function = timeways.get("Saved").unwrap();
    saved.call::<Table>(()).unwrap().set("token", "tw").unwrap();

    let relay_db: Table = relay.get::<Table>("Store").unwrap().get("db").unwrap();
    let timeways_db: Table = game.lua.globals().get("TimewaysDB").unwrap();
    assert_eq!(timeways_db.get::<String>("token").unwrap(), "tw");
    assert_ne!(relay_db.get::<String>("token").unwrap(), "tw");
    assert_eq!(
        relay_db,
        game.lua.globals().get::<Table>("GnomishRelayDB").unwrap()
    );
    assert!(timeways_db.get::<Value>("chats").unwrap().is_nil());
}

/// Folders of a game and of the bridge, with the slots of both apps.
fn bridge_folders() -> (tempfile::TempDir, bridge::run::Paths) {
    let root = tempfile::tempdir().unwrap();
    let paths = bridge::run::Paths {
        addons: root.path().join("Interface/AddOns"),
        screenshots: root.path().join("Screenshots"),
        accounts: root.path().join("WTF/Account"),
        state: root.path().join("data"),
    };
    for dir in [
        &paths.addons,
        &paths.screenshots,
        &paths.accounts,
        &paths.state,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for app in [App::Relay, App::Timeways] {
        slots::install(&paths.addons, app, &Files::empty(app, 0)).unwrap();
    }
    (root, paths)
}

fn slot_bytes(addons: &std::path::Path, file: &str) -> Vec<u8> {
    std::fs::read(addons.join(slots::slot_name(App::Timeways, 1)).join(file)).unwrap()
}

#[test]
fn a_lua_timeways_strip_comes_back_as_the_fixed_reply_in_a_timeways_slot() {
    let (_root, paths) = bridge_folders();
    let addons = paths.addons.clone();
    let screenshot = paths.screenshots.join("WoWScrnShot_1.png");
    let keys = KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap();
    let policy = bridge::config::Policy {
        folders: bridge::relay::Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: std::collections::BTreeMap::new(),
        default_agent: "claude".into(),
    };
    let mut bridge = Bridge::new(paths, policy, keys, std::collections::BTreeMap::new()).unwrap();
    let game = Game::new();
    game.set_time(now());
    let timeways = game.shared(&TIMEWAYS);
    game.show(&timeways, "open the portal");
    std::fs::write(&screenshot, screenshot_png(&game.last_shot(TIMEWAYS.strip))).unwrap();

    let start = std::time::Instant::now();
    while !String::from_utf8_lossy(&slot_bytes(&addons, BODY_FILE)).contains(NO_STORY) {
        assert!(
            start.elapsed().as_secs() < 30,
            "no reply in the Timeways slot"
        );
        bridge.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    game.put_slot_files(
        App::Timeways,
        slot_bytes(&addons, BODY_FILE),
        slot_bytes(&addons, RESTORE_FILE),
        slot_bytes(&addons, LIVE_FILE),
    );
    let (loaded, data, _, _) = load_slot(&timeways);

    assert!(loaded);
    assert_eq!(text_of(&data), NO_STORY.as_bytes());
    assert!(!screenshot.exists());
}

/// The loopback of SPEC.md 9.7, step 5: the Lua strip, the bridge, the fake story
/// program, and the Lua slot poll.
#[test]
fn a_lua_timeways_strip_comes_back_from_the_fake_story_program_through_the_slot_poll() {
    let (_root, paths) = bridge_folders();
    let addons = paths.addons.clone();
    let screenshot = paths.screenshots.join("WoWScrnShot_1.png");
    let story = StorySpec {
        program: env!("CARGO_BIN_EXE_fake-story").into(),
        args: vec!["echo".into()],
        walls: Walls {
            folder: paths.state.join("timeways").join(STORY_DIR),
            hidden: Vec::new(),
            readable: Vec::new(),
        },
        sandbox: Sandbox::None,
        timeout: std::time::Duration::from_secs(20),
    };
    let keys = KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap();
    let policy = bridge::config::Policy {
        folders: bridge::relay::Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: std::collections::BTreeMap::new(),
        default_agent: "claude".into(),
    };
    let mut bridge = Bridge::new(paths, policy, keys, std::collections::BTreeMap::new())
        .unwrap()
        .with_story(story);
    let game = Game::new();
    game.set_time(now());
    let timeways = game.shared(&TIMEWAYS);
    let batch = "{\"type\":\"character_entered\",\"realm\":\"Stormrage\",\"name\":\"Anduin\"}\n\
                 {\"type\":\"zone_entered\",\"at\":1,\"zone\":\"Elwynn Forest\"}\n\
                 {\"type\":\"lore_asked\",\"at\":2,\"question\":\"open the portal\"}";
    game.show(&timeways, batch);
    std::fs::write(&screenshot, screenshot_png(&game.last_shot(TIMEWAYS.strip))).unwrap();

    let start = std::time::Instant::now();
    while !String::from_utf8_lossy(&slot_bytes(&addons, BODY_FILE)).contains("story: open") {
        assert!(
            start.elapsed().as_secs() < 30,
            "no story in the Timeways slot"
        );
        bridge.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    game.put_slot_files(
        App::Timeways,
        slot_bytes(&addons, BODY_FILE),
        slot_bytes(&addons, RESTORE_FILE),
        slot_bytes(&addons, LIVE_FILE),
    );
    let (loaded, data, _, _) = load_slot(&timeways);

    assert!(loaded);
    let reply: serde_json::Value = serde_json::from_slice(&text_of(&data)).unwrap();
    assert_eq!(reply["type"], "lore_answer");
    assert_eq!(reply["text"], "story: open the portal");
    assert!(!screenshot.exists());
}
