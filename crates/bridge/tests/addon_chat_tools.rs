//! The reading and sending tools of the chat window in a fake game (SPEC.md 13.1): the
//! summary of a long reply, quick actions, search, and pinned replies.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{fake_game_for, game_lua_for, load_into, measured, start_addon};
use hmac::{Hmac, Mac};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::cell::decode_cells;
use protocol::frame::{decode_frame, signed_len};
use protocol::markdown::render_markdown;
use protocol::record::{Record, parse_records};
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
    "Notices.lua",
    "Blocks.lua",
    "QuickActions.lua",
    "QuickBar.lua",
    "QuickEditor.lua",
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

    fn shown(&self, name: &str) -> bool {
        self.run(&format!("return {name} ~= nil and {name}:IsVisible()"))
            .as_boolean()
            .unwrap()
    }

    fn click(&self, name: &str) {
        self.run(&format!(
            "{name}:GetScript('OnClick')({name}, 'LeftButton')"
        ));
    }

    /// The records of the last strip on screen, decoded by the Rust decoder with its tag checked.
    fn last_strip(&self) -> Vec<Record> {
        let shots: Table = self.wow.get("shots").unwrap();
        let rows: Table = shots.get(shots.raw_len()).unwrap();
        let mut cells: Vec<u8> = (1..=rows.raw_len())
            .skip(2)
            .flat_map(|row| rows.get::<Vec<u8>>(row).unwrap())
            .collect();
        cells.truncate(cells.len() / 8 * 8);
        let wire = decode_cells(&cells).expect("whole cell groups");
        let frame = decode_frame(&wire).ok().expect("a valid frame");
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(&wire[..signed_len(&frame)]);
        assert_eq!(frame.tag[..], mac.finalize().into_bytes()[..8], "bad tag");
        parse_records(&frame.payload)
            .ok()
            .expect("the payload parses")
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

// Quick actions

fn quick_names(game: &Game) -> Vec<String> {
    (1..=6)
        .filter(|i| game.shown(&format!("GnomishRelayQuick{i}")))
        .map(|i| game.text(&format!("return GnomishRelayQuick{i}.label:GetText()")))
        .collect()
}

fn quick_width(game: &Game, i: usize) -> i64 {
    game.run(&format!("return GnomishRelayQuick{i}:GetWidth()"))
        .as_integer()
        .unwrap()
}

fn open_quick_editor(game: &Game) {
    game.run("local ns = ... ns.Window.ShowTab('settings')");
    game.click("GnomishRelaySettingsQuick");
}

/// Types `text` into the edit box `name`, and ends with the script `end_with`.
fn type_into(game: &Game, name: &str, text: &str, end_with: &str) {
    game.run(&format!(
        "{name}:SetFocus() {name}:SetText({text:?}) {name}:GetScript('{end_with}')({name})"
    ));
}

#[test]
fn the_quick_actions_start_with_the_defaults_in_a_row_above_the_input() {
    let game = Game::start();

    assert_eq!(
        quick_names(&game),
        [
            "Run tests",
            "Fix tests",
            "Git status",
            "Summarize changes",
            "Open PR"
        ]
    );
    let above = game.run(
        "return select(5, GnomishRelayQuickBar:GetPoint()) > select(5, GnomishRelayInput:GetPoint())",
    );
    assert_eq!(above.as_boolean(), Some(true));
}

#[test]
fn a_quick_action_sends_its_message_as_a_signed_strip() {
    let game = Game::start();

    game.click("GnomishRelayQuick1");
    game.advance(1.0);

    let records = game.last_strip();
    assert_eq!(records.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&records[0].text),
        "Run the tests. Tell me what passes and what fails. Change no code."
    );
    assert!(game.shows("[You]|r: Run the tests."));
}

#[test]
fn a_quick_action_shows_its_name_and_message_in_a_tooltip() {
    let game = Game::start();

    game.run("GnomishRelayQuick3:GetScript('OnEnter')(GnomishRelayQuick3)");

    let tooltip = game.text("return GameTooltip.text");
    assert_eq!(tooltip, "Git status");
    assert!(game.run("return GameTooltip.shown").as_boolean().unwrap());
}

#[test]
fn the_quick_actions_row_gives_way_to_the_reload_banner() {
    let game = Game::start();

    game.run(
        "local ns = ... table.insert(ns.Messages.Db().outbox, { chat = 'c', id = 1 }) \
         ns.Window.Refresh()",
    );

    assert!(game.shown("GnomishRelayBanner"));
    assert!(!game.shown("GnomishRelayQuickBar"));
}

#[test]
fn the_quick_actions_row_hides_while_the_input_counts_the_bytes_left() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");

    game.run("GnomishRelayInput:SetText(string.rep('x', 2500))");
    assert!(!game.shown("GnomishRelayQuickBar"));

    game.run("GnomishRelayInput:SetText('')");
    assert!(game.shown("GnomishRelayQuickBar"));
}

#[test]
fn names_that_do_not_fit_share_the_row_equally() {
    let game = Game::start();
    assert_ne!(quick_width(&game, 1), quick_width(&game, 4));

    game.run(
        "local ns = ... ns.QuickActions.Add() \
         ns.QuickActions.Rename(6, 'Deploy to staging now') \
         ns.QuickActions.SetMessage(6, 'Deploy.') ns.Window.Refresh()",
    );

    assert_eq!(quick_names(&game).len(), 6);
    let first = quick_width(&game, 1);
    assert!((2..=6).all(|i| quick_width(&game, i) == first));
}

#[test]
fn the_editor_renames_moves_and_removes_a_quick_action_and_the_row_follows() {
    let game = Game::start();
    open_quick_editor(&game);

    type_into(
        &game,
        "GnomishRelayQuickEditName1",
        "Test",
        "OnEnterPressed",
    );
    game.click("GnomishRelayQuickEditDown1");
    game.click("GnomishRelayQuickEditRemove5");
    game.run("local ns = ... ns.Window.ShowTab('chats')");

    assert_eq!(
        quick_names(&game),
        ["Fix tests", "Test", "Git status", "Summarize changes"]
    );
}

#[test]
fn a_changed_message_saves_when_its_box_loses_the_focus() {
    let game = Game::start();
    open_quick_editor(&game);

    type_into(
        &game,
        "GnomishRelayQuickEditMessage2",
        "Fix only the first failing test.",
        "OnEditFocusLost",
    );

    let message = game.text("local ns = ... return ns.QuickActions.List()[2].message");
    assert_eq!(message, "Fix only the first failing test.");
}

#[test]
fn escape_puts_the_old_name_back_and_an_empty_name_keeps_the_old_one() {
    let game = Game::start();
    open_quick_editor(&game);

    type_into(
        &game,
        "GnomishRelayQuickEditName1",
        "Oops",
        "OnEscapePressed",
    );
    assert_eq!(
        game.text("return GnomishRelayQuickEditName1:GetText()"),
        "Run tests"
    );

    type_into(&game, "GnomishRelayQuickEditName1", "  ", "OnEnterPressed");
    assert_eq!(
        game.text("return GnomishRelayQuickEditName1:GetText()"),
        "Run tests"
    );
    let saved = game.text("local ns = ... return ns.QuickActions.List()[1].name");
    assert_eq!(saved, "Run tests");
}

#[test]
fn add_stops_at_six_and_reset_brings_the_defaults_back() {
    let game = Game::start();
    open_quick_editor(&game);

    game.click("GnomishRelayQuickEditAdd");
    assert_eq!(
        game.text("return GnomishRelayQuickEditName6:GetText()"),
        "New action"
    );
    assert!(!game.shown("GnomishRelayQuickEditAdd"));

    game.click("GnomishRelayQuickEditRemove1");
    game.click("GnomishRelayQuickEditReset");
    assert!(!game.shown("GnomishRelayQuickEditRow6"));
    assert_eq!(
        game.text("return GnomishRelayQuickEditName1:GetText()"),
        "Run tests"
    );
}

#[test]
fn a_new_action_with_no_message_does_not_show_in_the_row() {
    let game = Game::start();
    open_quick_editor(&game);

    game.click("GnomishRelayQuickEditAdd");
    game.click("GnomishRelayQuickEditDone");
    game.run("local ns = ... ns.Window.ShowTab('chats')");

    assert_eq!(quick_names(&game).len(), 5);
}

#[test]
fn done_and_leaving_the_settings_tab_close_the_editor() {
    let game = Game::start();
    open_quick_editor(&game);
    assert!(game.shown("GnomishRelayQuickEdit"));

    game.click("GnomishRelayQuickEditDone");
    assert!(!game.shown("GnomishRelayQuickEdit"));

    game.click("GnomishRelaySettingsQuick");
    game.run("local ns = ... ns.Window.ShowTab('diag') ns.Window.ShowTab('settings')");
    assert!(!game.shown("GnomishRelayQuickEdit"));
}

#[test]
fn the_quick_actions_stay_after_a_reload() {
    let game = Game::start();
    game.run("local ns = ... ns.QuickActions.Remove(1) ns.QuickActions.Move(1, 1)");

    let game = game.reload();

    assert_eq!(
        quick_names(&game),
        ["Git status", "Fix tests", "Summarize changes", "Open PR"]
    );
}

#[test]
fn a_saved_list_that_is_not_a_list_of_names_and_messages_gives_the_defaults() {
    let game = Game::start();
    game.run("local ns = ... ns.Store.db.quickActions = { 'x', { name = 1 } }");

    let game = game.reload();

    assert_eq!(quick_names(&game).len(), 5);
}

#[test]
fn moving_past_either_end_changes_nothing() {
    let game = Game::start();

    game.run(
        "local ns = ... ns.QuickActions.Move(1, -1) ns.QuickActions.Move(5, 1) \
         ns.Window.Refresh()",
    );

    assert_eq!(quick_names(&game)[0], "Run tests");
    assert_eq!(quick_names(&game)[4], "Open PR");
}
