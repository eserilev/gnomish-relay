//! The reading and sending tools of the chat window in a fake game (SPEC.md 13.1): the
//! summary of a long reply, quick actions, search, and pinned replies.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{fake_game_for, game_lua_for, load_into, measured, start_addon};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::markdown::render_markdown;
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

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
    "Notices.lua",
    "Blocks.lua",
    "Transcript.lua",
    "Folders.lua",
    "Browser.lua",
    "BridgeSettings.lua",
    "RulesGroup.lua",
    "SettingsTab.lua",
    "DiagTab.lua",
    "Window.lua",
    "Popup.lua",
    "NoticeFrames.lua",
    "Core.lua",
];

/// Eleven paragraphs: long enough to show only the summary.
const LONG: &str = "Fixed the flaky test.\n\n\
    Two.\n\nThree.\n\nFour.\n\nFive.\n\nSix.\n\nSeven.\n\nEight.\n\nNine.\n\nTen.\n\nThe last line.";

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

impl Game {
    fn start() -> Game {
        Game::boot(None)
    }

    /// `/reload`: WoW saves the saved variables, and a new UI session loads them.
    fn reload(&self) -> Game {
        let saved: String = self
            .wow
            .get::<Function>("Save")
            .unwrap()
            .call("GnomishRelayDB")
            .unwrap();
        Game::boot(Some(&saved))
    }

    fn boot(saved: Option<&str>) -> Game {
        let fake = measured();
        let lua = game_lua_for(&fake);
        let wow = fake_game_for(&lua, "addon/tests/api.lua", &fake);
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        start_addon(
            &lua,
            &wow,
            &fake,
            "GnomishRelay",
            saved.map(str::as_bytes),
            || {
                load_into(&lua, &ns, FILES);
            },
        );
        let game = Game { lua, wow, ns };
        game.run("local ns = ... ns.Window.Open()");
        game
    }

    fn run(&self, code: &str) -> Value {
        self.lua.load(code).call(self.ns.clone()).unwrap()
    }

    fn text(&self, code: &str) -> String {
        self.run(code).as_string_lossy().unwrap_or_default()
    }

    fn advance(&self, seconds: f64) {
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(seconds).unwrap();
    }

    fn now(&self) -> u32 {
        u32::try_from(self.run("return time()").as_integer().unwrap()).unwrap()
    }

    fn send(&self, text: &str) {
        let send: Function = self.ns.get::<Table>("Window").unwrap().get("Send").unwrap();
        send.call::<()>(text).unwrap();
    }

    fn chat_id(&self) -> String {
        self.text("local ns = ... return ns.Window.SelectedChat().id")
    }

    fn last_message_id(&self) -> u32 {
        let id = self.run(
            "local ns = ... local h = ns.Window.SelectedChat().history \
             for i = #h, 1, -1 do if h[i].role == 'user' then return h[i].id end end",
        );
        u32::try_from(id.as_integer().unwrap()).unwrap()
    }

    /// The bridge answers the last message, and the addon polls.
    fn answer(&self, status: Status, text: Vec<u8>) {
        self.advance(1.0);
        let reply = Reply {
            chat: self.chat_id().into_bytes(),
            id: self.last_message_id(),
            status,
            text,
        };
        let body = slot_body(App::Relay, self.now(), &prepare_replies(&[reply]));
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
        self.run("local ns = ... ns.Transport.Poll()");
    }

    /// Sends `message`, and the bridge answers it with `markdown`, rendered.
    fn exchange(&self, message: &str, markdown: &str) {
        self.send(message);
        self.answer(Status::Done, render_markdown(markdown.as_bytes()));
    }

    /// Every shown text of the transcript, top to bottom.
    fn transcript(&self) -> Vec<String> {
        let root: Table = self.lua.globals().get("GnomishRelayTranscript").unwrap();
        let drawn: Table = self
            .wow
            .get::<Function>("Drawn")
            .unwrap()
            .call(root)
            .unwrap();
        drawn
            .sequence_values::<Table>()
            .filter_map(|d| d.unwrap().get::<Option<String>>("text").unwrap())
            .collect()
    }

    /// The shown object of the transcript whose text holds `part`.
    fn drawn(&self, part: &str) -> Table {
        let find: Function = self
            .lua
            .load(
                "local wow, part = ... \
                 for _, d in ipairs(wow.Drawn(GnomishRelayTranscript)) do \
                   if d.text and d.text:find(part, 1, true) then return d.object end \
                 end \
                 error('nothing shows ' .. part)",
            )
            .into_function()
            .unwrap();
        find.call((self.wow.clone(), part)).unwrap()
    }

    fn shows(&self, part: &str) -> bool {
        self.transcript().iter().any(|t| t.contains(part))
    }

    /// Clicks the shown link or button of the transcript with the text `label`.
    fn click_link(&self, label: &str) {
        let click: Function = self
            .lua
            .load(
                "local wow, label = ... \
                 for _, d in ipairs(wow.Drawn(GnomishRelayTranscript)) do \
                   local button = d.object.parent \
                   if d.text and d.text:find(label, 1, true) and button.kind == 'Button' then \
                     return button:GetScript('OnClick')(button, 'LeftButton') \
                   end \
                 end \
                 error('no link ' .. label)",
            )
            .into_function()
            .unwrap();
        click.call::<()>((self.wow.clone(), label)).unwrap();
    }
}

#[test]
fn a_long_reply_shows_its_first_paragraph_and_a_show_more_link() {
    let game = Game::start();

    game.exchange("fix it", LONG);

    assert!(game.shows("Fixed the flaky test."));
    assert!(!game.shows("The last line."));
    assert!(game.shows("Show more"));
}

#[test]
fn show_more_opens_the_whole_reply_in_place_and_show_less_closes_it() {
    let game = Game::start();
    game.exchange("fix it", LONG);

    game.click_link("Show more");

    assert!(game.shows("The last line."));
    assert!(game.shows("Show less"));
    assert!(!game.shows("Show more"));

    game.click_link("Show less");

    assert!(!game.shows("The last line."));
    assert!(game.shows("Show more"));
}

#[test]
fn a_short_reply_shows_in_full_with_no_link() {
    let game = Game::start();

    game.exchange("status?", "All tests pass.\n\nNothing else changed.");

    assert!(game.shows("Nothing else changed."));
    assert!(!game.shows("Show more"));
}

#[test]
fn a_long_reply_that_starts_with_a_heading_shows_its_first_two_blocks() {
    let game = Game::start();
    let reply = format!("# Result\n\nThe build is green.\n\n{LONG}");

    game.exchange("build", &reply);

    assert!(game.shows("<h1>Result</h1>"));
    assert!(game.shows("The build is green."));
    assert!(!game.shows("Fixed the flaky test."));
}

#[test]
fn a_long_reply_with_few_blocks_but_many_bytes_shows_only_its_summary() {
    let game = Game::start();
    let reply = format!("Short summary.\n\n{}", "word ".repeat(200));

    game.exchange("explain", &reply);

    assert!(game.shows("Short summary."));
    assert!(!game.shows("word word"));
    assert!(game.shows("Show more"));
}

#[test]
fn show_more_leaves_the_entries_above_it_as_they_are() {
    let game = Game::start();
    game.exchange("first", "ok");
    game.exchange("fix it", LONG);
    let first = game.drawn("[You]|r: first");
    let drawn_at: i64 = first.get("textAt").unwrap();
    game.advance(1.0);

    game.click_link("Show more");

    assert_eq!(game.drawn("[You]|r: first"), first);
    assert_eq!(
        first.get::<i64>("textAt").unwrap(),
        drawn_at,
        "no second draw"
    );
    assert!(game.shows("The last line."));
}

#[test]
fn an_opened_reply_comes_back_closed_after_a_reload() {
    let game = Game::start();
    game.exchange("fix it", LONG);
    game.click_link("Show more");

    let game = game.reload();

    assert!(game.shows("Show more"));
    assert!(!game.shows("The last line."));
}

#[test]
fn a_message_below_an_opened_reply_keeps_its_delivery_state() {
    let game = Game::start();
    game.exchange("fix it", LONG);
    game.send("and the docs");

    game.click_link("Show more");

    assert!(game.shows("Sending..."));
    game.answer(Status::Working, Vec::new());
    assert!(game.shows("Delivered"));
    assert!(!game.shows("Sending..."));
}
