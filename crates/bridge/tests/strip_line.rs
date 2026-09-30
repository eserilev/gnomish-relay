//! The line of `Strip.lua` in the fake game, drawn into a PNG and read by the bridge
//! (SPEC.md 7.1.3). The fake game keeps each texture of a shot as a rectangle of
//! physical pixels, so the test sees the pixels that the game would draw.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use bridge::ids::hex;
use bridge::line::{self, MODES, Mode};
use bridge::line_choice::{LineChoice, with_line};
use bridge::receive::{KeySet, StripKey, receive};
use bridge::strip::{Image, read_with};
use common::{HEIGHT, WIDTH, encode_png, fake_game, game_lua, load_addon};
use mlua::{Function, Lua, Table};
use png::{BitDepth, ColorType};
use protocol::apps::App;
use protocol::record::Record;
use protocol::slot::slot_body;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const STRIP: &str = "GnomishRelayStrip";
const FILES: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Health.lua",
    "Strip.lua",
    "Slots.lua",
];
const APP: &str = r#"
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

/// One texture of a shot, in physical pixels.
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
}

impl Game {
    fn new() -> Game {
        let lua = game_lua();
        let wow = fake_game(&lua);
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        lua.load(APP)
            .call::<()>(("GnomishRelay", ns.clone()))
            .unwrap();
        load_addon(&lua, "GnomishRelay", &ns, FILES);
        Game { lua, wow, ns }
    }

    /// `Strip.TakeLine` as a slot body calls it, with the screen of the fake game.
    fn take_line(&self, mode: u8) {
        self.take_line_for(mode, WIDTH, HEIGHT);
    }

    fn take_line_for(&self, mode: u8, width: u32, height: u32) {
        let line = self.lua.create_table().unwrap();
        line.set("mode", mode).unwrap();
        line.set("width", width).unwrap();
        line.set("height", height).unwrap();
        self.strip_fn("TakeLine").call::<()>(line).unwrap();
    }

    fn strip_fn(&self, name: &str) -> Function {
        self.ns.get::<Table>("Strip").unwrap().get(name).unwrap()
    }

    fn saved_line(&self) -> Option<Table> {
        self.lua
            .load("return GnomishRelayDB and GnomishRelayDB.stripLine")
            .eval()
            .unwrap()
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
            .get(STRIP)
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

    fn now(&self) -> u32 {
        self.lua.load("return time()").eval().unwrap()
    }
}

/// The picture on a grey 1280x720 scene, as WoW saves it.
fn image_of(picture: &[Rect]) -> Image {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut rgb = vec![70u8; width * height * 3];
    for rect in picture {
        for y in (rect.y..rect.y + rect.height).filter(|&y| y < height) {
            for x in (rect.x..rect.x + rect.width).filter(|&x| x < width) {
                let at = (y * width + x) * 3;
                rgb[at..at + 3].copy_from_slice(&rect.color);
            }
        }
    }
    let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb);
    Image::from_png(&png_bytes).unwrap()
}

fn records_of(image: &Image, now: u32) -> Vec<Record> {
    let keys = KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap();
    let bytes = read_with(image, |bytes| receive(bytes, &keys, now).is_ok()).unwrap();
    receive(&bytes, &keys, now).unwrap().1
}

fn line_mode(picture: &[Rect]) -> Option<Mode> {
    line::read(&image_of(picture)).map(|(mode, _)| mode)
}

/// The height of the picture in pixels, and its width.
fn extent(picture: &[Rect]) -> (usize, usize) {
    let bottom = picture.iter().map(|r| r.y + r.height).max().unwrap();
    let right = picture.iter().map(|r| r.x + r.width).max().unwrap();
    (bottom, right)
}

#[test]
fn the_line_of_each_mode_reads_back_through_a_png() {
    for mode in MODES {
        let game = Game::new();
        game.take_line(mode.id());

        game.show(7, "a line from the game");

        let picture = game.last_picture();
        assert_eq!(line_mode(&picture), Some(mode), "{}", mode.name());
        let records = records_of(&image_of(&picture), game.now());
        assert_eq!(records[0].text, b"a line from the game");
    }
}

#[test]
fn a_short_message_in_mode_1_is_a_line_1_pixel_tall_and_200_wide() {
    let game = Game::new();
    game.take_line(1);

    game.show(7, "hi");

    assert_eq!(extent(&game.last_picture()), (1, 200));
}

#[test]
fn the_largest_message_in_mode_1_adds_rows_and_keeps_the_width() {
    let game = Game::new();
    game.take_line(1);

    game.show(7, &"x".repeat(3000));

    let picture = game.last_picture();
    assert_eq!(extent(&picture), (6, 200));
    assert_eq!(
        records_of(&image_of(&picture), game.now())[0].text,
        "x".repeat(3000).as_bytes()
    );
}

#[test]
fn with_no_line_from_the_bridge_the_addon_draws_the_old_strip() {
    let game = Game::new();

    game.show(7, "old strip");

    let picture = game.last_picture();
    assert_eq!(line_mode(&picture), None);
    // The line test of SPEC.md 7.1.4 sits right of the old strip.
    let strip: Vec<&Rect> = picture.iter().filter(|r| r.x < 600).collect();
    assert!(strip.iter().all(|r| r.width == 3 && r.height == 3));
    assert_eq!(strip.iter().map(|r| r.x + r.width).max(), Some(600));
    assert_eq!(
        records_of(&image_of(&picture), game.now())[0].text,
        b"old strip"
    );
}

#[test]
fn a_line_for_another_screen_size_draws_the_old_strip() {
    let game = Game::new();
    game.take_line_for(1, 2560, 1440);

    game.show(7, "new resolution");

    assert_eq!(line_mode(&game.last_picture()), None);
}

#[test]
fn a_retry_of_the_same_frame_id_draws_the_old_strip() {
    let game = Game::new();
    game.take_line(1);

    game.show(7, "first show");
    game.show(7, "the retry");

    assert_eq!(line_mode(&game.last_picture()), None);
    game.show(8, "the next message");
    assert_eq!(line_mode(&game.last_picture()), Some(MODES[0]));
}

#[test]
fn a_hello_with_frame_id_0_is_never_a_retry() {
    let game = Game::new();
    game.take_line(1);

    game.show(0, "");
    game.show(0, "");

    assert_eq!(line_mode(&game.last_picture()), Some(MODES[0]));
}

#[test]
fn retries_of_two_messages_turn_the_line_off_until_a_reload() {
    let game = Game::new();
    game.take_line(1);
    for id in [7, 8] {
        game.show(id, "shown");
        game.show(id, "again");
    }

    game.show(9, "after two retries");

    assert_eq!(line_mode(&game.last_picture()), None);
    let reloaded = Game::new();
    reloaded.take_line(1);
    reloaded.show(9, "after a reload");
    assert_eq!(line_mode(&reloaded.last_picture()), Some(MODES[0]));
}

#[test]
fn a_line_of_a_wrong_shape_is_not_kept() {
    let game = Game::new();
    for bad in [
        "return nil",
        "return 1",
        "return { mode = 7, width = 1280, height = 720 }",
        "return { mode = 1.5, width = 1280, height = 720 }",
        "return { mode = 1, width = '1280', height = 720 }",
    ] {
        game.take_line(1);
        let line: mlua::Value = game.lua.load(bad).eval().unwrap();

        game.strip_fn("TakeLine").call::<()>(line).unwrap();

        assert!(game.saved_line().is_none(), "{bad}");
    }
}

#[test]
fn a_saved_line_of_a_wrong_shape_draws_the_old_strip() {
    let game = Game::new();
    game.lua
        .load("GnomishRelayDB = { stripLine = { mode = 'one', width = 1280, height = 720 } }")
        .exec()
        .unwrap();

    game.show(7, "damaged saved variables");

    assert_eq!(line_mode(&game.last_picture()), None);
}

/// The relay body that the bridge writes, with or without the line.
fn load_body(game: &Game, n: u32, choice: Option<LineChoice>) {
    let body = with_line(slot_body(App::Relay, game.now(), &[]), App::Relay, choice);
    game.wow
        .set("body", game.lua.create_string(body).unwrap())
        .unwrap();
    let load: Function = game.ns.get::<Table>("Slots").unwrap().get("Load").unwrap();
    load.call::<()>(n).unwrap();
}

#[test]
fn a_body_with_a_line_keeps_it_and_a_body_without_one_removes_it() {
    let game = Game::new();
    let choice = LineChoice {
        mode: 3,
        width: WIDTH,
        height: HEIGHT,
        reason: None,
    };

    load_body(&game, 1, Some(choice));
    game.show(7, "after the first body");

    assert_eq!(line_mode(&game.last_picture()), Some(MODES[2]));
    load_body(&game, 2, None);
    assert!(game.saved_line().is_none());
    game.show(8, "after the second body");
    assert_eq!(line_mode(&game.last_picture()), None);
}
