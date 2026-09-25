//! The whole addon in a fake game: strips out, slots in, the outbox, restore, and the window.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fmt::Write;

use bridge::activity::text_hash;
use bridge::agent::{Agent, Control, Echo};
use bridge::config::{Permission, Policy};
use bridge::receive::{StripKey, receive};
use bridge::relay::{Folders, Relay};
use bridge::strip::{self, Image};
use common::{Bits, load_into, lua, repo_file, screenshot_png};
use hmac::{Hmac, Mac};
use mlua::{Function, Lua, Table, Value};
use protocol::cell::decode_cells;
use protocol::frame::{decode_frame, signed_len};
use protocol::live::{
    OptionKind, PermOption, Progress, Request as LiveRequest, live_body, prepare_progress,
    prepare_requests,
};
use protocol::record::{Record, parse_records};
use protocol::restore::{Chat, Entry, Role, prepare_restore, restore_body};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};
use sha2::Sha256;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const FILES: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Store.lua",
    "Health.lua",
    "Strip.lua",
    "Transport.lua",
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
        let body = slot_body(1_790_211_079, &prepare_replies(replies));
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
    let key = StripKey::from_hex(&common::hex(KEY)).unwrap();
    for frame in frames {
        let records = receive(&frame, &key, 1_790_211_209).unwrap();
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
    let restore = restore_body(token.as_bytes(), &prepare_restore(&chats));
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
    live_body(&prepare_progress(progress), &prepare_requests(requests))
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

    let transcript: Table = game.lua.globals().get("GnomishRelayTranscript").unwrap();
    let lines: Vec<String> = transcript.get("lines").unwrap();
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
