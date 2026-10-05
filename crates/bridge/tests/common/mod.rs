//! Runs the addon in a real Lua 5.1, with WoW's `bit` library as a Lua shim.

// Each test file uses a different part of this module.
#![allow(dead_code)]
// Clippy sees a shared test module as normal code, so its test exceptions miss it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bridge::fixture::{self, BitResults, Fake, Fixture, SavedVariables};
use bridge::run::Bridge;
use bridge::wow_client::WowClient;
use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Table, Value};
use serde_json::Value as Json;

#[derive(Clone, Copy)]
pub enum Bits {
    /// What WoW's `bit` returns.
    Unsigned,
    /// What the `bit` of `LuaJIT` returns.
    Signed,
}

pub fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn lua(bits: Bits) -> Lua {
    let lua = Lua::new();
    let mode = match bits {
        Bits::Unsigned => "unsigned",
        Bits::Signed => "signed",
    };
    let bit: Table = lua
        .load(repo_file("addon/tests/bit.lua"))
        .set_name("bit.lua")
        .call(mode)
        .unwrap();
    lua.globals().set("bit", bit).unwrap();
    lua
}

/// Loads addon files the way WoW does: each file gets the addon name and the shared table.
pub fn load(lua: &Lua, files: &[&str]) -> Table {
    let ns = lua.create_table().unwrap();
    load_into(lua, &ns, files);
    ns
}

pub fn load_into(lua: &Lua, ns: &Table, files: &[impl AsRef<str>]) {
    load_addon(lua, "GnomishRelay", ns, files);
}

/// The shared transport lives in `addon/transport`, and each addon gets it at install.
pub fn addon_file(addon: &str, file: &str) -> String {
    let shared = format!("addon/transport/{file}");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if root.join(&shared).exists() {
        return repo_file(&shared);
    }
    repo_file(&format!("addon/{addon}/{file}"))
}

pub fn load_addon(lua: &Lua, addon: &str, ns: &Table, files: &[impl AsRef<str>]) {
    for file in files {
        let file = file.as_ref();
        lua.load(addon_file(addon, file))
            .set_name(file)
            .call::<()>((addon, ns.clone()))
            .unwrap();
    }
}

/// The files of `addon` in the load order of its TOC, so no test keeps a copy of the list.
pub fn toc_files(addon: &str) -> Vec<String> {
    repo_file(&format!("addon/{addon}/{addon}.toc"))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Deterministic test bytes.
pub fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x.to_be_bytes()[3]
        })
        .collect()
}

/// The screen of the strip and line tests. Their pictures and drawing math are for this
/// size, and the screen size is no part of the client.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Two calibration rows, then the cells of `frame`, 200 per row, as the addon draws them.
pub fn strip_rows(frame: &[u8]) -> Vec<Vec<u8>> {
    let mut rows: Vec<Vec<u8>> = vec![
        (0..200u8).map(|c| c % 8).collect(),
        (0..200u8).map(|c| 7 - c % 8).collect(),
    ];
    rows.extend(
        protocol::cell::encode_cells(frame)
            .chunks(200)
            .map(<[u8]>::to_vec),
    );
    rows
}

/// The pixels of a 1280x720 game scene with the strip drawn at a fractional cell size.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn scene(rows: &[Vec<u8>], pitch_x: f64, pitch_y: f64) -> Vec<u8> {
    let width = WIDTH as usize;
    let mut rgb = vec![70u8; width * HEIGHT as usize * 3];
    for (r, row) in rows.iter().enumerate() {
        for (c, &cell) in row.iter().enumerate() {
            let xs = (c as f64 * pitch_x) as usize..((c + 1) as f64 * pitch_x) as usize;
            for y in (r as f64 * pitch_y) as usize..((r + 1) as f64 * pitch_y) as usize {
                for x in xs.clone() {
                    let at = (y * width + x) * 3;
                    rgb[at..at + 3].copy_from_slice(&[
                        255 * (cell >> 2 & 1),
                        255 * (cell >> 1 & 1),
                        255 * (cell & 1),
                    ]);
                }
            }
        }
    }
    rgb
}

pub fn encode_png(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    png_bytes
}

/// A screenshot as WoW saves it at 1280x720, where a cell is 3.875 by 4 pixels.
pub fn screenshot_png(rows: &[Vec<u8>]) -> Vec<u8> {
    encode_png(
        WIDTH,
        HEIGHT,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &scene(rows, 3.875, 4.0),
    )
}

/// A frame with a valid tag under `key`, as the addon makes it.
pub fn signed_frame(time: u32, payload: &[u8], key: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut wire = protocol::frame::encode_frame(time, 1, payload, [0; 8]).unwrap();
    let signed = wire.len() - 8;
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    mac.update(&wire[..signed]);
    wire[signed..].copy_from_slice(&mac.finalize().into_bytes()[..8]);
    wire
}

pub fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// The client of the fake game: Forever, or the client that `GNOMISH_TEST_CLIENT`
/// names. CI runs the addon tests once more with `anniversary` (SPEC.md 7.9).
pub fn test_client() -> WowClient {
    let name = std::env::var("GNOMISH_TEST_CLIENT").unwrap_or_default();
    if name.is_empty() {
        return WowClient::Forever;
    }
    WowClient::ALL
        .into_iter()
        .find(|c| c.name() == name)
        .unwrap_or_else(|| panic!("GNOMISH_TEST_CLIENT names no client: {name}"))
}

/// The API file of the test client, which the fake game checks each call against.
pub fn api_file() -> &'static str {
    match test_client() {
        WowClient::Forever => "addon/tests/api.lua",
        WowClient::Anniversary => "addon/tests/api-anniversary.lua",
    }
}

/// The newest fixture of the test client in `tests/fixtures`: what the self-test
/// measured in the real game, or the Forever placeholder while nothing is measured
/// (SPEC.md 14.3).
pub fn fixture() -> Fixture {
    let path = fixture::newest_of(&repo_path("tests/fixtures"), test_client()).unwrap();
    fixture::read(&path).unwrap()
}

/// JSON as a Lua value. A null becomes nil, so a missing return value stays missing.
pub fn lua_value(lua: &Lua, json: &Json) -> Value {
    match json {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap()),
        Json::String(s) => Value::String(lua.create_string(s).unwrap()),
        Json::Array(items) => {
            let table = lua.create_table().unwrap();
            for (i, item) in items.iter().enumerate() {
                table.set(i + 1, lua_value(lua, item)).unwrap();
            }
            Value::Table(table)
        }
        Json::Object(fields) => {
            let table = lua.create_table().unwrap();
            for (key, item) in fields {
                table.set(key.as_str(), lua_value(lua, item)).unwrap();
            }
            Value::Table(table)
        }
    }
}

/// The behavior of the real game, as the newest fixture measured it.
pub fn measured() -> Fake {
    fixture().fake
}

/// A Lua 5.1 with the `bit` results of the client of `fake`.
pub fn game_lua_for(fake: &Fake) -> Lua {
    match fake.bit {
        BitResults::Unsigned => lua(Bits::Unsigned),
        BitResults::Signed => lua(Bits::Signed),
    }
}

pub fn game_lua() -> Lua {
    game_lua_for(&measured())
}

/// The fake game of `addon/tests/wow.lua` with the API file `api` and the behavior `fake`.
pub fn fake_game_for(lua: &Lua, api: &str, fake: &Fake) -> Table {
    let api: Table = lua
        .load(repo_file(api))
        .set_name("api.lua")
        .call(())
        .unwrap();
    let fake = serde_json::to_value(fake).unwrap();
    lua.load(repo_file("addon/tests/wow.lua"))
        .set_name("wow.lua")
        .call((api, lua_value(lua, &fake)))
        .unwrap()
}

/// The behavior of the real game on the screen of the strip tests.
pub fn measured_at_test_screen() -> Fake {
    let mut fake = measured();
    fake.physical_screen = [WIDTH, HEIGHT];
    fake
}

/// The fake game for the relay and the shared transport, as the real game measured, on
/// the screen of the strip tests.
pub fn fake_game(lua: &Lua) -> Table {
    fake_game_for(lua, api_file(), &measured_at_test_screen())
}

pub fn fire(lua: &Lua, wow: &Table, event: &str, args: impl IntoLuaMulti) {
    let fire: Function = wow.get("Fire").unwrap();
    let mut all = vec![Value::String(lua.create_string(event).unwrap())];
    all.extend(args.into_lua_multi(lua).unwrap());
    fire.call::<()>(MultiValue::from_vec(all)).unwrap();
}

/// The login events of the client of `fake`, in its order, for the addon `addon`.
pub fn log_in(lua: &Lua, wow: &Table, fake: &Fake, addon: &str) {
    for event in &fake.login_events {
        match event.as_str() {
            "ADDON_LOADED" => fire(lua, wow, event, addon),
            "PLAYER_ENTERING_WORLD" => fire(lua, wow, event, (true, false)),
            _ => fire(lua, wow, event, ()),
        }
    }
}

/// Runs the saved variables `saved` and the files of an addon in the order of the
/// client of `fake`, then logs in.
pub fn start_addon(
    lua: &Lua,
    wow: &Table,
    fake: &Fake,
    addon: &str,
    saved: Option<&[u8]>,
    files: impl FnOnce(),
) {
    let load_saved = || {
        if let Some(saved) = saved {
            lua.load(saved).set_name("SavedVariables").exec().unwrap();
        }
    };
    match fake.saved_variables {
        SavedVariables::BeforeFiles => {
            load_saved();
            files();
        }
        SavedVariables::AfterFiles => {
            files();
            load_saved();
        }
    }
    log_in(lua, wow, fake, addon);
}

/// Makes the slots from 1 to `SLOT_WINDOW`: the bridge writes only there until the addon
/// reports a next slot. A full install writes 4000 files, which takes most of a minute
/// on a Windows runner.
pub fn install_window(addons: &std::path::Path, app: protocol::apps::App) {
    use bridge::slots::{self, Files};
    for n in 1..=protocol::slot::SLOT_WINDOW {
        std::fs::create_dir(addons.join(slots::slot_name(app, n))).unwrap();
    }
    slots::publish(addons, app, &Files::empty(app, 0), 1).unwrap();
}

/// Returns `false` when `done` is still false after `limit`.
pub fn step_until_within(bridge: &mut Bridge, limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        bridge.step();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// WoW cuts a text with "..." only with no word wrap and a width: its own, or from a left
/// and a right edge.
pub fn is_cut(text: &Table) -> bool {
    let anchors: Option<Table> = text.get("anchors").unwrap();
    let has_edge = |points: [&str; 3]| {
        anchors
            .as_ref()
            .is_some_and(|a| points.iter().any(|p| a.contains_key(*p).unwrap()))
    };
    let both_edges = has_edge(["LEFT", "TOPLEFT", "BOTTOMLEFT"])
        && has_edge(["RIGHT", "TOPRIGHT", "BOTTOMRIGHT"]);
    let width = text.get::<Option<f64>>("width").unwrap().unwrap_or(0.0);
    let no_wrap = text.get::<Option<bool>>("wordWrap").unwrap() == Some(false);
    no_wrap && (width > 0.0 || both_edges)
}

/// The font string of the fake game that shows `shown`.
pub fn font_string_with(wow: &Table, shown: &str) -> Table {
    let frames: Table = wow.get("frames").unwrap();
    frames
        .sequence_values::<Table>()
        .map(Result::unwrap)
        .find(|f| {
            f.get::<String>("kind").unwrap() == "FontString"
                && f.get::<Option<String>>("text").unwrap().as_deref() == Some(shown)
        })
        .unwrap_or_else(|| panic!("no text {shown:?}"))
}
