//! The line test of SPEC.md 7.1.4: `Strip.lua` in the fake game draws it next to an old
//! strip, the real bridge judges its PNG, and the body brings the mode back.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bridge::agent::Echo;
use bridge::config::{Permission, Policy};
use bridge::folder_path::path_bytes;
use bridge::ids::hex;
use bridge::line::{self, MODES, Mode};
use bridge::line_choice::{self, LineChoice, Reason};
use bridge::line_test::{self, LEFT};
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{BODY_FILE, slot_name};
use bridge::strip::{Image, read_with};
use common::{HEIGHT, WIDTH, encode_png, fake_game, game_lua, install_window, load_addon};
use mlua::{Function, Lua, Table};
use png::{BitDepth, ColorType};
use protocol::apps::App;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const FILES: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Health.lua",
    "Strip.lua",
    "Slots.lua",
];

/// The names of one app of the shared transport.
struct AppNames {
    addon: &'static str,
    strip: &'static str,
    table: &'static str,
}

const RELAY: AppNames = AppNames {
    addon: "GnomishRelay",
    strip: "GnomishRelayStrip",
    table: r#"{
	title = "Gnomish Relay", version = 1, helloChat = "relay",
	slotPrefix = "GnomishRelay_S%04d", slotData = "GnomishRelay_SlotData",
	restore = "GnomishRelay_Restore", live = "GnomishRelay_Live",
	strip = "GnomishRelayStrip", saved = "GnomishRelayDB",
}"#,
};

const TIMEWAYS: AppNames = AppNames {
    addon: "Timeways",
    strip: "TimewaysStrip",
    table: r#"{
	title = "Timeways", version = 1, helloChat = "timeways",
	slotPrefix = "Timeways_S%04d", slotData = "Timeways_SlotData",
	restore = "Timeways_Restore", live = "Timeways_Live",
	strip = "TimewaysStrip", saved = "TimewaysDB",
}"#,
};

/// One texture of a shot, in physical pixels.
#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: [u8; 3],
}

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
    strip: &'static str,
}

impl Game {
    fn new() -> Game {
        Game::of(&RELAY)
    }

    /// The clock of the game follows this computer, so the bridge takes its frames.
    fn of(app: &AppNames) -> Game {
        let lua = game_lua();
        let wow = fake_game(&lua);
        wow.set("strips", vec![app.strip]).unwrap();
        let game_now: f64 = wow.get("now").unwrap();
        #[allow(clippy::cast_possible_truncation)] // the fake clock starts at 1000
        wow.set("epoch", i64::from(now()) - game_now as i64 + 1000)
            .unwrap();
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        let names: Table = lua.load(format!("return {}", app.table)).eval().unwrap();
        ns.set("App", names).unwrap();
        load_addon(&lua, app.addon, &ns, FILES);
        Game {
            lua,
            wow,
            ns,
            strip: app.strip,
        }
    }

    fn set_screen(&self, width: u32, height: u32) {
        let fake: Table = self.wow.get("fake").unwrap();
        fake.set("physical_screen", vec![width, height]).unwrap();
    }

    fn screen(&self) -> (u32, u32) {
        let fake: Table = self.wow.get("fake").unwrap();
        let size: Vec<u32> = fake.get("physical_screen").unwrap();
        (size[0], size[1])
    }

    /// Signs one record with frame id `id` and shows it, then lets the shot finish.
    fn show(&self, id: u32, text: &str) {
        let show: Function = self
            .lua
            .load(
                r#"local ns, id, text = ...
                local record = { token = "tok", chat = "c1", id = id, flags = "", text = text }
                local frame = ns.Codec.Frame(time(), id, ns.Codec.Payload({ record }), ns.key)
                assert(ns.Strip.Show(frame, function() end))"#,
            )
            .into_function()
            .unwrap();
        show.call::<()>((self.ns.clone(), id, text)).unwrap();
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(3.0).unwrap();
    }

    fn last_picture(&self) -> Vec<Rect> {
        let pictures: Table = self
            .wow
            .get::<Table>("picturesOf")
            .unwrap()
            .get(self.strip)
            .unwrap();
        let picture: Table = pictures.get(pictures.raw_len()).unwrap();
        picture
            .sequence_values::<Table>()
            .map(|rect| {
                let rect = rect.unwrap();
                Rect {
                    x: rect.get("x").unwrap(),
                    y: rect.get("y").unwrap(),
                    width: rect.get("width").unwrap(),
                    height: rect.get("height").unwrap(),
                    color: rect.get::<Vec<u8>>("color").unwrap().try_into().unwrap(),
                }
            })
            .collect()
    }

    /// Loads a body of the bridge into slot `n`, as the slot poll does.
    fn load_body(&self, n: u32, body: &str) {
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
        let load: Function = self.ns.get::<Table>("Slots").unwrap().get("Load").unwrap();
        load.call::<()>(n).unwrap();
    }
}

/// The picture on a grey scene of the screen of the game, as WoW saves it.
fn png_of(picture: &[Rect], (width, height): (u32, u32)) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut rgb = vec![70u8; w * h * 3];
    for rect in picture {
        for y in (rect.y..rect.y + rect.height).filter(|&y| y < h) {
            for x in (rect.x..rect.x + rect.width).filter(|&x| x < w) {
                let at = (y * w + x) * 3;
                rgb[at..at + 3].copy_from_slice(&rect.color);
            }
        }
    }
    encode_png(width, height, ColorType::Rgb, BitDepth::Eight, &rgb)
}

/// Each pixel of the test lines takes a quarter from its left and its right neighbor, as
/// a soft scaler does. The old strip stays sharp: its cells are 3 pixels wide.
fn blurred(picture: &[Rect]) -> Vec<Rect> {
    let mut sharp = vec![[70u8; 3]; WIDTH as usize * 24];
    let mut rest = Vec::new();
    for rect in picture {
        if rect.x < LEFT {
            rest.push(*rect);
            continue;
        }
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..(rect.x + rect.width).min(WIDTH as usize) {
                sharp[y * WIDTH as usize + x] = rect.color;
            }
        }
    }
    let width = WIDTH as usize;
    for y in 0..24 {
        for x in LEFT..width - 1 {
            let [left, here, right] = [x - 1, x, x + 1].map(|at| sharp[y * width + at]);
            let mix = |ch: usize| {
                let sum = u16::from(left[ch]) + 2 * u16::from(here[ch]) + u16::from(right[ch]);
                u8::try_from(sum / 4).unwrap()
            };
            rest.push(Rect {
                x,
                y,
                width: 1,
                height: 1,
                color: [mix(0), mix(1), mix(2)],
            });
        }
    }
    rest
}

fn image(png: &[u8]) -> Image {
    Image::from_png(png).unwrap()
}

/// The result of the line test in a picture, as the bridge judges it.
fn result_of(png: &[u8]) -> Option<LineChoice> {
    let image = image(png);
    let bytes = read_with(&image, |_| true).unwrap();
    line_test::result(&image, &bytes)
}

fn line_mode(png: &[u8]) -> Option<Mode> {
    line::read(&image(png)).map(|(mode, _)| mode)
}

fn test_lines(picture: &[Rect]) -> usize {
    picture.iter().filter(|r| r.x >= LEFT).count()
}

struct Desktop {
    _root: tempfile::TempDir,
    addons: PathBuf,
    screenshots: PathBuf,
    data: PathBuf,
    bridge: Bridge,
}

impl Desktop {
    fn new() -> Desktop {
        let root = tempfile::tempdir().unwrap();
        let dir = |name: &str| {
            let path = root.path().join(name);
            fs::create_dir_all(&path).unwrap();
            path
        };
        let (addons, screenshots) = (dir("Interface/AddOns"), dir("Screenshots"));
        let (accounts, data) = (dir("WTF/Account"), dir("data"));
        install_window(&addons, App::Relay);
        let paths = Paths {
            addons: addons.clone(),
            screenshots: screenshots.clone(),
            accounts,
            state: data.clone(),
            config: data.join("config"),
        };
        let base = path_bytes(&std::env::temp_dir().canonicalize().unwrap());
        let policy = Policy {
            folders: Folders {
                roots: vec![base.clone()],
                base,
            },
            agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
            default_agent: "claude".into(),
        };
        let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
        let agents = [("claude".to_owned(), Arc::new(Echo) as _)].into();
        let bridge = Bridge::new(paths, policy, keys, agents).unwrap();
        Desktop {
            _root: root,
            addons,
            screenshots,
            data,
            bridge,
        }
    }

    fn body(&self) -> String {
        fs::read_to_string(self.addons.join(slot_name(App::Relay, 1)).join(BODY_FILE)).unwrap()
    }

    /// Hands the screenshot to the bridge, and waits for a body that holds `wanted`.
    fn take(&mut self, png: &[u8], wanted: &str) -> String {
        fs::write(self.screenshots.join("WoWScrnShot_1.png"), png).unwrap();
        let addons = self.addons.clone();
        let body = || {
            fs::read_to_string(addons.join(slot_name(App::Relay, 1)).join(BODY_FILE))
                .is_ok_and(|b| b.contains(wanted))
        };
        let found = common::step_until_within(&mut self.bridge, Duration::from_secs(30), body);
        assert!(found, "no {wanted} in {}", self.body());
        self.body()
    }
}

#[test]
fn a_fresh_session_draws_the_line_test_next_to_its_first_strip() {
    let game = Game::new();

    game.show(7, "the first message");

    let picture = game.last_picture();
    assert!(test_lines(&picture) >= 6 * 200, "{}", test_lines(&picture));
    let png = png_of(&picture, game.screen());
    assert_eq!(line_mode(&png), None, "the message goes in the old strip");
    let result = result_of(&png).unwrap();
    assert_eq!(
        (result.mode, result.width, result.height),
        (1, WIDTH, HEIGHT)
    );
}

#[test]
fn the_bridge_sends_the_smallest_clean_mode_and_the_next_strip_is_a_line() {
    let game = Game::new();
    let mut desktop = Desktop::new();
    game.show(7, "the first message");
    let png = png_of(&game.last_picture(), game.screen());

    let body = desktop.take(&png, ".line = {mode = 1, width = 1280, height = 720}");
    game.load_body(1, &body);
    game.show(8, "the second message");

    let picture = game.last_picture();
    assert_eq!(line_mode(&png_of(&picture, game.screen())), Some(MODES[0]));
    assert!(
        picture.iter().all(|r| r.y == 0 && r.x < 200),
        "a line 1 px tall"
    );
    assert_eq!(
        line_choice::status_line(&desktop.data),
        "Colored bar: a thin line, 1 px tall (mode 1), for 1280x720."
    );
}

#[test]
fn a_blurred_line_test_keeps_the_old_strip_and_the_status_says_why() {
    let game = Game::new();
    let mut desktop = Desktop::new();
    game.show(7, "the first message");
    let png = png_of(&blurred(&game.last_picture()), game.screen());

    let body = desktop.take(&png, ".line = {mode = 0, width = 1280, height = 720}");
    game.load_body(1, &body);
    game.show(8, "the second message");

    let picture = game.last_picture();
    assert_eq!(line_mode(&png_of(&picture, game.screen())), None);
    assert_eq!(
        test_lines(&picture),
        0,
        "one test a session after no clean mode"
    );
    assert_eq!(
        line_choice::status_line(&desktop.data),
        "Colored bar: full size, because your screen blurs 1-px lines (anti-aliasing, render scale, or an upscaler). Messages still get through."
    );
    assert_eq!(
        line_choice::load(&desktop.data).unwrap()[0].reason,
        Some(Reason::Blur)
    );
}

#[test]
fn after_no_clean_mode_a_reload_tests_again() {
    let game = Game::new();
    game.lua
        .load("GnomishRelayDB = { stripLine = { mode = 0, width = 1280, height = 720 } }")
        .exec()
        .unwrap();

    game.show(7, "after a reload");

    assert!(test_lines(&game.last_picture()) > 0);
}

#[test]
fn a_new_screen_size_tests_again_for_that_size() {
    let game = Game::new();
    game.lua
        .load("GnomishRelayDB = { stripLine = { mode = 1, width = 1280, height = 720 } }")
        .exec()
        .unwrap();
    game.set_screen(1600, 900);

    game.show(7, "on a new screen");

    let png = png_of(&game.last_picture(), game.screen());
    assert_eq!(line_mode(&png), None);
    let result = result_of(&png).unwrap();
    assert_eq!((result.mode, result.width, result.height), (1, 1600, 900));
}

#[test]
fn with_a_line_for_this_screen_no_strip_carries_the_test() {
    let game = Game::new();
    game.lua
        .load("GnomishRelayDB = { stripLine = { mode = 1, width = 1280, height = 720 } }")
        .exec()
        .unwrap();

    game.show(7, "shown");
    game.show(7, "the retry draws the old strip");

    let picture = game.last_picture();
    assert_eq!(line_mode(&png_of(&picture, game.screen())), None);
    assert_eq!(test_lines(&picture), 0);
}

#[test]
fn a_bridge_that_never_answers_gets_at_most_3_tests_in_a_session() {
    let game = Game::new();
    let mut tested = 0;

    for id in 1..=5 {
        game.show(id, "no answer");
        tested += usize::from(test_lines(&game.last_picture()) > 0);
    }

    assert_eq!(tested, 3);
}

#[test]
fn a_timeways_strip_carries_the_line_test_too() {
    let game = Game::of(&TIMEWAYS);

    game.show(7, "a story message");

    let png = png_of(&game.last_picture(), game.screen());
    assert_eq!(result_of(&png).map(|r| r.mode), Some(1));
}

#[test]
fn a_body_with_mode_0_is_kept_as_a_test_with_no_clean_mode() {
    let game = Game::new();

    game.load_body(
        1,
        "GnomishRelay_SlotData = {}\nGnomishRelay_SlotData.line = {mode = 0, width = 1280, height = 720}\n",
    );

    let mode: i64 = game
        .lua
        .load("return GnomishRelayDB.stripLine.mode")
        .eval()
        .unwrap();
    assert_eq!(mode, 0);
}

#[test]
fn the_test_payload_and_the_beacon_of_the_addon_match_the_bridge() {
    let game = Game::new();
    let codec: Table = game.ns.get("Codec").unwrap();
    let beacon: Function = codec.get("Beacon").unwrap();

    let lua_payload: mlua::String = codec.get("LINE_TEST").unwrap();
    let lua_beacon: mlua::String = beacon.call((2560, 1440)).unwrap();

    assert_eq!(lua_payload.as_bytes().to_vec(), line_test::payload());
    let screen = line_test::Screen {
        width: 2560,
        height: 1440,
    };
    assert_eq!(lua_beacon.as_bytes().to_vec(), line_test::beacon(screen));
}

#[test]
fn every_test_line_fits_a_screen_1024_pixels_wide() {
    let widest = MODES
        .iter()
        .map(|&m| line_test::origin(m).0 + 200 * m.pixels())
        .max();
    assert_eq!(widest, Some(1008));
}
