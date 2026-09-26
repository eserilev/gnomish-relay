//! The shared Lua transport (`addon/transport`) with the names of each app, and two
//! addons in one fake game (SPEC.md 9.7, decisions 5 and 14, and step 5b). The second
//! addon is a small test addon, not the real Timeways addon. It sends and polls through
//! `Messages.lua`, behind the seam of the Timeways addon.

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
    "Health.lua",
    "Strip.lua",
    "Slots.lua",
    "Messages.lua",
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
    "Messages.lua",
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
	title = "Timeways",
	version = 1,
	helloChat = "story",
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
	title = "Gnomish Relay",
	version = 1,
	helloChat = "relay",
	slotPrefix = "GnomishRelay_S%04d",
	slotData = "GnomishRelay_SlotData",
	restore = "GnomishRelay_Restore",
	live = "GnomishRelay_Live",
	strip = "GnomishRelayStrip",
	saved = "GnomishRelayDB",
}
"#;
/// The Link.lua of the test addon: the seam `ns.Link` of the Timeways addon, as a thin
/// wrapper of `Messages.lua`. It keeps each final reply in `ns.replies`.
const TIMEWAYS_LINK: &str = r#"
local _, ns = ...
local CHAT = { id = "story" }
ns.replies = {}
ns.Link = {
	Fits = function(text)
		return ns.Messages.Fits(CHAT, text)
	end,
	Send = function(text)
		return ns.Messages.Send(CHAT, text) ~= nil
	end,
}
ns.Messages.OnReply = function(_, _, status, text)
	table.insert(ns.replies, status .. ": " .. text)
end
ns.Messages.Init()
C_Timer.NewTicker(1, ns.Messages.Tick)
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

    /// Loads the test addon: the shared transport and its Link.lua, and logs in.
    fn timeways(&self) -> Table {
        let ns = self.shared(&TIMEWAYS);
        self.lua
            .load(TIMEWAYS_LINK)
            .call::<()>((TIMEWAYS.addon, ns.clone()))
            .unwrap();
        ns
    }

    /// `/reload`: WoW saves `TimewaysDB` as Lua text, and a new UI session loads it.
    /// The clock of the game goes on.
    fn reload(&self) -> Game {
        let save: Function = self.wow.get("Save").unwrap();
        let saved: String = save.call("TimewaysDB").unwrap();
        let game = Game::new();
        game.set_time(self.now());
        game.lua.load(saved).exec().unwrap();
        game
    }

    fn timeways_db(&self) -> Table {
        self.lua.globals().get("TimewaysDB").unwrap()
    }

    /// The id of the first message that the test addon sent.
    fn first_id(&self) -> u32 {
        let sent: Table = self.timeways_db().get("sent").unwrap();
        sent.get::<Table>(1).unwrap().get("id").unwrap()
    }

    /// The records of each screenshot of the strip of `names`, oldest first.
    fn strips(&self, names: &Names) -> Vec<Vec<Record>> {
        self.shots(names.strip)
            .iter()
            .map(|rows| receive_png(rows, names.key, self.now()).expect("a valid strip"))
            .collect()
    }

    /// How many strips of `names` carried a message with `text`.
    fn shows_of(&self, names: &Names, text: &[u8]) -> usize {
        let strips = self.strips(names);
        strips
            .iter()
            .filter(|records| records.iter().any(|r| r.text == text))
            .count()
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
        let shots: Option<Table> = self
            .wow
            .get::<Table>("shotsOf")
            .unwrap()
            .get(strip)
            .unwrap();
        let Some(shots) = shots else {
            return Vec::new();
        };
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

fn link_send(ns: &Table, text: &str) -> bool {
    let link: Table = ns.get("Link").unwrap();
    link.get::<Function>("Send").unwrap().call(text).unwrap()
}

fn replies(ns: &Table) -> Vec<String> {
    ns.get("replies").unwrap()
}

fn flags_of(record: &Record) -> Vec<String> {
    String::from_utf8_lossy(&record.flags)
        .split(';')
        .map(str::to_owned)
        .collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
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

fn reply_body(app: App, chat: &str, id: u32, text: &str) -> Vec<u8> {
    let reply = Reply {
        chat: chat.as_bytes().to_vec(),
        id,
        status: Status::Done,
        text: text.as_bytes().to_vec(),
    };
    slot_body(app, 1_790_211_079, &prepare_replies(&[reply]))
}

/// Puts a body with one done reply into the Timeways slots.
fn answer_story(game: &Game, id: u32, text: &str) {
    game.put_slot_files(
        App::Timeways,
        reply_body(App::Timeways, "story", id, text),
        restore(App::Timeways, b"", b""),
        live(App::Timeways, b""),
    );
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
    let timeways = game.timeways();
    assert!(link_send(&timeways, "open the portal"));
    game.advance(1.0);
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
    game.advance(5.0);

    assert_eq!(replies(&timeways), [format!("error: {NO_STORY}")]);
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
        model: bridge::model::ModelSpec::none(),
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
    let timeways = game.timeways();
    let batch = "{\"type\":\"character_entered\",\"realm\":\"Stormrage\",\"name\":\"Anduin\"}\n\
                 {\"type\":\"zone_entered\",\"at\":1,\"zone\":\"Elwynn Forest\"}\n\
                 {\"type\":\"lore_asked\",\"at\":2,\"question\":\"open the portal\"}";
    assert!(link_send(&timeways, batch));
    game.advance(1.0);
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
    game.advance(5.0);

    let got = replies(&timeways);
    assert_eq!(got.len(), 1);
    let json = got[0].strip_prefix("done: ").unwrap();
    let reply: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(reply["type"], "lore_answer");
    assert_eq!(reply["text"], "story: open the portal");
    assert!(!screenshot.exists());
}

#[test]
fn the_test_addon_sends_through_the_link_as_a_strip_with_only_transport_flags() {
    let game = Game::new();
    let timeways = game.timeways();

    assert!(link_send(&timeways, "look around"));
    game.advance(2.0);

    let records = game.strips(&TIMEWAYS).remove(0);
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.chat, b"story");
    assert_eq!(r.id, game.first_id());
    assert_eq!(r.text, b"look around");
    assert!(r.cwd.is_empty() && r.name.is_empty());
    let flags = flags_of(r);
    assert!(
        flags.contains(&"next=1".into()) && flags.contains(&"ver=1".into()),
        "{flags:?}"
    );
    assert!(
        flags
            .iter()
            .all(|f| f.starts_with("next=") || f.starts_with("build=") || f.starts_with("ver=")),
        "{flags:?}"
    );
}

/// SPEC.md 7.7: each app reports its own version, not one of the shared transport.
#[test]
fn each_app_reports_the_version_of_its_own_app_lua() {
    let game = Game::new();
    let timeways = game.timeways();
    let app: Table = timeways.get("App").unwrap();
    app.set("version", 7).unwrap();

    let health: Table = timeways.get("Health").unwrap();
    let flags: Vec<String> = health.get::<Function>("Flags").unwrap().call(()).unwrap();

    assert!(flags.contains(&"ver=7".into()), "{flags:?}");
}

#[test]
fn the_link_refuses_a_text_too_long_for_one_strip_and_stores_nothing() {
    let game = Game::new();
    let timeways = game.timeways();
    let link: Table = timeways.get("Link").unwrap();
    let long = "x".repeat(3000);

    let fits: bool = link
        .get::<Function>("Fits")
        .unwrap()
        .call(long.as_str())
        .unwrap();
    let sent = link_send(&timeways, &long);
    game.advance(2.0);

    assert!(!fits);
    assert!(!sent);
    assert_eq!(game.shows_of(&TIMEWAYS, long.as_bytes()), 0);
    let sent: Table = game.timeways_db().get("sent").unwrap();
    assert_eq!(sent.raw_len(), 0);
}

#[test]
fn a_message_of_the_test_addon_with_no_answer_shows_three_times_then_goes_to_the_outbox() {
    let game = Game::new();
    let timeways = game.timeways();

    link_send(&timeways, "look around");
    game.advance(200.0);

    assert_eq!(game.shows_of(&TIMEWAYS, b"look around"), 3);
    let outbox: Table = game.timeways_db().get("outbox").unwrap();
    assert_eq!(outbox.raw_len(), 1);
    let messages: Table = timeways.get("Messages").unwrap();
    assert!(
        messages
            .get::<Function>("NeedsReload")
            .unwrap()
            .call::<bool>(())
            .unwrap()
    );
    assert!(replies(&timeways).is_empty());
}

#[test]
fn an_outbox_frame_of_the_test_addon_that_the_bridge_never_takes_gives_up_through_on_reply() {
    let game = Game::new();
    let timeways = game.timeways();

    link_send(&timeways, "look around");
    game.advance(400.0);

    assert_eq!(replies(&timeways), ["error: Not sent. Send it again."]);
    let outbox: Table = game.timeways_db().get("outbox").unwrap();
    assert_eq!(outbox.raw_len(), 0);
}

#[test]
fn after_a_reload_the_outbox_frame_of_the_test_addon_verifies_as_timeways_and_its_reply_clears_it()
{
    let game = Game::new();
    link_send(&game.timeways(), "look around");
    game.advance(130.0);
    let id = game.first_id();

    let game = game.reload();
    let timeways = game.timeways();
    let outbox: Table = game.timeways_db().get("outbox").unwrap();
    let frame: String = outbox.get::<Table>(1).unwrap().get("frame").unwrap();
    let keys = KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap();
    let (app, records) = receive(&unhex(&frame), &keys, game.now()).unwrap();
    answer_story(&game, id, "answer");
    game.advance(6.0);

    assert_eq!(app, App::Timeways);
    assert_eq!(records[0].text, b"look around");
    assert_eq!(replies(&timeways), ["done: answer"]);
    let outbox: Table = game.timeways_db().get("outbox").unwrap();
    assert_eq!(outbox.raw_len(), 0);
}

#[test]
fn after_a_reload_the_test_addon_shows_the_stored_frame_of_an_open_message_as_it_is() {
    let game = Game::new();
    link_send(&game.timeways(), "look around");
    game.advance(2.0);

    let game = game.reload();
    game.timeways();
    game.advance(2.0);

    let sent: Table = game.timeways_db().get("sent").unwrap();
    let frame: String = sent.get::<Table>(1).unwrap().get("frame").unwrap();
    let keys = KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap();
    let (_, stored) = receive(&unhex(&frame), &keys, game.now()).unwrap();
    let first = game.strips(&TIMEWAYS).remove(0);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, stored[0].id);
    assert_eq!(first[0].text, b"look around");
    // A stored frame goes out as it is, so it carries no new report.
    assert_eq!(first[0].flags, stored[0].flags);
    assert!(first[0].flags.is_empty());
}

#[test]
fn a_reply_reaches_on_reply_once_and_the_next_strip_reports_it_read() {
    let game = Game::new();
    let timeways = game.timeways();
    link_send(&timeways, "look around");
    game.advance(1.0);
    let id = game.first_id();

    answer_story(&game, id, "answer");
    game.advance(30.0);
    link_send(&timeways, "next");
    game.advance(1.0);

    assert_eq!(replies(&timeways), ["done: answer"]);
    let last = game.strips(&TIMEWAYS).pop().unwrap();
    assert!(
        flags_of(&last[0]).contains(&format!("read={id}")),
        "{:?}",
        flags_of(&last[0])
    );
}

#[test]
fn two_addons_in_one_game_send_and_get_replies_through_their_own_messages() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();

    let send: Function = relay.get::<Table>("Window").unwrap().get("Send").unwrap();
    send.call::<()>("from the relay").unwrap();
    link_send(&timeways, "from timeways");
    game.advance(2.0);
    let relay_db: Table = game.lua.globals().get("GnomishRelayDB").unwrap();
    let chat: Table = relay_db.get::<Table>("chats").unwrap().get(1).unwrap();
    let chat_id: String = chat.get("id").unwrap();
    let history: Table = chat.get("history").unwrap();
    let relay_id: u32 = history.get::<Table>(1).unwrap().get("id").unwrap();
    game.put_slot_files(
        App::Relay,
        reply_body(App::Relay, &chat_id, relay_id, "relay answer"),
        restore(App::Relay, b"", b""),
        live(App::Relay, b""),
    );
    answer_story(&game, game.first_id(), "story answer");
    game.advance(6.0);

    assert!(game.shows_of(&RELAY, b"from the relay") > 0);
    assert!(game.shows_of(&TIMEWAYS, b"from timeways") > 0);
    assert_eq!(replies(&timeways), ["done: story answer"]);
    let last: Table = history.get(history.raw_len()).unwrap();
    assert_eq!(last.get::<String>("text").unwrap(), "relay answer");
}

// The shared strip corner (SPEC.md 7.1 and 9.7, decision 13). Both addons tick at the
// same whole seconds, and a strip takes 0.5 s, so each timeline below is exact. The
// hellos of both addons at login are done after 7 s: a strip, a tail of 2 s, and a strip.

/// Past the login hellos of both addons and their tails.
const AFTER_HELLOS: f64 = 7.0;

const BLOCKED_LINE: &str = "Gnomish Relay: screenshots are blocked by another addon.";
const HOSTILE_HOLDER: &str = "GnomishStripCorner = { holder = 'HostileStrip', endsAt = math.huge }";

fn relay_send(ns: &Table, text: &str) {
    let send: Function = ns.get::<Table>("Window").unwrap().get("Send").unwrap();
    send.call::<()>(text).unwrap();
}

fn call<R: mlua::FromLuaMulti>(ns: &Table, module: &str, function: &str) -> R {
    let module: Table = ns.get(module).unwrap();
    module.get::<Function>(function).unwrap().call(()).unwrap()
}

impl Game {
    fn run(&self, code: &str) {
        self.lua.load(code).exec().unwrap();
    }

    fn overlaps(&self) -> u32 {
        self.wow.get("overlaps").unwrap()
    }

    fn printed(&self, line: &str) -> usize {
        let printed: Vec<String> = self.wow.get("printed").unwrap();
        printed.iter().filter(|l| *l == line).count()
    }

    fn reloads(&self) -> u32 {
        self.wow.get("reloads").unwrap()
    }
}

#[test]
fn two_addons_take_turns_and_never_show_their_strips_at_once() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();

    for n in 0..4 {
        relay_send(&relay, &format!("relay {n}"));
        link_send(&timeways, &format!("story {n}"));
        game.advance(0.3);
    }
    game.advance(40.0);

    assert_eq!(game.overlaps(), 0);
    for n in 0..4 {
        let relay_text = format!("relay {n}");
        let story_text = format!("story {n}");
        assert!(
            game.shows_of(&RELAY, relay_text.as_bytes()) > 0,
            "{relay_text}"
        );
        assert!(
            game.shows_of(&TIMEWAYS, story_text.as_bytes()) > 0,
            "{story_text}"
        );
    }
    assert_eq!(game.printed(BLOCKED_LINE), 0);
}

#[test]
fn the_relay_strip_goes_out_within_four_seconds_while_the_test_addon_sends() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();

    link_send(&timeways, "story first");
    relay_send(&relay, "relay second");
    game.advance(4.0);

    assert_eq!(game.shows_of(&TIMEWAYS, b"story first"), 1);
    assert_eq!(game.shows_of(&RELAY, b"relay second"), 1);
    assert_eq!(game.overlaps(), 0);
}

#[test]
fn an_app_that_waits_longer_gets_the_next_turn() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();

    relay_send(&relay, "relay a");
    link_send(&timeways, "story x");
    game.advance(0.6);
    // The strip of "relay a" has ended, and the relay holds the corner for its tail.
    relay_send(&relay, "relay b");
    game.advance(3.4);
    let story_at_4 = game.shows_of(&TIMEWAYS, b"story x");
    let relay_b_at_4 = game.shows_of(&RELAY, b"relay b");
    game.advance(3.0);

    assert_eq!(game.shows_of(&RELAY, b"relay a"), 1);
    assert_eq!(story_at_4, 1);
    assert_eq!(relay_b_at_4, 0);
    assert_eq!(game.shows_of(&RELAY, b"relay b"), 1);
}

#[test]
fn a_late_failed_event_of_one_app_never_ends_the_strip_of_the_other_or_its_health() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();
    game.advance(AFTER_HELLOS);
    // The event of the next shot comes after the 10 s timeout of its strip, and fails.
    game.wow.set("shotDelay", 10.5).unwrap();
    game.wow.set("shotsBlocked", true).unwrap();
    link_send(&timeways, "story slow");
    game.advance(0.2);
    game.wow.set("shotDelay", 0.4).unwrap();
    game.wow.set("shotsBlocked", false).unwrap();

    relay_send(&relay, "relay waits");
    game.advance(10.5);
    let relay_shows_during_the_hold = game.shows_of(&RELAY, b"relay waits");
    game.advance(3.0);

    assert_eq!(relay_shows_during_the_hold, 0);
    assert_eq!(game.shows_of(&RELAY, b"relay waits"), 1);
    let flags: Vec<String> = call(&relay, "Health", "Flags");
    assert!(flags.contains(&"out=shot".into()), "{flags:?}");
    let flags: Vec<String> = call(&timeways, "Health", "Flags");
    assert!(flags.contains(&"out=fail".into()), "{flags:?}");
    assert_eq!(game.overlaps(), 0);
}

#[test]
fn a_hostile_holder_gives_one_blocked_line_before_the_frame_is_stale_and_no_reload() {
    let game = Game::new();
    let relay = game.relay();
    game.run(HOSTILE_HOLDER);

    relay_send(&relay, "held back");
    game.advance(29.5);
    let early = game.printed(BLOCKED_LINE);
    game.advance(1.0);
    let at_30 = game.printed(BLOCKED_LINE);
    game.advance(260.0);

    assert_eq!(early, 0);
    assert_eq!(at_30, 1);
    assert_eq!(game.printed(BLOCKED_LINE), 1);
    assert!(game.shots(RELAY.strip).is_empty());
    assert_eq!(game.reloads(), 0);
    let db: Table = game.lua.globals().get("GnomishRelayDB").unwrap();
    assert_eq!(db.get::<Table>("outbox").unwrap().raw_len(), 0);
    assert!(!call::<bool>(&relay, "Transport", "NeedsReload"));
    assert_eq!(call::<String>(&relay, "Transport", "Problem"), "blocked");
}

#[test]
fn the_blocked_line_shows_again_only_after_the_corner_was_free_between() {
    let game = Game::new();
    let timeways = game.timeways();
    let line = "Timeways: screenshots are blocked by another addon.";
    game.run(HOSTILE_HOLDER);
    link_send(&timeways, "first");
    game.advance(100.0);
    let first_episode = game.printed(line);

    game.run("GnomishStripCorner = nil");
    game.advance(2.0);
    let blocked_when_free: bool = call(&timeways, "Health", "Blocked");
    game.run(HOSTILE_HOLDER);
    link_send(&timeways, "second");
    game.advance(40.0);

    assert_eq!(first_episode, 1);
    assert!(!blocked_when_free);
    assert_eq!(game.printed(line), 2);
}

#[test]
fn the_retry_count_does_not_grow_while_the_test_addon_waits_for_the_corner() {
    let game = Game::new();
    let timeways = game.timeways();
    link_send(&timeways, "look around");
    game.advance(2.0);
    game.run("GnomishStripCorner = { holder = 'HostileStrip', endsAt = GetTime() + 120 }");

    game.advance(118.0);
    let shows_while_held = game.shows_of(&TIMEWAYS, b"look around");
    let outbox_while_held = game.timeways_db().get::<Table>("outbox").unwrap().raw_len();
    game.advance(3.0);

    assert_eq!(shows_while_held, 1);
    assert_eq!(outbox_while_held, 0);
    assert_eq!(game.shows_of(&TIMEWAYS, b"look around"), 2);
}

#[test]
fn a_holder_that_stopped_with_an_error_frees_the_corner_after_its_hold() {
    let game = Game::new();
    let relay = game.relay();
    game.run("GnomishStripCorner = { holder = 'TimewaysStrip', endsAt = GetTime() + 12 }");

    relay_send(&relay, "after the hold");
    game.advance(11.5);
    let during = game.shows_of(&RELAY, b"after the hold");
    game.advance(1.5);

    assert_eq!(during, 0);
    assert_eq!(game.shows_of(&RELAY, b"after the hold"), 1);
    assert_eq!(game.printed(BLOCKED_LINE), 0);
}

#[test]
fn a_login_hello_that_waits_for_the_corner_goes_out_after_the_wait() {
    let game = Game::new();
    game.run("GnomishStripCorner = { holder = 'TimewaysStrip', endsAt = GetTime() + 5 }");

    game.relay();
    game.advance(7.0);

    let strips = game.strips(&RELAY);
    assert_eq!(strips.len(), 1);
    assert!(flags_of(&strips[0][0]).contains(&"h".into()));
}

#[test]
fn a_corner_value_of_a_wrong_type_or_with_a_metatable_counts_as_free() {
    for junk in [
        "GnomishStripCorner = 'junk'",
        "GnomishStripCorner = { holder = 'HostileStrip', endsAt = 'never', waits = 7 }",
        "GnomishStripCorner = { waits = { HostileStrip = { since = 'old', at = {} } } }",
        "GnomishStripCorner = setmetatable({}, { __index = function() error('read') end, \
         __newindex = function() error('write') end })",
    ] {
        let game = Game::new();
        let relay = game.relay();
        game.run(junk);

        relay_send(&relay, "through");
        game.advance(1.0);

        assert_eq!(game.shows_of(&RELAY, b"through"), 1, "{junk}");
    }
}

#[test]
fn a_player_screenshot_does_not_end_a_strip_while_both_addons_run() {
    let game = Game::new();
    let relay = game.relay();
    let timeways = game.timeways();
    game.advance(AFTER_HELLOS);

    link_send(&timeways, "story kept");
    // The player's own screenshot finishes before the strip of the test addon is taken.
    game.fire("SCREENSHOT_SUCCEEDED", ());
    relay_send(&relay, "relay next");
    game.advance(4.0);

    assert_eq!(game.shows_of(&TIMEWAYS, b"story kept"), 1);
    assert_eq!(game.shows_of(&RELAY, b"relay next"), 1);
    assert_eq!(game.overlaps(), 0);
}
