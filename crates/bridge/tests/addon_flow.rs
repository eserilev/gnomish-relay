//! The whole addon in a fake game: strips out, slots in, the outbox, restore, and the window.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fmt::Write;

use bridge::agent::{Agent, Echo};
use bridge::receive::{StripKey, receive};
use bridge::relay::{Folders, Relay};
use bridge::strip::{self, Image};
use common::{Bits, load_into, lua, repo_file};
use hmac::{Hmac, Mac};
use mlua::{Function, Lua, Table, Value};
use protocol::cell::decode_cells;
use protocol::frame::{decode_frame, signed_len};
use protocol::record::{Record, parse_records};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};
use sha2::Sha256;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const FILES: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Store.lua",
    "Strip.lua",
    "Transport.lua",
    "Window.lua",
    "Core.lua",
];

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

impl Game {
    fn start() -> Game {
        Game::start_with(|_| {})
    }

    /// `before` runs before login, for example to mark slots as loaded.
    fn start_with(before: impl FnOnce(&Table)) -> Game {
        let lua = lua(Bits::Unsigned);
        let wow: Table = lua
            .load(repo_file("addon/tests/wow.lua"))
            .set_name("wow.lua")
            .call(())
            .unwrap();
        before(&wow);
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        load_into(&lua, &ns, FILES);
        let game = Game { lua, wow, ns };
        game.fire("ADDON_LOADED", "GnomishRelay");
        game.fire("PLAYER_LOGIN", ());
        game
    }

    fn fire(&self, event: &str, args: impl mlua::IntoLuaMulti) {
        let fire: Function = self.wow.get("Fire").unwrap();
        let mut all = vec![Value::String(self.lua.create_string(event).unwrap())];
        all.extend(args.into_lua_multi(&self.lua).unwrap());
        fire.call::<()>(mlua::MultiValue::from_vec(all)).unwrap();
    }

    fn advance(&self, seconds: f64) {
        self.wow
            .get::<Function>("Advance")
            .unwrap()
            .call::<()>(seconds)
            .unwrap();
    }

    fn run(&self, code: &str) -> Value {
        self.lua.load(code).call(self.ns.clone()).unwrap()
    }

    fn db(&self) -> Table {
        self.lua.globals().get("GnomishRelayDB").unwrap()
    }

    fn send(&self, text: &str) {
        let send: Function = self.ns.get::<Table>("Window").unwrap().get("Send").unwrap();
        send.call::<()>(text).unwrap();
    }

    fn chat_id(&self) -> String {
        self.db()
            .get::<Table>("chats")
            .unwrap()
            .get::<Table>(1)
            .unwrap()
            .get("id")
            .unwrap()
    }

    fn shots(&self) -> usize {
        self.wow.get::<Table>("shots").unwrap().raw_len()
    }

    /// The cells of screenshot `n`, row by row, calibration rows first.
    fn shot_rows(&self, n: usize) -> Vec<Vec<u8>> {
        let rows: Table = self.wow.get::<Table>("shots").unwrap().get(n).unwrap();
        (1..=rows.raw_len())
            .map(|row| rows.get(row).unwrap())
            .collect()
    }

    /// Decodes screenshot `n` with the Rust decoder, and checks its tag.
    fn strip(&self, n: usize) -> Vec<Record> {
        let mut cells: Vec<u8> = self.shot_rows(n).into_iter().skip(2).flatten().collect();
        cells.truncate(cells.len() / 8 * 8);
        let wire = decode_cells(&cells).expect("the strip holds whole cell groups");
        let frame = decode_frame(&wire)
            .ok()
            .expect("the strip is a valid frame");
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(&wire[..signed_len(&frame)]);
        assert_eq!(
            frame.tag[..],
            mac.finalize().into_bytes()[..8],
            "strip {n} has a bad tag"
        );
        parse_records(&frame.payload)
            .ok()
            .expect("the payload parses")
    }

    fn last_strip(&self) -> Vec<Record> {
        self.strip(self.shots())
    }

    /// Puts a body into every slot, as the bridge does.
    fn publish(&self, replies: &[Reply]) {
        let body = slot_body(1_790_211_079, &prepare_replies(replies));
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
    }

    fn printed(&self) -> Vec<String> {
        self.wow.get::<Vec<String>>("printed").unwrap()
    }
}

/// Draws the cells the way WoW does at 1280x720, where a cell is 3.875 by 4 pixels,
/// and saves the image as a PNG.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn screenshot_png(rows: &[Vec<u8>]) -> Vec<u8> {
    let (width, height, pitch_x, pitch_y) = (1280usize, 720usize, 3.875, 4.0);
    let mut rgb = vec![70u8; width * height * 3];
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
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgb)
        .unwrap();
    png_bytes
}

fn reply(chat: &str, id: u32, status: Status, text: &str) -> Reply {
    Reply {
        chat: chat.as_bytes().to_vec(),
        id,
        status,
        text: text.as_bytes().to_vec(),
    }
}

fn flags(record: &Record) -> Vec<String> {
    String::from_utf8_lossy(&record.flags)
        .split(';')
        .map(String::from)
        .collect()
}

fn first_message_id(game: &Game) -> u32 {
    game.db()
        .get::<Table>("chats")
        .unwrap()
        .get::<Table>(1)
        .unwrap()
        .get::<Table>("history")
        .unwrap()
        .get::<Table>(1)
        .unwrap()
        .get("id")
        .unwrap()
}

#[test]
fn a_sent_message_goes_out_as_a_signed_strip_that_the_bridge_decodes() {
    let game = Game::start();
    game.send("fix the flaky test");
    game.advance(1.0);

    let records = game.last_strip();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(
        r.token,
        game.db().get::<String>("token").unwrap().as_bytes()
    );
    assert_eq!(r.chat, game.chat_id().as_bytes());
    assert_eq!(r.id, first_message_id(&game));
    assert_eq!(r.text, b"fix the flaky test");
    let f = flags(r);
    assert!(
        f.contains(&"agent=claude".into())
            && f.contains(&"n".into())
            && f.contains(&"next=1".into()),
        "{f:?}"
    );
}

#[test]
fn the_strip_is_on_screen_only_while_the_screenshot_is_taken() {
    let game = Game::start();
    game.send("hello");
    game.advance(1.0);
    let strip: Table = game.lua.globals().get("GnomishRelayStrip").unwrap();
    assert!(!strip.get::<bool>("shown").unwrap());
}

#[test]
fn the_first_poll_after_a_send_reads_the_reply_and_whispers_it() {
    let game = Game::start();
    game.send("fix the flaky test");
    game.advance(1.0);
    game.publish(&[reply(
        &game.chat_id(),
        first_message_id(&game),
        Status::Done,
        "Fixed | all green",
    )]);
    game.advance(5.0);

    let whisper = game
        .printed()
        .into_iter()
        .find(|l| l.contains("whispers:"))
        .expect("a whisper line");
    assert!(
        whisper.starts_with("|cfff0a860|Hgnomishrelay:"),
        "{whisper}"
    );
    assert!(
        whisper.contains("[Claude]") && whisper.contains("Fixed || all green"),
        "{whisper}"
    );
    assert_eq!(game.wow.get::<Vec<i64>>("sounds").unwrap(), [3081]);
}

#[test]
fn a_read_reply_is_reported_in_the_next_strip() {
    let game = Game::start();
    game.send("one");
    game.advance(1.0);
    let id = first_message_id(&game);
    game.publish(&[reply(&game.chat_id(), id, Status::Done, "done")]);
    game.advance(5.0);
    game.send("two");
    game.advance(1.0);

    let f = flags(&game.last_strip()[0]);
    assert!(f.contains(&format!("read={id}")), "{f:?}");
    assert!(f.contains(&"next=2".into()), "{f:?}");
}

#[test]
fn an_acknowledged_message_is_not_shown_again() {
    let game = Game::start();
    game.send("long task");
    game.advance(1.0);
    game.publish(&[reply(
        &game.chat_id(),
        first_message_id(&game),
        Status::Working,
        "",
    )]);
    game.advance(5.0);
    let shots = game.shots();
    game.advance(120.0);
    assert_eq!(game.shots(), shots);
}

#[test]
fn an_unacknowledged_message_goes_to_the_outbox_after_three_shows() {
    let game = Game::start();
    game.send("anyone there?");
    game.advance(130.0);

    assert_eq!(game.shots(), 3);
    let outbox: Table = game.db().get("outbox").unwrap();
    assert_eq!(outbox.raw_len(), 1);
    let entry: Table = outbox.get(1).unwrap();
    assert_eq!(
        entry.get::<String>("text").unwrap(),
        "616e796f6e652074686572653f"
    );
    assert!(
        game.run("local ns = ... return ns.Transport.NeedsReload()")
            .as_boolean()
            .unwrap()
    );
}

#[test]
fn a_send_with_few_slots_left_reloads_but_never_in_combat() {
    let low = |wow: &Table| {
        let loaded: Table = wow.get("loaded").unwrap();
        for n in 1..=985 {
            loaded.set(format!("GnomishRelay_S{n:04}"), true).unwrap();
        }
    };
    let game = Game::start_with(low);
    game.send("one");
    assert_eq!(game.wow.get::<i64>("reloads").unwrap(), 1);

    let game = Game::start_with(low);
    game.wow.set("combat", true).unwrap();
    game.send("one");
    assert_eq!(game.wow.get::<i64>("reloads").unwrap(), 0);
}

#[test]
fn after_twenty_polls_a_hello_reports_the_slot_position() {
    let game = Game::start();
    game.advance(2.0);
    for _ in 0..20 {
        game.run("local ns = ... ns.Transport.Poll()");
    }
    game.advance(2.0);

    let records = game.last_strip();
    assert_eq!(records[0].chat, b"relay");
    let f = flags(&records[0]);
    assert!(
        f.contains(&"h".into()) && f.contains(&"next=21".into()),
        "{f:?}"
    );
}

#[test]
fn a_restore_bundle_brings_chats_back_once_and_never_resends_them() {
    let game = Game::start();
    let token: String = game.db().get("token").unwrap();
    let body = format!(
        "GnomishRelay_SlotData = {{proto = 1, now = 1790211079, replies = {{}}, restore = {{token = \"{token}\", chats = {{
            {{id = \"oldchat01\", name = \"lighthouse\", agent = \"claude\",
              history = {{{{role = \"user\", id = 5, text = \"hi\"}}, {{role = \"agent\", id = 5, text = \"hello\"}}}}}}
        }}}}}}"
    );
    game.wow.set("body", body).unwrap();
    game.run("local ns = ... ns.Transport.Poll() ns.Transport.Poll()");
    game.advance(2.0);

    let chats: Table = game.db().get("chats").unwrap();
    assert_eq!(chats.raw_len(), 1);
    assert!(game.db().get::<bool>("restored").unwrap());
    let records = game.last_strip();
    assert_eq!(records.len(), 1, "only a hello, no resent message");
    assert!(flags(&records[0]).contains(&"restored".into()));
}

#[test]
fn a_body_from_another_protocol_version_is_reported() {
    let game = Game::start();
    game.wow
        .set("body", "GnomishRelay_SlotData = {proto = 2, replies = {}}")
        .unwrap();
    game.run("local ns = ... ns.Transport.Poll()");
    assert_eq!(
        game.run("local ns = ... return ns.Transport.Problem()")
            .as_string_lossy()
            .unwrap(),
        "mismatch"
    );
}

#[test]
fn missing_slots_are_reported_and_use_no_slot() {
    let game = Game::start_with(|wow| wow.set("slotsInstalled", false).unwrap());
    game.run("local ns = ... ns.Transport.Poll()");
    assert_eq!(
        game.run("local ns = ... return ns.Transport.Problem()")
            .as_string_lossy()
            .unwrap(),
        "missing"
    );
    assert_eq!(
        game.run("local ns = ... return ns.Transport.SlotsLeft()")
            .as_integer()
            .unwrap(),
        1000
    );
}

#[test]
fn the_window_shows_the_transcript_with_code_and_safe_pipes() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("show |cffff0000 red");
    game.advance(1.0);
    let id = first_message_id(&game);
    game.publish(&[reply(
        &game.chat_id(),
        id,
        Status::Done,
        "Here:\n```\nlet x = 1;\n```\nDone.",
    )]);
    game.advance(5.0);

    let transcript: Table = game.lua.globals().get("GnomishRelayTranscript").unwrap();
    let lines: Vec<String> = transcript.get("lines").unwrap();
    assert_eq!(
        lines,
        [
            "|cff69ccf0[You]|r: show ||cffff0000 red",
            "|cffff7d0a[Claude]|r: Here:",
            "    |cffb8c8b8let x = 1;|r",
            "Done.",
        ]
    );
}

#[test]
fn a_click_on_the_whisper_link_opens_that_chat() {
    let game = Game::start();
    game.send("hi");
    let id = game.chat_id();
    game.run(&format!("SetItemRef(\"gnomishrelay:{id}\")"));
    let frame: Table = game.lua.globals().get("GnomishRelayFrame").unwrap();
    assert!(frame.get::<bool>("shown").unwrap());
    assert_eq!(game.db().get::<String>("selected").unwrap(), id);
}

#[test]
fn a_message_goes_around_the_whole_loop_and_the_echo_comes_back() {
    let game = Game::start();
    game.send("ping the relay");
    game.advance(1.0);
    let now = 1_790_211_080;

    let png = screenshot_png(&game.shot_rows(game.shots()));
    let bytes = strip::read(&Image::from_png(&png).unwrap()).expect("the bridge finds the strip");
    let hex = KEY.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    let records = receive(&bytes, &StripKey::from_hex(&hex).unwrap(), now).unwrap();
    let mut relay = Relay::new(Folders {
        roots: vec![b"/home/x".to_vec()],
        base: b"/home/x".to_vec(),
    });
    relay.on_frame(&records, now);
    let job = relay.next_job().expect("the message is queued");
    relay.finish(&job, Echo.run(&job));
    game.wow
        .set("body", game.lua.create_string(relay.body(now)).unwrap())
        .unwrap();
    game.advance(5.0);

    let history: Table = game
        .db()
        .get::<Table>("chats")
        .unwrap()
        .get::<Table>(1)
        .unwrap()
        .get("history")
        .unwrap();
    let last: Table = history.get(history.raw_len()).unwrap();
    assert_eq!(last.get::<String>("text").unwrap(), "echo: ping the relay");
    assert!(
        game.printed()
            .iter()
            .any(|l| l.contains("echo: ping the relay"))
    );
}

#[test]
fn a_message_too_long_for_a_strip_stays_in_the_box_and_starts_no_screenshots() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    let input: Table = game.lua.globals().get("GnomishRelayInput").unwrap();
    input.set("text", "x".repeat(3150)).unwrap();
    let enter: Function = input
        .get::<Table>("scripts")
        .unwrap()
        .get("OnEnterPressed")
        .unwrap();
    enter.call::<()>(input.clone()).unwrap();
    game.advance(300.0);

    assert_eq!(input.get::<String>("text").unwrap().len(), 3150);
    let errors: Table = game.lua.globals().get("UIErrorsFrame").unwrap();
    assert_eq!(
        errors.get::<Vec<String>>("lines").unwrap(),
        ["Too long to send."]
    );
    assert!(
        game.shots() <= 1,
        "only the login hello, got {}",
        game.shots()
    );
}

#[test]
fn a_stored_message_that_no_longer_fits_becomes_an_error_and_stops_retrying() {
    let game = Game::start();
    game.send("short");
    game.run("local ns = ... ns.Store.db.chats[1].cwd = string.rep('d', 3200)");
    game.advance(300.0);

    assert!(game.shots() <= 3, "got {} shots", game.shots());
    let history: Table = game
        .db()
        .get::<Table>("chats")
        .unwrap()
        .get::<Table>(1)
        .unwrap()
        .get("history")
        .unwrap();
    let last: Table = history.get(history.raw_len()).unwrap();
    assert_eq!(last.get::<String>("role").unwrap(), "error");
    assert_eq!(last.get::<String>("text").unwrap(), "Too long to send.");
}
