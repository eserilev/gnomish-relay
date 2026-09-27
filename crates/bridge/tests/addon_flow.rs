//! The whole addon in a fake game: strips out, slots in, the outbox, restore, and the window.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fmt::Write;

use bridge::activity::text_hash;
use bridge::agent::{Agent, Control, Echo};
use bridge::config::{Permission, Policy};
use bridge::desktop::{Notice, Prompted, Waiting};
use bridge::receive::{KeySet, StripKey, receive};
use bridge::relay::{Folders, Relay};
use bridge::settings_list::{BridgeSettings, StorySettings, settings_reply};
use bridge::strip::{self, Image};
use common::{Bits, load_into, lua, repo_file, screenshot_png};
use hmac::{Hmac, Mac};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::cell::decode_cells;
use protocol::frame::{decode_frame, signed_len};
use protocol::live::{
    OptionKind, PermOption, Progress, Request as LiveRequest, live_body, prepare_progress,
    prepare_requests,
};
use protocol::markdown::render_markdown;
use protocol::record::{Record, parse_records};
use protocol::restore::{Chat, Entry, Role, prepare_restore, restore_body};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};
use sha2::Sha256;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const FILES: &[&str] = &[
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
    "Folders.lua",
    "Browser.lua",
    "BridgeSettings.lua",
    "SettingsTab.lua",
    "DiagTab.lua",
    "Window.lua",
    "Popup.lua",
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
        Game::boot(None, before)
    }

    /// `/reload`: WoW saves `GnomishRelayDB` as Lua text, and a new UI session loads it.
    /// The clock of the game goes on.
    fn reload(&self) -> Game {
        self.reload_after(0)
    }

    /// A logout, and a login `gap` seconds later.
    fn reload_after(&self, gap: i64) -> Game {
        let saved = self.saved_variables();
        let clock = self.run("return time()").as_integer().unwrap();
        Game::boot(Some(saved), |wow| wow.set("epoch", clock + gap).unwrap())
    }

    fn saved_variables(&self) -> String {
        self.wow
            .get::<Function>("Save")
            .unwrap()
            .call("GnomishRelayDB")
            .unwrap()
    }

    fn boot(saved: Option<String>, before: impl FnOnce(&Table)) -> Game {
        let lua = lua(Bits::Unsigned);
        let api: Table = lua
            .load(repo_file("addon/tests/api.lua"))
            .set_name("api.lua")
            .call(())
            .unwrap();
        let wow: Table = lua
            .load(repo_file("addon/tests/wow.lua"))
            .set_name("wow.lua")
            .call(api)
            .unwrap();
        before(&wow);
        if let Some(saved) = saved {
            lua.load(saved).set_name("GnomishRelay.lua").exec().unwrap();
        }
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
        let body = slot_body(App::Relay, 1_790_211_079, &prepare_replies(replies));
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
    }

    fn printed(&self) -> Vec<String> {
        self.wow.get::<Vec<String>>("printed").unwrap()
    }
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

fn loaded_slots(game: &Game) -> usize {
    game.wow
        .get::<Table>("loaded")
        .unwrap()
        .pairs::<String, bool>()
        .count()
}

/// One shown object of the transcript, from `wow.Drawn`.
struct Drawn {
    kind: String,
    text: Option<String>,
    y: i64,
    object: Table,
}

/// Every shown object of the transcript, top to bottom.
fn transcript(game: &Game) -> Vec<Drawn> {
    let root: Table = game.lua.globals().get("GnomishRelayTranscript").unwrap();
    let drawn: Table = game
        .wow
        .get::<Function>("Drawn")
        .unwrap()
        .call(root)
        .unwrap();
    drawn
        .sequence_values::<Table>()
        .map(|d| {
            let d = d.unwrap();
            Drawn {
                kind: d.get("kind").unwrap(),
                text: d.get("text").unwrap(),
                y: d.get("y").unwrap(),
                object: d.get("object").unwrap(),
            }
        })
        .collect()
}

fn texts(drawn: &[Drawn]) -> Vec<String> {
    drawn.iter().filter_map(|d| d.text.clone()).collect()
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
            && f.contains(&"level=auto-edit".into())
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
fn an_unacknowledged_message_goes_to_the_outbox_as_a_signed_frame() {
    let game = Game::start();
    game.send("anyone there?");
    game.advance(130.0);

    assert_eq!(game.shots(), 3);
    let outbox: Table = game.db().get("outbox").unwrap();
    assert_eq!(outbox.raw_len(), 1);
    assert!(
        game.run("local ns = ... return ns.Transport.NeedsReload()")
            .as_boolean()
            .unwrap()
    );
    let frames = bridge::saved::frames(&game.saved_variables());
    assert!(!frames.is_empty());
    let keys = KeySet::new(StripKey::from_hex(&common::hex(KEY)).unwrap(), None).unwrap();
    for frame in frames {
        let (_, records) = receive(&frame, &keys, 1_790_211_209).unwrap();
        assert_eq!(records[0].text, b"anyone there?");
    }
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
    let entry = |role, text: &str| Entry {
        role,
        id: 5,
        text: text.as_bytes().to_vec(),
    };
    let chats = [Chat {
        id: b"oldchat01".to_vec(),
        name: b"lighthouse".to_vec(),
        agent: b"claude".to_vec(),
        cwd: b"Code/x".to_vec(),
        history: vec![entry(Role::User, "hi"), entry(Role::Agent, "hello")],
    }];
    let restore = restore_body(App::Relay, token.as_bytes(), &prepare_restore(&chats));
    game.wow
        .set("restore", game.lua.create_string(restore).unwrap())
        .unwrap();
    game.run("local ns = ... ns.Transport.Poll() ns.Transport.Poll()");
    game.advance(2.0);

    let chats: Table = game.db().get("chats").unwrap();
    assert_eq!(chats.raw_len(), 1);
    let chat: Table = chats.get(1).unwrap();
    assert_eq!(chat.get::<String>("cwd").unwrap(), "Code/x");
    assert_eq!(last_entry(&game).get::<String>("text").unwrap(), "hello");
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
    game.run("local ns = ... ns.Transport.Poll() ns.Transport.Poll()");
    let told = game
        .printed()
        .iter()
        .filter(|l| l.starts_with("Gnomish Relay: slots are missing."))
        .count();
    assert_eq!(told, 1, "one line, not one per poll");
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

    assert_eq!(
        texts(&transcript(&game)),
        [
            "|cff69ccf0[You]|r: show ||cffff0000 red",
            "|cffff7d0a[Claude]|r: Here:\n    |cffb8c8b8let x = 1;|r\nDone.",
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
    let hex = KEY.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    let keys = KeySet::new(StripKey::from_hex(&hex).unwrap(), None).unwrap();
    let tag_checks = |bytes: &[u8]| receive(bytes, &keys, now).is_ok();
    let bytes = strip::read_with(&Image::from_png(&png).unwrap(), tag_checks)
        .expect("the bridge finds the strip");
    let (_, records) = receive(&bytes, &keys, now).unwrap();
    let mut relay = Relay::new(Policy {
        folders: Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    });
    relay.on_frame(&records, now);
    let job = relay.next_job().expect("the message is queued");
    relay.finish(&job, Echo.run(&job, &Control::default()).reply);
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
    assert_eq!(
        last.get::<String>("text").unwrap(),
        "\x1bM1\np\x1fecho: ping the relay\n"
    );
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
fn a_change_to_saved_data_after_a_send_does_not_change_the_strip() {
    let game = Game::start();
    game.send("the real task");
    game.advance(1.0);
    game.run(
        "local ns = ... local chat = ns.Store.db.chats[1]
         chat.cwd = '/etc' chat.history[1].text = 'rm -rf ~'",
    );
    game.advance(41.0);

    let retry = game.last_strip();
    assert_eq!(retry[0].text, b"the real task");
    assert_eq!(retry[0].cwd, b"");
}

#[test]
fn after_a_reload_the_stored_signed_frame_goes_out_as_it_is() {
    let game = Game::start();
    game.send("survive the reload");
    let game = game.reload();
    game.run("local ns = ... ns.Store.db.chats[1].history[1].text = 'changed'");
    game.advance(2.0);

    let records = (1..=game.shots())
        .flat_map(|n| game.strip(n))
        .collect::<Vec<_>>();
    let message = records
        .iter()
        .find(|r| r.id != 0)
        .expect("the stored frame");
    assert_eq!(message.text, b"survive the reload");
}

fn last_entry(game: &Game) -> Table {
    let chats: Table = game.db().get("chats").unwrap();
    let history: Table = chats.get::<Table>(1).unwrap().get("history").unwrap();
    history.get(history.raw_len()).unwrap()
}

#[test]
fn an_outbox_frame_that_the_bridge_never_takes_asks_to_be_sent_again() {
    let game = Game::start();
    game.send("stuck in the outbox");
    let game = game.reload();
    game.advance(300.0);
    assert_eq!(
        last_entry(&game).get::<String>("text").unwrap(),
        "Not sent. Send it again."
    );
}

#[test]
fn a_stored_frame_too_old_at_login_asks_to_be_sent_again() {
    let game = Game::start();
    game.send("sent before a long break");
    game.advance(1.0);
    let game = game.reload_after(300);
    game.advance(2.0);
    assert_eq!(
        last_entry(&game).get::<String>("text").unwrap(),
        "Not sent. Send it again."
    );
}

#[test]
fn polls_follow_the_schedule_after_a_send() {
    let game = Game::start();
    game.send("go");
    let mut polls = Vec::new();
    for second in 1..=30 {
        game.advance(1.0);
        polls.push((second, loaded_slots(&game)));
    }
    let at = |s: usize| polls[s - 1].1;
    assert_eq!((at(4), at(5)), (0, 1), "first poll at 5 s");
    assert_eq!((at(9), at(10)), (1, 2), "second poll at 10 s");
    assert_eq!((at(15), at(16)), (2, 3), "third poll at 16 s");
    assert_eq!((at(23), at(24)), (3, 4), "fourth poll at 24 s");
}

#[test]
fn with_no_message_open_the_addon_polls_every_ten_minutes() {
    let game = Game::start();
    game.advance(6.0);
    let after_login = loaded_slots(&game);
    game.advance(590.0);
    assert_eq!(loaded_slots(&game), after_login);
    game.advance(20.0);
    assert_eq!(loaded_slots(&game), after_login + 1);
}

#[test]
fn stop_sends_a_stop_record_for_the_chat() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("long task");
    game.advance(1.0);
    game.publish(&[reply(
        &game.chat_id(),
        first_message_id(&game),
        Status::Working,
        "",
    )]);
    game.advance(5.0);

    let stop: Table = game.lua.globals().get("GnomishRelayStop").unwrap();
    assert!(
        stop.get::<bool>("shown").unwrap(),
        "Stop shows while the agent works"
    );
    stop.get::<Table>("scripts")
        .unwrap()
        .get::<Function>("OnClick")
        .unwrap()
        .call::<()>(stop.clone())
        .unwrap();
    game.advance(1.0);

    let records = game.last_strip();
    let stop_record = records
        .iter()
        .find(|r| flags(r).contains(&"stop".into()))
        .expect("a stop record");
    assert_eq!(stop_record.chat, game.chat_id().as_bytes());
}

/// A live file, written the way the bridge writes it.
fn live(progress: &[Progress], requests: &[LiveRequest]) -> Vec<u8> {
    live_body(
        App::Relay,
        &prepare_progress(progress),
        &prepare_requests(requests),
    )
}

fn texts_of(game: &Game, kind: &str) -> Vec<String> {
    let frames: Table = game.wow.get("frames").unwrap();
    frames
        .sequence_values::<Table>()
        .filter_map(Result::ok)
        .filter(|f| {
            f.get::<String>("kind").unwrap() == kind && f.get::<bool>("shown").unwrap_or(false)
        })
        .filter_map(|f| f.get::<Option<String>>("text").unwrap())
        .collect()
}

#[test]
fn the_activity_panel_shows_the_progress_of_a_working_agent() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("build it");
    game.advance(1.0);
    let id = first_message_id(&game);
    game.publish(&[reply(&game.chat_id(), id, Status::Working, "")]);
    let progress = Progress {
        chat: game.chat_id().into_bytes(),
        id,
        lines: vec![b"edit src/main.rs".to_vec(), b"$ cargo test".to_vec()],
    };
    game.wow
        .set(
            "live",
            game.lua.create_string(live(&[progress], &[])).unwrap(),
        )
        .unwrap();
    game.advance(5.0);

    let texts = texts_of(&game, "FontString");
    assert!(texts.contains(&"edit src/main.rs".to_owned()), "{texts:?}");
    assert!(texts.contains(&"$ cargo test".to_owned()), "{texts:?}");
}

fn show_progress(game: &Game, lines: &[&[u8]]) {
    let id = first_message_id(game);
    game.publish(&[reply(&game.chat_id(), id, Status::Working, "")]);
    let progress = Progress {
        chat: game.chat_id().into_bytes(),
        id,
        lines: lines.iter().map(|l| l.to_vec()).collect(),
    };
    game.wow
        .set(
            "live",
            game.lua.create_string(live(&[progress], &[])).unwrap(),
        )
        .unwrap();
    game.advance(5.0);
}

#[test]
fn the_header_shows_the_level_that_the_bridge_used_not_the_one_the_chat_asked_for() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("build it");
    game.advance(1.0);
    show_progress(&game, &[b"Level: ask (config)", b"edit src/main.rs"]);

    let texts = texts_of(&game, "FontString");
    assert!(
        texts.contains(&"Claude · ask (config)".to_owned()),
        "{texts:?}"
    );
    assert!(
        !texts.contains(&"Claude · auto-edit".to_owned()),
        "{texts:?}"
    );
}

#[test]
fn a_level_line_that_is_not_first_does_not_change_the_header() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("build it");
    game.advance(1.0);
    show_progress(&game, &[b"edit src/main.rs", b"Level: full-auto"]);

    let texts = texts_of(&game, "FontString");
    assert!(
        texts.contains(&"Claude · auto-edit".to_owned()),
        "{texts:?}"
    );
}

/// Puts a working record and a desktop line of the bridge into every slot.
fn wait_on_desktop(game: &Game, line: &str) {
    let id = first_message_id(game);
    game.publish(&[reply(&game.chat_id(), id, Status::Working, "")]);
    let progress = Progress {
        chat: game.chat_id().into_bytes(),
        id,
        lines: vec![
            b"Level: auto-edit".to_vec(),
            line.as_bytes().to_vec(),
            b"Read ~/.ssh/id_rsa".to_vec(),
        ],
    };
    game.wow
        .set(
            "live",
            game.lua.create_string(live(&[progress], &[])).unwrap(),
        )
        .unwrap();
}

fn whispers_with(game: &Game, text: &str) -> usize {
    game.printed()
        .iter()
        .filter(|l| l.contains("whispers:") && l.contains(text))
        .count()
}

const WAIT: &str = "Desktop: wait a1b2c3d4e5f6 dialog";

#[test]
fn a_desktop_request_shows_a_row_and_no_popup() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("read my key");
    game.advance(1.0);
    wait_on_desktop(&game, WAIT);
    game.run("local ns = ... ns.Transport.Poll()");

    let texts = texts_of(&game, "FontString");
    assert!(
        texts.contains(&"Approve on desktop".to_owned()),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.starts_with("Desktop:")),
        "{texts:?}"
    );
    let popup = game.run("return GnomishRelayPopup and GnomishRelayPopup:IsShown() or false");
    assert_eq!(
        popup,
        Value::Boolean(false),
        "no popup for a desktop request"
    );
}

#[test]
fn the_desktop_row_changes_on_each_answer() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("read my key");
    game.advance(1.0);
    for (state, row) in [
        ("wait", "Approve on desktop"),
        ("approved", "Approved on desktop"),
        ("denied", "Denied on desktop"),
        ("none", "No answer on desktop"),
    ] {
        wait_on_desktop(&game, &format!("Desktop: {state} a1b2c3d4e5f6 dialog"));
        game.run("local ns = ... ns.Transport.Poll()");
        let texts = texts_of(&game, "FontString");
        assert!(texts.contains(&row.to_owned()), "{state}: {texts:?}");
    }
}

#[test]
fn the_whisper_line_prints_once_per_desktop_request_also_after_a_reload() {
    let game = Game::start();
    game.send("read my key");
    game.advance(1.0);
    wait_on_desktop(&game, WAIT);
    game.advance(30.0);
    assert_eq!(whispers_with(&game, "] Approve on your desktop."), 1);

    let game = game.reload();
    wait_on_desktop(&game, WAIT);
    game.advance(30.0);
    assert_eq!(whispers_with(&game, "Approve on your desktop"), 0);

    wait_on_desktop(&game, "Desktop: wait 0123456789ab dialog");
    game.advance(10.0);
    assert_eq!(whispers_with(&game, "] Approve on your desktop."), 1);
}

#[test]
fn with_no_dialog_the_whisper_line_names_the_command_and_a_raise_names_the_level() {
    let game = Game::start();
    game.send("read my key");
    game.advance(1.0);
    wait_on_desktop(&game, "Desktop: wait a1b2c3d4e5f6 command");
    game.run("local ns = ... ns.Transport.Poll()");
    wait_on_desktop(&game, "Desktop: wait 0123456789ab dialog raise auto-edit");
    game.run("local ns = ... ns.Transport.Poll()");

    assert_eq!(
        whispers_with(&game, "] Run: gnomish-relay approve a1b2c3d4e5f6"),
        1
    );
    assert_eq!(
        whispers_with(
            &game,
            "] Approve on your desktop: let Claude work at auto-edit."
        ),
        1
    );
}

#[test]
fn a_desktop_line_in_the_wrong_place_or_shape_is_only_a_step() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("read my key");
    game.advance(1.0);
    show_progress(&game, &[b"Level: auto-edit", b"edit a.rs", WAIT.as_bytes()]);
    wait_on_desktop(&game, "Desktop: wait a1b2 dialog");
    game.run("local ns = ... ns.Transport.Poll()");

    let texts = texts_of(&game, "FontString");
    assert!(
        !texts.contains(&"Approve on desktop".to_owned()),
        "{texts:?}"
    );
    assert_eq!(whispers_with(&game, "desktop"), 0);
}

/// The seconds between the polls of the game, from a clock that ticks each second.
fn poll_gaps(game: &Game, seconds: usize) -> Vec<usize> {
    let mut at = Vec::new();
    let mut last = loaded_slots(game);
    for second in 1..=seconds {
        game.advance(1.0);
        let now = loaded_slots(game);
        if now != last {
            at.push(second);
            last = now;
        }
    }
    at.windows(2).map(|w| w[1] - w[0]).collect()
}

#[test]
fn a_desktop_wait_polls_every_five_seconds_and_stops_after_24_polls() {
    let game = Game::start();
    game.send("read my key");
    game.advance(1.0);
    let id = first_message_id(&game);
    game.publish(&[reply(&game.chat_id(), id, Status::Working, "")]);
    game.advance(400.0);
    wait_on_desktop(&game, WAIT);

    let gaps = poll_gaps(&game, 300);

    let fast = gaps.iter().take_while(|g| **g == 5).count();
    assert_eq!(
        fast, 23,
        "24 polls in all while the request waits: {gaps:?}"
    );
    assert_eq!(gaps[fast], 60, "then the normal schedule: {gaps:?}");
}

/// The records of the last strip, read by the bridge from its screenshot.
fn records_of_last_shot(game: &Game, now: u32) -> Vec<Record> {
    let png = screenshot_png(&game.shot_rows(game.shots()));
    let keys = KeySet::new(StripKey::from_hex(&common::hex(KEY)).unwrap(), None).unwrap();
    let tag_checks = |bytes: &[u8]| receive(bytes, &keys, now).is_ok();
    let bytes = strip::read_with(&Image::from_png(&png).unwrap(), tag_checks)
        .expect("the bridge finds the strip");
    receive(&bytes, &keys, now).unwrap().1
}

#[test]
fn a_new_message_in_the_game_ends_the_wait_on_the_desktop() {
    let game = Game::start();
    let now = 1_790_211_080;
    let mut relay = Relay::new(Policy {
        folders: Folders {
            roots: vec![b"/home/x".to_vec()],
            base: b"/home/x".to_vec(),
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    });
    game.send("read my key");
    game.advance(1.0);
    relay.on_frame(&records_of_last_shot(&game, now), now);
    let job = relay.next_job().unwrap();
    relay.desktop(
        &job.chat,
        job.id,
        Notice {
            id: "a1b2c3d4e5f6".into(),
            prompted: Prompted::Dialog,
            waiting: Waiting::Open,
            raise: None,
        },
    );
    game.wow
        .set("body", game.lua.create_string(relay.body(now)).unwrap())
        .unwrap();
    game.wow
        .set("live", game.lua.create_string(relay.live_file()).unwrap())
        .unwrap();
    game.advance(5.0);
    assert_eq!(whispers_with(&game, "Approve on your desktop."), 1);

    game.send("no, do this instead");
    game.advance(1.0);
    relay.on_frame(&records_of_last_shot(&game, now), now);

    assert_eq!(relay.take_interrupts(), [job.chat]);
}

/// A request with an "allow" option that the agent labels "Reject".
fn request(game: &Game, text: &str) -> LiveRequest {
    let option = |id: &[u8], kind, label: &[u8]| PermOption {
        id: id.to_vec(),
        kind,
        label: label.to_vec(),
    };
    LiveRequest {
        request: b"p1a2b".to_vec(),
        chat: game.chat_id().into_bytes(),
        id: first_message_id(game),
        text: text.as_bytes().to_vec(),
        options: vec![
            option(b"o1", OptionKind::AllowOnce, b"Reject"),
            option(b"o2", OptionKind::RejectOnce, b"Allow"),
        ],
    }
}

fn ask(game: &Game, text: &str) {
    let file = live(&[], &[request(game, text)]);
    game.wow
        .set("live", game.lua.create_string(file).unwrap())
        .unwrap();
    game.run("local ns = ... ns.Transport.Poll()");
}

#[test]
fn a_permission_request_shows_the_honest_text_and_buttons_by_kind() {
    let game = Game::start();
    game.send("clean up");
    ask(
        &game,
        "rm -rf build\nthe agent says: clean |cffff0000the build",
    );

    let texts = texts_of(&game, "FontString");
    assert!(
        texts.contains(&"rm -rf build\nthe agent says: clean ||cffff0000the build".to_owned()),
        "the text shows as it is, with no color code: {texts:?}"
    );
    let buttons = texts_of(&game, "Button");
    assert_eq!(
        buttons[buttons.len() - 2..],
        ["Allow once", "Reject"],
        "{buttons:?}"
    );
}

#[test]
fn a_click_sends_the_answer_with_the_hash_of_the_text_once() {
    let game = Game::start();
    game.send("clean up");
    let text = "rm -rf build\nthe agent says: clean the build";
    ask(&game, text);
    let shots = game.shots();
    game.run("GnomishRelayPopupButton1:Click()");
    game.advance(5.0);

    let expected = format!("perm=p1a2b:o1:{}", text_hash(text.as_bytes()));
    let answers = (shots + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .filter(|r| flags(r).contains(&expected))
        .count();
    assert!(answers >= 1, "no strip carried {expected}");
    assert!(
        game.run("local ns = ... return ns.Transport.Request()")
            .is_nil(),
        "answered once"
    );
}

#[test]
fn a_chat_keeps_its_last_200_entries() {
    let game = Game::start();
    game.run("local ns = ... local chat = ns.Store.NewChat() for i = 1, 205 do ns.Store.AddMessage(chat, 'm' .. i) end");
    let history: Table = game
        .db()
        .get::<Table>("chats")
        .unwrap()
        .get::<Table>(1)
        .unwrap()
        .get("history")
        .unwrap();
    assert_eq!(history.raw_len(), 200);
    assert_eq!(
        history
            .get::<Table>(1)
            .unwrap()
            .get::<String>("text")
            .unwrap(),
        "m6"
    );
}

#[test]
fn a_restore_bundle_skips_a_bad_chat_id_a_bad_agent_and_a_bad_role() {
    let game = Game::start();
    game.run(
        "local ns = ... ns.Store.MergeChats({
            { id = '../../x', name = 'evil' },
            { id = 'good01', name = 'ok', agent = '|cffff0000fake',
              history = { { role = 'system', id = 1, text = 'x' }, { role = 'user', id = 2, text = 'y' } } },
        })",
    );
    let chats: Table = game.db().get("chats").unwrap();
    assert_eq!(chats.raw_len(), 1);
    let chat: Table = chats.get(1).unwrap();
    assert_eq!(chat.get::<String>("id").unwrap(), "good01");
    assert_eq!(chat.get::<String>("agent").unwrap(), "claude");
    assert_eq!(chat.get::<Table>("history").unwrap().raw_len(), 1);
}

#[test]
fn when_every_slot_is_used_the_addon_stops_polling_and_asks_for_a_reload() {
    let all = |wow: &Table| {
        let loaded: Table = wow.get("loaded").unwrap();
        for n in 1..=1000 {
            loaded.set(format!("GnomishRelay_S{n:04}"), true).unwrap();
        }
    };
    let game = Game::start_with(all);
    game.run("local ns = ... ns.Transport.Poll()");
    assert_eq!(
        game.run("local ns = ... return ns.Transport.SlotsLeft()")
            .as_integer()
            .unwrap(),
        0
    );
    assert!(
        game.run("local ns = ... return ns.Transport.NeedsReload()")
            .as_boolean()
            .unwrap()
    );
}

#[test]
fn the_fake_game_refuses_what_the_client_does_not_have() {
    let game = Game::start();
    let call = |code: &str| game.lua.load(code).exec().map_err(|e| e.to_string());

    let removed = call("CreateFrame('Frame'):SetBackdrop({})").unwrap_err();
    assert!(
        removed.contains("Frame has no SetBackdrop in WoW Forever"),
        "{removed}"
    );
    assert!(call("SetPortraitToTexture(nil, '')").is_err());
    assert!(
        call("CreateFrame('Frame', nil, UIParent, 'PortraitFrameTemplate'):SetTitle('x')").is_ok()
    );
    assert!(call("CreateFrame('Frame'):SetTitle('x')").is_err());
}

#[test]
fn a_client_without_a_required_function_turns_the_relay_off() {
    let game = Game::start();
    game.run("Screenshot = nil");
    game.fire("PLAYER_LOGIN", ());
    assert!(
        game.printed().contains(
            &"Gnomish Relay: this game version has no Screenshot. The relay is off.".into()
        ),
        "{:?}",
        game.printed()
    );
}

#[test]
fn every_required_function_is_in_the_forever_api() {
    let game = Game::start();
    let api: Table = game
        .lua
        .load(repo_file("addon/tests/api.lua"))
        .call(())
        .unwrap();
    let known: Vec<String> = api.get("globals").unwrap();
    let required: Vec<String> = game
        .run("local ns = ... local out = {} for _, r in ipairs(ns.Health.Required()) do table.insert(out, r[1]) end return out")
        .as_table()
        .unwrap()
        .sequence_values()
        .map(Result::unwrap)
        .collect();
    for name in required {
        assert!(
            known.contains(&name),
            "{name} is not in addon/tests/api.lua"
        );
    }
}

#[test]
fn a_strip_reports_the_build_and_the_health_of_both_channels() {
    let game = Game::start();
    game.advance(6.0);
    game.send("health check");
    game.advance(1.0);

    let f = flags(&game.last_strip()[0]);
    for flag in ["build=70009", "out=shot", "in=slots", "ver=1"] {
        assert!(f.contains(&flag.into()), "{f:?}");
    }
}

#[test]
fn blocked_screenshots_show_one_line_and_mark_the_window() {
    let game = Game::start_with(|wow| wow.set("shotsBlocked", true).unwrap());
    game.advance(30.0);

    let blocked = game
        .printed()
        .iter()
        .filter(|l| *l == "Gnomish Relay: screenshots are blocked.")
        .count();
    assert_eq!(blocked, 1);
    let problem: String = game
        .run("local ns = ... return ns.Transport.Problem()")
        .to_string()
        .unwrap();
    assert_eq!(problem, "blocked");
}

#[test]
fn an_addon_with_no_key_asks_for_setup() {
    let game = Game::start();
    game.run("local ns = ... ns.key = nil");
    game.fire("PLAYER_LOGIN", ());
    assert!(
        game.printed().contains(
            &"Gnomish Relay: run gnomish-relay setup. Get it at github.com/eserilev/gnomish-relay"
                .into()
        ),
        "{:?}",
        game.printed()
    );
}

#[test]
fn a_silent_bridge_shows_one_line_a_minute_after_login() {
    let game = Game::start();
    game.advance(59.0);
    let silent = |game: &Game| {
        game.printed()
            .iter()
            .filter(|l| *l == "Gnomish Relay: bridge not running.")
            .count()
    };
    assert_eq!(silent(&game), 0);
    game.advance(120.0);
    assert_eq!(silent(&game), 1);
}

#[test]
fn a_bridge_that_answers_gets_no_line() {
    let game = Game::start();
    game.publish(&[]);
    game.advance(120.0);
    assert!(
        !game
            .printed()
            .iter()
            .any(|l| l == "Gnomish Relay: bridge not running.")
    );
}

#[test]
fn a_screenshot_of_the_player_does_not_end_our_strip() {
    let game = Game::start();
    game.advance(10.0);
    let shots = game.shots();
    game.send("keep the strip");
    // The player's own screenshot finishes before ours is taken.
    game.fire("SCREENSHOT_SUCCEEDED", ());
    game.advance(2.0);

    assert!(game.shots() > shots, "our screenshot was taken");
    for n in shots + 1..=game.shots() {
        assert!(
            !game.shot_rows(n).is_empty(),
            "screenshot {n} has no strip in it"
        );
    }
    let sent = (shots + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .any(|r| r.text == b"keep the strip");
    assert!(sent);
}

fn right_click(game: &Game, frame: &str) {
    let frame: Table = game.lua.globals().get(frame).unwrap();
    frame
        .get::<Table>("scripts")
        .unwrap()
        .get::<Function>("OnClick")
        .unwrap()
        .call::<()>((frame.clone(), "RightButton"))
        .unwrap();
}

fn chat_count(game: &Game) -> usize {
    game.db().get::<Table>("chats").unwrap().raw_len()
}

#[test]
fn a_right_click_and_delete_removes_the_chat_and_tells_the_bridge() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("hi");
    game.advance(1.0);
    let chat = game.chat_id();
    game.publish(&[reply(&chat, first_message_id(&game), Status::Done, "hello")]);
    game.advance(5.0);

    right_click(&game, "GnomishRelayTile1");
    let confirm: Table = game.lua.globals().get("GnomishRelayConfirm").unwrap();
    assert!(confirm.get::<bool>("shown").unwrap());
    game.run("GnomishRelayConfirmDelete:Click()");
    game.advance(1.0);

    assert_eq!(chat_count(&game), 0);
    let records = game.last_strip();
    let delete = records
        .iter()
        .find(|r| flags(r).contains(&"d".into()))
        .expect("a delete record");
    assert_eq!(delete.chat, chat.as_bytes());
}

#[test]
fn cancel_keeps_the_chat() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("hi");

    right_click(&game, "GnomishRelayTile1");
    game.run("GnomishRelayConfirmCancel:Click()");

    assert_eq!(chat_count(&game), 1);
    let confirm: Table = game.lua.globals().get("GnomishRelayConfirm").unwrap();
    assert!(!confirm.get::<bool>("shown").unwrap());
}

#[test]
fn a_right_click_on_new_chat_asks_nothing() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");

    right_click(&game, "GnomishRelayTile1");

    let confirm: Table = game.lua.globals().get("GnomishRelayConfirm").unwrap();
    assert!(!confirm.get::<bool>("shown").unwrap());
}

#[test]
fn a_delete_while_the_bridge_is_off_goes_out_again_after_a_reload() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("hi");
    let chat = game.chat_id();
    game.run("local ns = ... ns.Transport.Delete(ns.Store.Chats()[1])");
    game.advance(1.0);
    assert_eq!(
        game.db().get::<Table>("forget").unwrap().raw_len(),
        1,
        "no body came, so the bridge can be off"
    );

    let game = game.reload();
    game.publish(&[]);
    game.advance(601.0);
    let deletes: Vec<Record> = (1..=game.shots())
        .flat_map(|n| game.strip(n))
        .filter(|r| flags(r).contains(&"d".into()))
        .collect();
    assert!(!deletes.is_empty());
    assert!(deletes.iter().all(|r| r.chat == chat.as_bytes()));
    assert_eq!(game.db().get::<Table>("forget").unwrap().raw_len(), 0);
}

const LIST: &str = "claude\ta1\t7200\t0\t\tapp\tapp\tFix bugs\n\
                    claude\tb2\t10\t1\t\tapp\tapp\tLive work\n\
                    codex\tc3\t90000\t0\t\t../w\tw\tOther\n\
                    Bad Agent\td4\t1\t0\t\tapp\tapp\tSkipped\n\
                    claude\te;5\t1\t0\t\tapp\tapp\tSkipped too";

fn text_of(game: &Game, code: &str) -> String {
    game.run(&format!("return {code}"))
        .to_string()
        .unwrap_or_default()
}

/// Opens the window, clicks Resume, and answers the list request with `list`.
fn open_sessions(game: &Game, list: &str, status: Status) -> u32 {
    game.run("local ns = ... ns.Window.Open()");
    let resume_tile = format!("GnomishRelayTile{}", chat_count(game) + 2);
    let before = game.shots();
    game.run(&format!("{resume_tile}:Click()"));
    game.advance(2.0);
    let request = (before + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .find(|r| flags(r).contains(&"list".into()))
        .expect("a list request");
    assert_eq!(request.chat, b"relay");
    game.publish(&[reply("relay", request.id, status, list)]);
    game.advance(5.0);
    request.id
}

#[test]
fn resume_lists_the_sessions_by_folder_and_skips_bad_lines() {
    let game = Game::start();
    open_sessions(&game, LIST, Status::Done);

    let rows: Vec<String> = (1..=6)
        .map(|i| {
            text_of(
                &game,
                &format!(
                    "GnomishRelayPick{i}:IsShown() and GnomishRelayPick{i}.text:GetText() or ''"
                ),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            "|cffffd100app|r",
            "   Fix bugs",
            "   Live work",
            "|cffffd100w|r",
            "   Other",
            ""
        ]
    );
    assert_eq!(
        text_of(&game, "GnomishRelayPick2.right:GetText()"),
        "Claude  2 h"
    );
    assert!(text_of(&game, "GnomishRelayPick3.right:GetText()").contains("open"));
    assert_eq!(
        text_of(&game, "GnomishRelayPick5.right:GetText()"),
        "Codex  1 d"
    );
}

#[test]
fn the_list_reply_is_reported_as_read() {
    let game = Game::start();
    let id = open_sessions(&game, LIST, Status::Done);
    game.send("hi");
    game.advance(1.0);
    let first = &game.last_strip()[0];
    assert!(
        flags(first).contains(&format!("read={id}")),
        "{:?}",
        flags(first)
    );
}

#[test]
fn a_click_on_a_session_opens_a_chat_that_asks_to_attach_it() {
    let game = Game::start();
    open_sessions(&game, LIST, Status::Done);

    game.run("GnomishRelayPick2:Click()");
    game.advance(1.0);

    let record = game
        .last_strip()
        .into_iter()
        .find(|r| r.chat == game.chat_id().as_bytes())
        .unwrap();
    let sent = flags(&record);
    assert!(sent.contains(&"attach=a1".into()), "{sent:?}");
    assert!(
        !sent.contains(&"n".into()),
        "a resumed chat never starts a new session"
    );
    assert_eq!(record.cwd, b"app");
    assert!(record.text.is_empty());
    assert_eq!(text_of(&game, "GnomishRelayDB.chats[1].name"), "Fix bugs");
}

#[test]
fn the_attach_reply_shows_the_last_exchange_and_later_messages_resume() {
    let game = Game::start();
    open_sessions(&game, LIST, Status::Done);
    game.run("GnomishRelayPick2:Click()");
    game.advance(1.0);
    let chat = game.chat_id();
    game.publish(&[reply(
        &chat,
        first_message_id(&game),
        Status::Done,
        "fix the bugs\nAll fixed.",
    )]);
    game.advance(5.0);

    let lines = texts(&transcript(&game));
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].contains("Resumed \"Fix bugs\""));
    assert!(lines[1].contains("[You]") && lines[1].contains("fix the bugs"));
    assert!(lines[2].contains("All fixed."));
    assert!(
        game.printed().iter().all(|p| !p.contains("All fixed")),
        "no whisper for an attach"
    );

    game.send("go on");
    game.advance(1.0);
    let record = game
        .last_strip()
        .into_iter()
        .find(|r| r.text == b"go on")
        .unwrap();
    let sent = flags(&record);
    assert!(
        sent.iter().all(|f| !f.starts_with("attach=") && f != "n"),
        "{sent:?}"
    );
    assert_eq!(record.cwd, b"app");
}

#[test]
fn a_session_that_has_a_chat_opens_that_chat() {
    let game = Game::start();
    game.send("hi");
    let chat = game.chat_id();
    open_sessions(
        &game,
        &format!("claude\ta1\t7200\t0\t{chat}\tapp\tapp\tFix bugs"),
        Status::Done,
    );

    game.run("GnomishRelayPick2:Click()");

    assert_eq!(chat_count(&game), 1);
    assert_eq!(text_of(&game, "GnomishRelayDB.selected"), chat);
}

#[test]
fn a_failed_list_shows_its_error() {
    let game = Game::start();
    open_sessions(&game, "claude: not logged in", Status::Error);
    assert_eq!(
        text_of(&game, "GnomishRelayPick1:IsShown() and 'shown' or 'hidden'"),
        "hidden"
    );
    assert!(text_of(&game, "GnomishRelayPickNote:GetText()").contains("not logged in"));
}

/// The default folder, then one line per folder: `parent \t name \t mark`.
const TREE: &str = "~/Code\n0\t~/Code\t\n1\tPersonal\t\n1\tscratch\t\n2\tgnomish-relay\tg\n2\ttimeways\tg\n4\tcrates\t\nbroken line";

const NEW_FOLDER_ROW: &str = "|cff9fe39fNew folder|r";

fn click_new_chat(game: &Game) {
    game.run("local ns = ... ns.Window.Open()");
    game.run(&format!("GnomishRelayTile{}:Click()", chat_count(game) + 1));
}

/// Clicks the folder button, and answers the folder list request with `tree`.
fn open_browser_with(game: &Game, tree: &str, status: Status) -> u32 {
    let before = game.shots();
    game.run("GnomishRelayFolderButton:Click()");
    game.advance(2.0);
    let request = (before + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .find(|r| flags(r).contains(&"list=folders".into()))
        .expect("a folder list request");
    assert_eq!(request.chat, b"folders");
    game.publish(&[reply("folders", request.id, status, tree)]);
    game.advance(5.0);
    request.id
}

fn open_browser(game: &Game) -> u32 {
    open_browser_with(game, TREE, Status::Done)
}

fn shown(game: &Game, frame: &str) -> bool {
    text_of(game, &format!("{frame}:IsVisible() and 'yes' or 'no'")) == "yes"
}

/// The text of each shown row of the browser. A hidden row gives an empty text.
fn browse_rows(game: &Game, count: usize) -> Vec<String> {
    (1..=count)
        .map(|i| {
            let row = format!("GnomishRelayBrowseRow{i}");
            text_of(
                game,
                &format!("{row}:IsShown() and {row}.text:GetText() or ''"),
            )
        })
        .collect()
}

fn crumbs(game: &Game) -> Vec<String> {
    (1..=7)
        .map(|i| {
            let crumb = format!("GnomishRelayCrumb{i}");
            text_of(
                game,
                &format!("{crumb}:IsShown() and {crumb}.text:GetText() or ''"),
            )
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn sent_by_chat(game: &Game, text: &[u8]) -> Record {
    game.last_strip()
        .into_iter()
        .find(|r| r.text == text)
        .expect("the message in the last strip")
}

fn header_folder(game: &Game) -> String {
    text_of(game, "GnomishRelayFolderButton.text:GetText()")
}

fn filter_key(game: &Game, script: &str, arg: &str) {
    game.run(&format!(
        "GnomishRelayBrowserFilter:GetScript('{script}')(GnomishRelayBrowserFilter, '{arg}')"
    ));
}

fn chat_field(game: &Game, chat: usize, field: &str) -> String {
    text_of(game, &format!("GnomishRelayDB.chats[{chat}].{field}"))
}

#[test]
fn new_chat_starts_in_the_default_folder_with_its_transcript_and_sends_no_request() {
    let game = Game::start();
    game.advance(2.0);
    let before = game.shots();

    click_new_chat(&game);
    game.advance(2.0);

    let requests = (before + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .filter(|r| r.chat == b"folders")
        .count();
    assert_eq!(requests, 0, "no folder list request");
    assert!(!shown(&game, "GnomishRelayBrowser"));
    assert!(shown(&game, "GnomishRelayTranscript"));
    game.send("hi");
    game.advance(1.0);
    let record = sent_by_chat(&game, b"hi");
    assert!(record.cwd.is_empty());
    assert!(!flags(&record).contains(&"mkdir=1".into()));
    assert_eq!(chat_field(&game, 1, "name"), "Chat 1");
}

#[test]
fn the_folder_button_opens_the_browser_in_the_center_and_closes_it_again() {
    let game = Game::start();
    click_new_chat(&game);

    open_browser(&game);

    assert!(shown(&game, "GnomishRelayBrowser"));
    assert!(!shown(&game, "GnomishRelayTranscript"));
    assert!(shown(&game, "GnomishRelayInput"), "the input stays");
    assert_eq!(
        text_of(
            &game,
            "GnomishRelayBrowserFilter:HasFocus() and 'yes' or 'no'"
        ),
        "yes"
    );
    assert_eq!(header_folder(&game), "~/Code");

    game.run("GnomishRelayFolderButton:Click()");

    assert!(!shown(&game, "GnomishRelayBrowser"));
    assert!(shown(&game, "GnomishRelayTranscript"));
}

#[test]
fn the_browser_shows_the_subfolders_with_a_git_mark_under_a_breadcrumb() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);

    assert_eq!(crumbs(&game), ["|cffffd100Code|r"]);
    assert_eq!(
        browse_rows(&game, 5),
        ["", "Personal", "scratch", NEW_FOLDER_ROW, ""],
        "the breadcrumb row, the subfolders, and New folder; a broken line is skipped"
    );

    game.run("GnomishRelayBrowseRow2:Click()");

    assert_eq!(
        crumbs(&game),
        ["|cffffd100Code|r", "\u{203a} |cffffd100Personal|r"]
    );
    assert_eq!(browse_rows(&game, 3)[1], "gnomish-relay");
    assert_eq!(
        text_of(&game, "GnomishRelayBrowseRow2.mark:GetText()"),
        "|cff8fb6e8git|r"
    );
}

#[test]
fn the_breadcrumb_goes_up_but_never_above_the_root() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);
    game.run("GnomishRelayBrowseRow2:Click()");
    game.run("GnomishRelayBrowseRow2:Click()");
    assert_eq!(crumbs(&game).len(), 3);

    game.run("GnomishRelayCrumb2:Click()");
    assert_eq!(browse_rows(&game, 3)[1..], ["gnomish-relay", "timeways"]);

    game.run("GnomishRelayCrumb1:Click()");
    assert_eq!(crumbs(&game), ["|cffffd100Code|r"], "the root is the top");
    assert_eq!(browse_rows(&game, 2)[1], "Personal");
}

#[test]
fn open_sets_the_folder_and_the_first_message_carries_it() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);
    game.run("GnomishRelayBrowseRow2:Click()");
    game.run("GnomishRelayBrowseRow2:Click()");
    assert_eq!(text_of(&game, "GnomishRelayBrowserOpen:GetText()"), "Open");

    game.run("GnomishRelayBrowserOpen:Click()");

    assert!(!shown(&game, "GnomishRelayBrowser"));
    assert_eq!(chat_field(&game, 1, "name"), "gnomish-relay");
    assert_eq!(header_folder(&game), "~/Code/Personal/gnomish-relay");
    game.send("hi");
    game.advance(1.0);
    let record = sent_by_chat(&game, b"hi");
    assert_eq!(record.cwd, b"Personal/gnomish-relay");
    assert!(flags(&record).contains(&"n".into()));
    assert!(!flags(&record).contains(&"mkdir=1".into()));
}

#[test]
fn the_filter_matches_the_paths_and_enter_picks_the_chosen_one() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);

    game.run("GnomishRelayBrowserFilter:SetText('gre')");

    assert_eq!(
        browse_rows(&game, 3),
        ["gnomish-relay", "crates", ""],
        "repositories first, then shorter paths"
    );
    assert_eq!(
        text_of(&game, "GnomishRelayBrowseRow2.right:GetText()"),
        "~/Code/Personal/gnomish-relay"
    );
    filter_key(&game, "OnArrowPressed", "DOWN");
    filter_key(&game, "OnArrowPressed", "DOWN");
    filter_key(&game, "OnEnterPressed", "");

    assert_eq!(chat_field(&game, 1, "cwd"), "Personal/gnomish-relay/crates");
    assert!(!shown(&game, "GnomishRelayBrowser"));
}

#[test]
fn escape_closes_the_browser_and_gives_the_keys_back_to_the_game() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);

    filter_key(&game, "OnEscapePressed", "");

    assert!(!shown(&game, "GnomishRelayBrowser"));
    assert_eq!(
        text_of(
            &game,
            "GnomishRelayBrowserFilter:HasFocus() and 'yes' or 'no'"
        ),
        "no"
    );
}

#[test]
fn a_recent_folder_sets_the_folder_in_one_click_and_a_gone_folder_does_not_show() {
    let game = Game::start();
    game.run("local ns = ... ns.Store.SetFolder(ns.Store.NewChat(), 'gone', 'gone')");
    game.run(
        "local ns = ... ns.Store.SetFolder(ns.Store.NewChat(), 'Personal/timeways', 'timeways')",
    );
    click_new_chat(&game);
    open_browser(&game);

    assert_eq!(browse_rows(&game, 2), ["timeways", ""]);
    assert_eq!(
        text_of(&game, "GnomishRelayBrowseRow1.right:GetText()"),
        "~/Code/Personal"
    );
    game.run("GnomishRelayBrowseRow1:Click()");

    assert_eq!(chat_field(&game, 3, "cwd"), "Personal/timeways");
    assert_eq!(chat_field(&game, 3, "name"), "timeways 2");
}

#[test]
fn new_folder_checks_the_name_and_sets_the_folder_with_a_mark() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);
    game.run("GnomishRelayBrowseRow4:Click()");
    assert!(shown(&game, "GnomishRelayBrowserName"));
    assert_eq!(
        text_of(
            &game,
            "GnomishRelayBrowserName:HasFocus() and 'yes' or 'no'"
        ),
        "yes"
    );
    let enter = "GnomishRelayBrowserName:GetScript('OnEnterPressed')(GnomishRelayBrowserName)";

    for (name, problem) in [
        ("personal", "Already here"),
        ("a/b", "Bad name"),
        ("..", "Bad name"),
    ] {
        game.run(&format!("GnomishRelayBrowserName:SetText('{name}')"));
        game.run(enter);
        assert!(
            text_of(&game, "GnomishRelayBrowseRow4.right:GetText()").contains(problem),
            "{name}"
        );
        assert_eq!(chat_field(&game, 1, "cwd"), "");
    }
    game.run("GnomishRelayBrowserName:SetText('fresh')");
    game.run(enter);

    assert_eq!(chat_field(&game, 1, "cwd"), "fresh");
    assert_eq!(chat_field(&game, 1, "name"), "fresh");
    assert_eq!(header_folder(&game), "~/Code/fresh |cff9fe39fnew|r");
    game.send("hi");
    game.advance(1.0);
    let record = sent_by_chat(&game, b"hi");
    assert_eq!(record.cwd, b"fresh");
    assert!(flags(&record).contains(&"mkdir=1".into()));
    assert!(flags(&record).contains(&"n".into()));

    let chat = game.chat_id();
    game.publish(&[reply(&chat, record.id, Status::Working, "")]);
    game.advance(5.0);

    assert_eq!(
        header_folder(&game),
        "~/Code/fresh",
        "the bridge has the folder now"
    );
}

#[test]
fn after_the_first_message_the_browser_makes_a_new_chat_in_the_chosen_folder() {
    let game = Game::start();
    click_new_chat(&game);
    game.send("hi");
    open_browser(&game);
    assert_eq!(
        text_of(&game, "GnomishRelayBrowserOpen:GetText()"),
        "New Chat here"
    );
    game.run("GnomishRelayBrowseRow2:Click()");

    game.run("GnomishRelayBrowserOpen:Click()");

    assert_eq!(chat_count(&game), 2);
    assert_eq!(
        chat_field(&game, 1, "cwd"),
        "",
        "the first chat keeps its folder"
    );
    assert_eq!(chat_field(&game, 2, "cwd"), "Personal");
    assert_eq!(
        text_of(&game, "GnomishRelayDB.selected"),
        chat_field(&game, 2, "id")
    );
}

#[test]
fn two_roots_give_a_top_level_that_lists_them() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser_with(
        &game,
        "~/Code\n0\t~/Code\t\n0\t~/work\t\n2\tsite\t",
        Status::Done,
    );
    game.run("GnomishRelayCrumb1:Click()");

    assert_eq!(crumbs(&game), ["|cffffd100Roots|r"]);
    assert_eq!(
        browse_rows(&game, 3),
        ["", "|cff1eff00Code|r", "work"],
        "the folder of the chat is green"
    );
    assert!(
        !shown(&game, "GnomishRelayBrowserOpen"),
        "a list of roots is no folder"
    );

    game.run("GnomishRelayBrowseRow3:Click()");
    game.run("GnomishRelayBrowseRow2:Click()");
    game.run("GnomishRelayBrowserOpen:Click()");

    assert_eq!(chat_field(&game, 1, "cwd"), "../work/site");
    assert_eq!(header_folder(&game), "~/work/site");
}

#[test]
fn the_tree_is_kept_so_the_browser_shows_it_at_once_with_a_spinner_for_the_new_one() {
    let game = Game::start();
    click_new_chat(&game);
    let id = open_browser(&game);
    game.run("GnomishRelayFolderButton:Click()");
    game.send("hi");
    game.advance(1.0);
    assert!(flags(&game.last_strip()[0]).contains(&format!("read={id}")));

    let game = game.reload();
    click_new_chat(&game);
    game.run("GnomishRelayFolderButton:Click()");

    assert_eq!(
        browse_rows(&game, 3),
        ["Code", "", "Personal"],
        "the folder of the first chat is a recent folder"
    );
    assert!(shown(&game, "GnomishRelayBrowserSpinner"));
}

#[test]
fn a_failed_folder_list_keeps_the_last_tree() {
    let game = Game::start();
    click_new_chat(&game);
    open_browser(&game);
    game.run("GnomishRelayFolderButton:Click()");

    open_browser_with(&game, "no roots", Status::Error);

    assert_eq!(browse_rows(&game, 2)[1], "Personal");
    assert!(!shown(&game, "GnomishRelayBrowserSpinner"));
}

/// A tiny xorshift, so the test needs no crate and each run is the same.
fn next_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

/// Fails with the broken rule, or gives `true`.
const TREE_RULES: &str = r#"
local ns, text = ...
local tree = ns.Folders.Parse(text)
local seen = {}
for i, node in ipairs(tree.list) do
    assert(not node.folder:find("%c"), "a control character in a folder")
    assert(#node.folder <= 255, "a long folder")
    assert(node.name ~= "" and not node.name:find("[\t\n]"), "a bad name")
    assert(node.parent == nil or seen[node.parent], "a parent that comes later")
    assert(tree.byFolder[node.folder] == node, "two nodes for one folder")
    seen[node] = true
end
return true
"#;

#[test]
fn the_folder_tree_parser_gives_a_clean_tree_for_any_bytes() {
    let game = Game::start();
    let check: Function = game.lua.load(TREE_RULES).into_function().unwrap();
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    let alphabet = b"\t\n\t\n0123g/.~+a\x1f\x00|\xff\xc2\x85";
    for len in 0..600 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                alphabet[usize::try_from(next_random(&mut seed) % 256).unwrap() % alphabet.len()]
            })
            .collect();
        let text = game.lua.create_string(&bytes).unwrap();
        let clean: bool = check.call((game.ns.clone(), text)).unwrap();
        assert!(clean);
    }
}

const MONO: &str = "Interface\\AddOns\\GnomishRelay\\JetBrainsMono-Regular.ttf";

/// Sends one message and answers it with `markdown`, rendered as the bridge does.
fn rendered_reply(game: &Game, markdown: &str) {
    game.run("local ns = ... ns.Window.Open()");
    game.send("go");
    game.advance(1.0);
    let text = render_markdown(markdown.as_bytes());
    game.publish(&[Reply {
        chat: game.chat_id().into_bytes(),
        id: first_message_id(game),
        status: Status::Done,
        text,
    }]);
    game.advance(5.0);
}

fn of_kind<'a>(drawn: &'a [Drawn], kind: &str) -> Vec<&'a Drawn> {
    drawn.iter().filter(|d| d.kind == kind).collect()
}

fn text_color(d: &Drawn) -> Vec<f64> {
    d.object.get("textColor").unwrap()
}

/// Every `|` that WoW sees starts `||`, a color code, or `|r`.
fn wow_safe(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'|' {
            i += 1;
        } else if bytes.get(i + 1) == Some(&b'|') || bytes.get(i + 1) == Some(&b'r') {
            i += 2;
        } else if bytes.get(i + 1) == Some(&b'c')
            && bytes.len() >= i + 10
            && bytes[i + 2..i + 10].iter().all(u8::is_ascii_hexdigit)
        {
            i += 10;
        } else {
            return false;
        }
    }
    true
}

/// Every `&` of a `SimpleHTML` text starts a whole entity.
fn entities_whole(text: &str) -> bool {
    text.match_indices('&').all(|(i, _)| {
        ["&lt;", "&gt;", "&amp;"]
            .iter()
            .any(|e| text[i..].starts_with(e))
    })
}

#[test]
fn a_markdown_reply_draws_headings_a_code_box_and_a_table_grid() {
    let game = Game::start();
    rendered_reply(
        &game,
        "# Plan\n\nSome **bold** text.\n\n## Steps\n\n- one\n- two\n\n\
         ```\nfn main() {}\n```\n\n| Name | Age |\n|---|---|\n| Ann | 31 |",
    );

    let drawn = transcript(&game);
    let html = of_kind(&drawn, "SimpleHTML");
    assert_eq!(html.len(), 1);
    let doc = html[0].text.clone().unwrap();
    assert!(doc.contains("<h1>Plan</h1>"), "{doc}");
    assert!(doc.contains("<h2>Steps</h2>"), "{doc}");
    assert!(doc.contains("<p>Some |cffffd100bold|r text.</p>"), "{doc}");
    assert!(doc.contains("\u{2022} one</p><p>"), "{doc}");

    let code = drawn
        .iter()
        .find(|d| d.text.as_deref() == Some("fn main() {}"))
        .expect("a code line");
    assert_eq!(code.object.get::<String>("font").unwrap(), MONO);
    let parent: Table = code.object.get("parent").unwrap();
    assert_eq!(parent.get::<String>("kind").unwrap(), "Frame");

    let cell = |text: &str| {
        drawn
            .iter()
            .find(|d| d.text.as_deref() == Some(text))
            .unwrap_or_else(|| panic!("no cell {text}"))
    };
    let (name, age, ann, years) = (cell("Name"), cell("Age"), cell("Ann"), cell("31"));
    assert_eq!(name.y, age.y);
    assert_eq!(ann.y, years.y);
    assert!(ann.y > name.y);
    assert!(age.object.get::<f64>("x").unwrap() > name.object.get::<f64>("x").unwrap());
    assert_eq!(text_color(name), [1.0, 0.82, 0.0]);
    assert_eq!(text_color(ann), [1.0, 1.0, 1.0]);
    assert!(
        html[0].y < code.y && code.y < name.y,
        "blocks stack in order"
    );
}

#[test]
fn a_table_too_wide_for_the_window_draws_each_row_as_a_card() {
    let game = Game::start();
    let long = "x".repeat(80);
    rendered_reply(
        &game,
        &format!("| Test | Result | Note |\n|---|---|---|\n| parse | ok | {long} |"),
    );

    let drawn = transcript(&game);
    let title = drawn
        .iter()
        .find(|d| d.text.as_deref() == Some("parse"))
        .expect("the first cell as a title");
    assert_eq!(text_color(title), [1.0, 0.82, 0.0]);
    let body = drawn
        .iter()
        .find(|d| d.text.as_deref().is_some_and(|t| t.contains("ok")))
        .expect("the other cells");
    assert_eq!(
        body.text.as_deref().unwrap(),
        format!("|cff9d9d9dResult:|r ok\n|cff9d9d9dNote:|r {long}")
    );
    assert!(body.y > title.y);
    assert!(
        texts(&drawn).iter().all(|t| t != "Test"),
        "no card for the header"
    );
}

#[test]
fn a_reply_cut_at_any_byte_draws_with_no_broken_code() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open(ns.Store.NewChat().id)");
    let full = render_markdown(
        "# A <b> & c\n\n**bold** `|x|` [link](u) é\n\n| a | b |\n|---|---|\n| 1 | 2 |\n```\nx | y\n```"
            .as_bytes(),
    );
    for cut in 4..=full.len() {
        let text = game.lua.create_string(&full[..cut]).unwrap();
        game.lua.globals().set("CUT", text).unwrap();
        game.run(
            "local ns = ... local chat = ns.Store.db.chats[1] \
             chat.history = { { role = 'agent', text = CUT } } ns.Window.Refresh()",
        );
        let drawn = transcript(&game);
        assert!(
            of_kind(&drawn, "SimpleHTML").len() <= 1,
            "cut at {cut} fell back or doubled"
        );
        for text in texts(&drawn) {
            assert!(wow_safe(&text), "cut at {cut}: {text:?}");
            assert!(
                !text.starts_with("<html>") || entities_whole(&text),
                "cut at {cut} leaves half an entity: {text:?}"
            );
        }
    }
}

#[test]
fn a_reply_that_fails_to_draw_shows_as_plain_text() {
    let game = Game::start();
    game.wow.set("brokenHtml", true).unwrap();
    rendered_reply(&game, "# Title\n\nA <b> and `code`.");

    let drawn = transcript(&game);
    assert!(of_kind(&drawn, "SimpleHTML").is_empty());
    assert_eq!(
        texts(&drawn).last().unwrap(),
        "|cffff7d0a[Claude]|r: Title\nA <b> and code."
    );
}

#[test]
fn a_missing_mono_font_falls_back_to_a_game_font() {
    let game = Game::start_with(|wow| {
        let missing: Table = wow.get("missingFiles").unwrap();
        missing.set(MONO, true).unwrap();
    });
    rendered_reply(&game, "```\nlet x = 1;\n```");

    let drawn = transcript(&game);
    let code = drawn
        .iter()
        .find(|d| d.text.as_deref() == Some("let x = 1;"))
        .expect("a code line");
    assert_eq!(
        code.object.get::<String>("font").unwrap(),
        "Fonts\\ARIALN.TTF"
    );
}

#[test]
fn the_whisper_line_shows_the_plain_words_of_a_rendered_reply() {
    let game = Game::start();
    rendered_reply(&game, "**Done**: all `tests` pass | green\n\nMore.");

    let whisper = game
        .printed()
        .into_iter()
        .find(|l| l.contains("whispers:"))
        .expect("a whisper line");
    assert!(
        whisper.ends_with("] Done: all tests pass || green|r"),
        "{whisper}"
    );
}

#[test]
fn a_long_transcript_scrolls_to_the_newest_entry_and_the_wheel_scrolls_up() {
    let game = Game::start();
    rendered_reply(&game, &"line\n\n".repeat(60));

    let scroll: Table = game.lua.globals().get("GnomishRelayScroll").unwrap();
    let bottom: i64 = scroll.get("scroll").unwrap();
    assert!(bottom > 0);
    game.run("GnomishRelayScroll:GetScript('OnMouseWheel')(GnomishRelayScroll, 1)");
    assert_eq!(scroll.get::<i64>("scroll").unwrap(), bottom - 40);
    game.run("GnomishRelayScroll:GetScript('OnMouseWheel')(GnomishRelayScroll, -5)");
    assert_eq!(scroll.get::<i64>("scroll").unwrap(), bottom);
}

#[test]
fn an_error_that_looks_rendered_shows_as_plain_text() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.send("go");
    game.advance(1.0);
    game.publish(&[reply(
        &game.chat_id(),
        first_message_id(&game),
        Status::Error,
        "\x1bM1\np\x1f|cffff0000fake\n",
    )]);
    game.advance(5.0);

    let drawn = transcript(&game);
    assert!(of_kind(&drawn, "SimpleHTML").is_empty());
    assert!(texts(&drawn).last().unwrap().contains("||cffff0000fake"));
}

/// The settings list of a bridge with two agents, as the bridge writes it.
fn settings_text(story: bool) -> String {
    let settings = BridgeSettings {
        version: "0.1.0".into(),
        sandbox: "bwrap".into(),
        default_cwd: "~/Code".into(),
        roots: vec!["~/Code".into()],
        kinds: vec![
            ("claude".into(), "claude".into()),
            ("codex".into(), "codex".into()),
        ],
        timeout_minutes: 30,
        permission_timeout_minutes: 10,
        story: story.then(|| StorySettings {
            model: "claude haiku".into(),
            budget_window_minutes: 20,
        }),
        allow: vec!["cargo test".into()],
        allow_folders: vec![("~/Code/lighthouse".into(), "npm test".into())],
    };
    let policy = Policy {
        folders: Folders {
            roots: vec![b"/home/x/Code".to_vec()],
            base: b"/home/x/Code".to_vec(),
        },
        agents: [
            ("claude".to_owned(), Permission::AutoEdit),
            ("codex".to_owned(), Permission::Ask),
        ]
        .into(),
        default_agent: "claude".into(),
    };
    settings_reply(&settings, &policy)
}

fn click(game: &Game, frame: &str) {
    game.run(&format!("{frame}:Click()"));
}

fn open_tab(game: &Game, n: usize) {
    game.run("local ns = ... ns.Window.Open()");
    click(game, &format!("GnomishRelayTab{n}"));
}

const SETTINGS: usize = 2;
const DIAG: usize = 3;

fn settings_requests(game: &Game, since: usize) -> usize {
    (since + 1..=game.shots())
        .flat_map(|n| game.strip(n))
        .filter(|r| flags(r).contains(&"list=settings".into()) && r.chat == b"settings")
        .count()
}

/// Opens Settings, and answers its list request with the list of `settings_text`.
fn open_settings_with_list(game: &Game, story: bool) {
    let before = game.shots();
    open_tab(game, SETTINGS);
    game.advance(2.0);
    assert_eq!(settings_requests(game, before), 1, "one settings request");
    game.publish(&[reply("settings", 99, Status::Done, &settings_text(story))]);
    game.run("local ns = ... ns.Transport.Poll()");
}

fn shown_by_name(game: &Game, name: &str) -> bool {
    game.run(&format!("return {name}:IsVisible()"))
        .as_boolean()
        .unwrap()
}

#[test]
fn settings_takes_the_place_of_the_center_and_a_tile_goes_back_to_chats() {
    let game = Game::start();
    game.send("hello");
    open_tab(&game, SETTINGS);

    assert!(shown_by_name(&game, "GnomishRelaySettingsStatus"));
    assert!(!shown_by_name(&game, "GnomishRelayInput"));
    assert!(!shown_by_name(&game, "GnomishRelayTranscript"));
    assert!(!shown_by_name(&game, "GnomishRelayFolderButton"));
    assert!(shown_by_name(&game, "GnomishRelayTile1"), "the tiles stay");

    click(&game, "GnomishRelayTile1");
    assert!(!shown_by_name(&game, "GnomishRelaySettingsStatus"));
    assert!(shown_by_name(&game, "GnomishRelayInput"));
    assert!(shown_by_name(&game, "GnomishRelayTranscript"));
}

#[test]
fn settings_asks_for_the_list_only_when_it_is_old_and_on_a_click_on_the_status() {
    let game = Game::start();
    open_settings_with_list(&game, false);
    click(&game, "GnomishRelayTab1");
    let before = game.shots();

    click(&game, &format!("GnomishRelayTab{SETTINGS}"));
    game.advance(2.0);
    assert_eq!(
        settings_requests(&game, before),
        0,
        "a fresh list asks nothing"
    );

    click(&game, "GnomishRelaySettingsStatus");
    game.advance(2.0);
    assert_eq!(settings_requests(&game, before), 1, "a click asks again");

    click(&game, "GnomishRelayTab1");
    game.advance(700.0);
    let before = game.shots();
    click(&game, &format!("GnomishRelayTab{DIAG}"));
    game.advance(2.0);
    assert_eq!(
        settings_requests(&game, before),
        1,
        "an old list asks again"
    );
}

#[test]
fn the_status_line_shows_the_age_of_the_list_and_the_state_of_the_bridge() {
    let game = Game::start();
    open_tab(&game, SETTINGS);
    assert_eq!(
        text_of(&game, "GnomishRelaySettingsStatus.text:GetText()"),
        "|cff8d8778No data yet.|r"
    );

    game.publish(&[reply("settings", 99, Status::Done, &settings_text(false))]);
    game.run("local ns = ... ns.Transport.Poll()");
    game.advance(120.0);
    game.run("local ns = ... ns.Window.Refresh()");
    assert_eq!(
        text_of(&game, "GnomishRelaySettingsStatus.text:GetText()"),
        "|cff1eff00Online · 2m ago|r"
    );

    game.advance(500.0);
    game.run("local ns = ... ns.Window.Refresh()");
    assert_eq!(
        text_of(&game, "GnomishRelaySettingsStatus.text:GetText()"),
        "|cffff9f40Online · 10m ago|r",
        "an old list is orange"
    );

    game.wow.set("body", Value::Nil).unwrap();
    game.advance(3600.0);
    game.run("local ns = ... ns.Window.Refresh()");
    assert!(
        text_of(&game, "GnomishRelaySettingsStatus.text:GetText()")
            .starts_with("|cff8d8778Offline · 1h ago"),
        "{}",
        text_of(&game, "GnomishRelaySettingsStatus.text:GetText()")
    );
}

#[test]
fn a_new_chat_takes_the_agent_and_level_that_settings_chose() {
    let game = Game::start();
    open_settings_with_list(&game, false);
    assert_eq!(
        text_of(&game, "GnomishRelaySettingsAgentChoice2.text:GetText()"),
        "Codex"
    );
    click(&game, "GnomishRelaySettingsAgent");
    click(&game, "GnomishRelaySettingsAgentChoice2");
    click(&game, "GnomishRelaySettingsLevel");
    click(&game, "GnomishRelaySettingsLevelChoice1");

    let texts = texts_of(&game, "FontString");
    assert!(
        texts.contains(&"|cff8d8778Max: ask (set on the desktop)|r".to_owned()),
        "{texts:?}"
    );
    click_new_chat(&game);
    game.send("hi");
    game.advance(1.0);

    let f = flags(&game.last_strip()[0]);
    assert!(
        f.contains(&"agent=codex".into()) && f.contains(&"level=ask".into()),
        "{f:?}"
    );
}

#[test]
fn a_chosen_agent_that_the_bridge_no_longer_has_gives_the_default_agent() {
    let game = Game::start();
    game.run("local ns = ... ns.Store.db.newAgent = 'gemini'");
    open_settings_with_list(&game, false);
    click_new_chat(&game);
    game.send("hi");
    game.advance(1.0);

    let f = flags(&game.last_strip()[0]);
    assert!(f.contains(&"agent=claude".into()), "{f:?}");
}

fn font_of(game: &Game, code: &str) -> i64 {
    game.run(&format!("return {code}.fontSize"))
        .as_integer()
        .unwrap()
}

#[test]
fn the_font_size_applies_at_once_to_all_chat_text_and_the_input() {
    let game = Game::start();
    rendered_reply(&game, "# Title\n\nSome words.\n\n```\nlet x = 1;\n```\n");
    open_tab(&game, SETTINGS);
    game.run("GnomishRelaySettingsFont:GetScript('OnValueChanged')(GnomishRelaySettingsFont, 18)");
    click(&game, "GnomishRelayTab1");

    let drawn = transcript(&game);
    let html = of_kind(&drawn, "SimpleHTML")[0];
    let fonts: Table = html.object.get("fonts").unwrap();
    assert!(fonts.contains_key("p").unwrap());
    let code = of_kind(&drawn, "Frame")
        .into_iter()
        .find(|d| d.object.get::<Option<Table>>("text").unwrap().is_some())
        .expect("a code box");
    let code_text: Table = code.object.get("text").unwrap();
    assert_eq!(code_text.get::<i64>("fontSize").unwrap(), 16);
    let line = drawn.iter().find(|d| d.kind == "FontString").unwrap();
    assert_eq!(line.object.get::<i64>("fontSize").unwrap(), 18);
    assert_eq!(font_of(&game, "GnomishRelayInput"), 18);
    assert_eq!(game.db().get::<i64>("fontSize").unwrap(), 18);
}

#[test]
fn relay_size_sets_the_font_size_within_12_to_20() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.run("SlashCmdList.GNOMISHRELAY('size 16')");
    assert_eq!(game.db().get::<i64>("fontSize").unwrap(), 16);
    assert_eq!(font_of(&game, "GnomishRelayInput"), 16);
    game.run("SlashCmdList.GNOMISHRELAY('size 40')");
    assert_eq!(game.db().get::<i64>("fontSize").unwrap(), 20);
    game.run("SlashCmdList.GNOMISHRELAY('size 3')");
    assert_eq!(game.db().get::<i64>("fontSize").unwrap(), 12);
}

fn answer_first_message(game: &Game, text: &str) {
    game.send("go");
    game.advance(1.0);
    game.publish(&[reply(
        &game.chat_id(),
        first_message_id(game),
        Status::Done,
        text,
    )]);
    game.advance(5.0);
}

#[test]
fn the_reply_line_can_be_off_or_silent_and_takes_the_chosen_color() {
    let game = Game::start();
    open_tab(&game, SETTINGS);
    click(&game, "GnomishRelaySettingsColor3");
    click(&game, "GnomishRelaySettingsSound");
    let preview = texts_of(&game, "FontString")
        .into_iter()
        .find(|t| t.contains("whispers:"))
        .expect("a preview");
    assert!(preview.starts_with("|cff69ccf0["), "{preview}");

    answer_first_message(&game, "done");
    let whisper = game
        .printed()
        .into_iter()
        .find(|l| l.contains("whispers:"))
        .unwrap();
    assert!(whisper.starts_with("|cff69ccf0|H"), "{whisper}");
    let sounds: Table = game.wow.get("sounds").unwrap();
    assert_eq!(sounds.raw_len(), 0, "no sound");

    click(&game, "GnomishRelaySettingsReply");
    let game = game.reload();
    answer_first_message(&game, "second");
    assert!(
        !game.printed().iter().any(|l| l.contains("whispers:")),
        "the line is off"
    );
}

#[test]
fn a_desktop_request_gets_its_line_also_with_the_reply_line_off() {
    let game = Game::start();
    game.run("local ns = ... ns.Store.db.whisperOn = false");
    game.send("read my key");
    game.advance(1.0);
    wait_on_desktop(&game, WAIT);
    game.run("local ns = ... ns.Transport.Poll()");
    assert_eq!(whispers_with(&game, "Approve on your desktop."), 1);
}

#[test]
fn the_window_opens_where_the_player_left_it_until_a_reset() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open()");
    game.run(
        "GnomishRelayFrame:SetPoint('TOPLEFT', UIParent, 'TOPLEFT', 40, -30) \
         GnomishRelayFrame:GetScript('OnDragStop')(GnomishRelayFrame)",
    );
    let game = game.reload();
    game.run("local ns = ... ns.Window.Open()");
    assert_eq!(
        text_of(&game, "select(1, GnomishRelayFrame:GetPoint())"),
        "TOPLEFT"
    );
    assert_eq!(
        game.run("return select(4, GnomishRelayFrame:GetPoint())")
            .as_integer(),
        Some(40)
    );

    open_tab(&game, SETTINGS);
    click(&game, "GnomishRelaySettingsReset");
    assert_eq!(
        text_of(&game, "select(1, GnomishRelayFrame:GetPoint())"),
        "CENTER"
    );
    let game = game.reload();
    game.run("local ns = ... ns.Window.Open()");
    assert_eq!(
        text_of(&game, "select(1, GnomishRelayFrame:GetPoint())"),
        "CENTER"
    );
}

fn diag_texts(game: &Game) -> Vec<String> {
    (1..=26)
        .filter(|i| shown_by_name(game, &format!("GnomishRelayDiagLine{i}")))
        .map(|i| {
            let label = text_of(game, &format!("GnomishRelayDiagLine{i}.label:GetText()"));
            let value = text_of(game, &format!("GnomishRelayDiagLine{i}.value:GetText()"));
            format!("{label}|{value}")
        })
        .collect()
}

#[test]
fn diag_shows_the_bridge_values_timeways_versions_and_the_transport_lines() {
    let game = Game::start();
    open_settings_with_list(&game, true);
    click(&game, &format!("GnomishRelayTab{DIAG}"));

    let rows = diag_texts(&game);
    for expected in [
        "Allowed|~/Code",
        "Default folder|~/Code",
        "Agents|claude  auto-edit",
        "|codex  ask",
        "Commands|cargo test",
        "|npm test  in ~/Code/lighthouse",
        "Timeout|30 min · ask 10 min",
        "Model|claude haiku",
        "Budget|10 calls / 20 min",
        "|Bridge 0.1.0 · protocol 1",
    ] {
        assert!(rows.contains(&expected.to_owned()), "{expected}: {rows:#?}");
    }
    assert!(
        rows.iter().any(|r| r.contains("Gnomish Relay: slot ")),
        "{rows:#?}"
    );
}

#[test]
fn diag_greys_the_bridge_values_while_the_bridge_is_offline() {
    let game = Game::start();
    open_settings_with_list(&game, false);
    game.wow.set("body", Value::Nil).unwrap();
    game.advance(3600.0);
    click(&game, &format!("GnomishRelayTab{DIAG}"));

    let rows = diag_texts(&game);
    assert!(
        rows.contains(&"Allowed||cff8d8778~/Code|r".to_owned()),
        "{rows:#?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("Timeways")),
        "no story section"
    );
}

#[test]
fn diag_says_no_data_yet_with_no_list() {
    let game = Game::start();
    open_tab(&game, DIAG);
    let rows = diag_texts(&game);
    assert!(
        rows.contains(&"Status||cff8d8778No data yet.|r".to_owned()),
        "{rows:#?}"
    );
}

/// Fails with the broken rule, or gives `true`.
const SETTINGS_RULES: &str = r#"
local ns, text = ...
local parsed = ns.BridgeSettings.Parse(text)
for key, value in pairs(parsed.values) do
    assert(key:match("^[%l_]+$"), "a bad key")
    assert(not value:find("%c"), "a control character in a value")
end
for _, agent in ipairs(parsed.agents) do
    assert(ns.Codec.IsValidId(agent.name), "a bad agent name")
    assert(({ ask = true, ["auto-edit"] = true, ["full-auto"] = true })[agent.level], "a bad level")
end
for _, rule in ipairs(parsed.folders) do
    assert(rule.folder ~= "" and rule.pattern ~= "", "an empty rule")
    assert(not (rule.folder .. rule.pattern):find("%c"), "a control character in a rule")
end
for _, list in ipairs({ parsed.roots, parsed.allow }) do
    for _, value in ipairs(list) do
        assert(type(value) == "string" and not value:find("[\n]"), "a bad line")
    end
end
return true
"#;

#[test]
fn the_settings_parser_gives_clean_values_for_any_bytes() {
    let game = Game::start();
    let check: Function = game.lua.load(SETTINGS_RULES).into_function().unwrap();
    let mut seed = 0x2545_f491_4f6c_dd1d;
    let alphabet = b"\t\n\t\nagent_low+claude-ask\x1f\x00|\xff\xc2\x85auto-edit";
    for len in 0..600 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                alphabet[usize::try_from(next_random(&mut seed) % 256).unwrap() % alphabet.len()]
            })
            .collect();
        let text = game.lua.create_string(&bytes).unwrap();
        let clean: bool = check.call((game.ns.clone(), text)).unwrap();
        assert!(clean);
    }
}

#[test]
fn the_settings_parser_reads_the_list_of_the_bridge() {
    let game = Game::start();
    let text = game.lua.create_string(settings_text(true)).unwrap();
    let check: Function = game
        .lua
        .load(
            "local ns, text = ... local p = ns.BridgeSettings.Parse(text) \
             return p.values.default_agent, #p.agents, p.agents[2].level, p.folders[1].folder, p.cut",
        )
        .into_function()
        .unwrap();
    let (agent, agents, level, folder, cut): (String, i64, String, String, bool) =
        check.call((game.ns.clone(), text)).unwrap();
    assert_eq!(
        (agent.as_str(), agents, level.as_str(), folder.as_str(), cut),
        ("claude", 2, "ask", "~/Code/lighthouse", false)
    );
}
