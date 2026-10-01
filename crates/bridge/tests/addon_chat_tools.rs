//! The reading and sending tools of the chat window in a fake game (SPEC.md 13.1): the
//! summary of a long reply, the quick actions as suggestions, search, pinned replies, and
//! earlier messages in the input.

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
    "KeyHandoff.lua",
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
    "Pins.lua",
    "Search.lua",
    "QuickActions.lua",
    "Suggestions.lua",
    "QuickEditor.lua",
    "Changes.lua",
    "Transcript.lua",
    "Folders.lua",
    "Browser.lua",
    "GitBar.lua",
    "DesktopRequest.lua",
    "BridgeSettings.lua",
    "RulesGroup.lua",
    "SettingsTab.lua",
    "DiagTab.lua",
    "InputHistory.lua",
    "LevelMenu.lua",
    "Window.lua",
    "Popup.lua",
    "NoticeFrames.lua",
    "SetupNeeded.lua",
    "Core.lua",
];

/// Eleven paragraphs: a reply with more than 8 blocks.
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
fn a_long_reply_draws_all_its_blocks_and_no_show_more_link() {
    let game = Game::start();
    let paragraphs: Vec<String> = (1..=10)
        .map(|n| format!("Paragraph {n}: {}", "word ".repeat(20)))
        .collect();
    let reply = paragraphs.join("\n\n");
    assert!(reply.len() > 800, "more than 800 bytes");

    game.exchange("explain", &reply);

    for n in 1..=10 {
        assert!(game.shows(&format!("Paragraph {n}:")), "paragraph {n}");
    }
    assert!(!game.shows("Show more"));
}

// Quick actions: suggestions in an empty chat

const SUGGESTED: [&str; 5] = [
    "Run the tests and tell me what fails",
    "Fix the failing tests",
    "Show git status",
    "Summarize my uncommitted changes",
    "Open a pull request for these changes",
];

fn suggestions(game: &Game) -> Vec<String> {
    (1..=6)
        .filter(|i| game.shown(&format!("GnomishRelaySuggestion{i}")))
        .map(|i| game.text(&format!("return GnomishRelaySuggestion{i}.label:GetText()")))
        .collect()
}

fn width_of(game: &Game, name: &str) -> i64 {
    game.run(&format!("return {name}:GetWidth()"))
        .as_integer()
        .unwrap()
}

fn height_of(game: &Game, name: &str) -> i64 {
    game.run(&format!("return {name}:GetHeight()"))
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

fn show_reload_banner(game: &Game) {
    game.run(
        "local ns = ... table.insert(ns.Messages.Db().outbox, { chat = 'c', id = 1 }) \
         ns.Window.Refresh()",
    );
}

#[test]
fn an_empty_chat_suggests_the_quick_actions_by_their_messages() {
    let game = Game::start();

    assert!(game.shown("GnomishRelaySuggestionsHint"));
    assert_eq!(
        game.text("return GnomishRelaySuggestionsHint:GetText()"),
        "Try one of these:"
    );
    assert_eq!(suggestions(&game), SUGGESTED);
}

#[test]
fn a_suggestion_sends_its_message_as_a_signed_strip() {
    let game = Game::start();

    game.click("GnomishRelaySuggestion1");
    game.advance(1.0);

    let records = game.last_strip();
    assert_eq!(records.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&records[0].text),
        "Run the tests and tell me what fails"
    );
    assert!(game.shows("[You]|r: Run the tests and tell me what fails"));
}

#[test]
fn a_suggestion_shows_its_whole_message_in_a_tooltip() {
    let game = Game::start();
    let long = "Run the tests, fix each failure at its cause, and run them again. ".repeat(4);
    game.run(&format!(
        "local ns = ... ns.QuickActions.SetMessage(3, {long:?}) ns.Window.Refresh()"
    ));

    game.run("GnomishRelaySuggestion3:GetScript('OnEnter')(GnomishRelaySuggestion3)");

    assert_eq!(game.text("return GameTooltip.text"), long.trim());
    assert!(game.run("return GameTooltip.shown").as_boolean().unwrap());
}

#[test]
fn the_suggestions_hide_after_the_first_message_of_the_chat() {
    let game = Game::start();

    game.send("hello");

    assert!(!game.shown("GnomishRelaySuggestions"));
    assert!(suggestions(&game).is_empty());
}

#[test]
fn a_new_chat_suggests_the_quick_actions_again() {
    let game = Game::start();
    game.send("hello");

    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");

    assert_eq!(suggestions(&game), SUGGESTED);
}

#[test]
fn the_suggestions_stay_hidden_after_a_reload() {
    let game = Game::start();
    game.send("hello");

    let game = game.reload();

    assert!(game.shows("hello"));
    assert!(!game.shown("GnomishRelaySuggestions"));
}

#[test]
fn no_row_of_quick_actions_sits_above_the_input() {
    let game = Game::start();

    let gone = game.run("return GnomishRelayQuickBar == nil and GnomishRelayQuick1 == nil");

    assert_eq!(gone.as_boolean(), Some(true));
}

#[test]
fn the_suggestion_rows_span_the_transcript_at_every_window_size() {
    let game = Game::start();
    let small = width_of(&game, "GnomishRelaySuggestion1");
    assert!(small * 10 > width_of(&game, "GnomishRelayScroll") * 8);
    assert!(width_of(&game, "GnomishRelaySuggestion1.label") < small);
    assert!(height_of(&game, "GnomishRelaySuggestions") < height_of(&game, "GnomishRelayScroll"));

    game.run(
        "GnomishRelayFrame:SetSize(1400, 700) \
         GnomishRelayResizeGrip:GetScript('OnMouseUp')(GnomishRelayResizeGrip)",
    );

    assert_eq!(width_of(&game, "GnomishRelaySuggestion1"), small + 500);
    assert!(width_of(&game, "GnomishRelaySuggestion1.label") < small + 500);
}

#[test]
fn six_suggestions_fit_in_the_smallest_window() {
    let game = Game::start();
    game.run(
        "local ns = ... ns.QuickActions.Add() ns.QuickActions.SetMessage(6, 'Deploy.') \
         ns.Window.Refresh()",
    );
    show_reload_banner(&game);

    assert_eq!(suggestions(&game).len(), 6);
    assert!(height_of(&game, "GnomishRelaySuggestions") < height_of(&game, "GnomishRelayScroll"));
}

#[test]
fn with_no_quick_action_an_empty_chat_shows_no_hint() {
    let game = Game::start();

    game.run("local ns = ... for _ = 1, 5 do ns.QuickActions.Remove(1) end ns.Window.Refresh()");

    assert!(!game.shown("GnomishRelaySuggestionsHint"));
    assert!(suggestions(&game).is_empty());
}

#[test]
fn the_transcript_takes_the_row_above_the_input_until_the_reload_banner_needs_it() {
    let game = Game::start();
    let tall = height_of(&game, "GnomishRelayScroll");

    show_reload_banner(&game);
    assert!(game.shown("GnomishRelayBanner"));
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall - 28);

    game.run("local ns = ... ns.Messages.Db().outbox = {} ns.Window.Refresh()");
    assert!(!game.shown("GnomishRelayBanner"));
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall);
}

#[test]
fn the_byte_counter_takes_the_row_back_from_the_transcript() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");
    let tall = height_of(&game, "GnomishRelayScroll");

    game.run("GnomishRelayInput:SetText(string.rep('x', 2500))");
    assert!(game.shown("GnomishRelayInputCount"));
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall - 28);

    game.run("GnomishRelayInput:SetText('')");
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall);
}

#[test]
fn the_newest_line_stays_in_view_when_the_banner_takes_the_row() {
    let game = Game::start();
    for _ in 0..40 {
        game.send("more");
    }
    let bottom = scroll(&game);

    show_reload_banner(&game);

    assert_eq!(scroll(&game), bottom + 28);
}

#[test]
fn the_editor_changes_moves_and_removes_a_quick_action_and_the_suggestions_follow() {
    let game = Game::start();
    open_quick_editor(&game);

    type_into(
        &game,
        "GnomishRelayQuickEditMessage1",
        "Run only the unit tests",
        "OnEnterPressed",
    );
    game.click("GnomishRelayQuickEditDown1");
    game.click("GnomishRelayQuickEditRemove5");
    game.run("local ns = ... ns.Window.ShowTab('chats')");

    assert_eq!(
        suggestions(&game),
        [
            "Fix the failing tests",
            "Run only the unit tests",
            "Show git status",
            "Summarize my uncommitted changes"
        ]
    );
}

#[test]
fn the_editor_has_no_box_for_the_name_that_no_screen_shows() {
    let game = Game::start();

    open_quick_editor(&game);

    assert!(game.shown("GnomishRelayQuickEditMessage1"));
    let gone = game.run("return GnomishRelayQuickEditName1 == nil");
    assert_eq!(gone.as_boolean(), Some(true));
}

#[test]
fn move_up_keeps_a_message_that_is_still_being_typed() {
    let game = Game::start();
    open_quick_editor(&game);
    game.run(
        "GnomishRelayQuickEditMessage2:SetFocus() GnomishRelayQuickEditMessage2:SetText('Fix it')",
    );

    game.click("GnomishRelayQuickEditUp2");

    let message = game.text("local ns = ... return ns.QuickActions.List()[1].message");
    assert_eq!(message, "Fix it");
    assert_eq!(
        game.text("return GnomishRelayQuickEditMessage1:GetText()"),
        "Fix it"
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
fn escape_puts_the_old_message_back_and_an_empty_message_keeps_the_old_one() {
    let game = Game::start();
    open_quick_editor(&game);

    type_into(
        &game,
        "GnomishRelayQuickEditMessage1",
        "Oops",
        "OnEscapePressed",
    );
    assert_eq!(
        game.text("return GnomishRelayQuickEditMessage1:GetText()"),
        SUGGESTED[0]
    );

    type_into(
        &game,
        "GnomishRelayQuickEditMessage1",
        "  ",
        "OnEnterPressed",
    );
    assert_eq!(
        game.text("return GnomishRelayQuickEditMessage1:GetText()"),
        SUGGESTED[0]
    );
    let saved = game.text("local ns = ... return ns.QuickActions.List()[1].message");
    assert_eq!(saved, SUGGESTED[0]);
}

#[test]
fn add_stops_at_six_and_reset_brings_the_defaults_back() {
    let game = Game::start();
    open_quick_editor(&game);

    game.click("GnomishRelayQuickEditAdd");
    assert!(game.shown("GnomishRelayQuickEditRow6"));
    assert_eq!(
        game.text("return GnomishRelayQuickEditMessage6:GetText()"),
        ""
    );
    assert!(game.shown("GnomishRelayQuickEditMessage6"));
    assert!(!game.shown("GnomishRelayQuickEditAdd"));

    game.click("GnomishRelayQuickEditRemove1");
    game.click("GnomishRelayQuickEditReset");
    assert!(!game.shown("GnomishRelayQuickEditRow6"));
    assert_eq!(
        game.text("return GnomishRelayQuickEditMessage1:GetText()"),
        SUGGESTED[0]
    );
}

#[test]
fn a_new_action_with_no_message_is_not_suggested() {
    let game = Game::start();
    open_quick_editor(&game);

    game.click("GnomishRelayQuickEditAdd");
    game.click("GnomishRelayQuickEditDone");
    game.run("local ns = ... ns.Window.ShowTab('chats')");

    assert_eq!(suggestions(&game).len(), 5);
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
        suggestions(&game),
        [SUGGESTED[2], SUGGESTED[1], SUGGESTED[3], SUGGESTED[4]]
    );
}

#[test]
fn a_saved_list_of_names_and_messages_from_0_3_0_still_loads() {
    let game = Game::start();
    game.run(
        "local ns = ... ns.Store.db.quickActions = \
         { { name = 'Deploy', message = 'Deploy to staging and tell me the link.' } }",
    );

    let game = game.reload();

    assert_eq!(
        suggestions(&game),
        ["Deploy to staging and tell me the link."]
    );
}

#[test]
fn the_unchanged_defaults_of_0_3_0_become_the_new_defaults() {
    let game = Game::start();
    game.run(
        "local ns = ... ns.Store.db.quickActions = { \
         { name = 'Run tests', message = 'Run the tests. Tell me what passes and what fails. Change no code.' }, \
         { name = 'Fix tests', message = 'Run the tests and fix each failure at its cause. Then run the tests again.' }, \
         { name = 'Git status', message = 'Show the git status: the branch and the changed files. Change nothing.' }, \
         { name = 'Summarize changes', message = 'Summarize the changes that are not committed yet: what changed and why. Change nothing.' }, \
         { name = 'Open PR', message = 'Commit the changes on a new branch, push it, and open a pull request with a short title and description. Tell me the link.' } }",
    );

    let game = game.reload();

    assert_eq!(suggestions(&game), SUGGESTED);
}

#[test]
fn a_saved_list_that_is_not_a_list_of_names_and_messages_gives_the_defaults() {
    let game = Game::start();
    game.run("local ns = ... ns.Store.db.quickActions = { 'x', { name = 1 } }");

    let game = game.reload();

    assert_eq!(suggestions(&game), SUGGESTED);
}

#[test]
fn moving_past_either_end_changes_nothing() {
    let game = Game::start();

    game.run(
        "local ns = ... ns.QuickActions.Move(1, -1) ns.QuickActions.Move(5, 1) \
         ns.Window.Refresh()",
    );

    assert_eq!(suggestions(&game), SUGGESTED);
}

// Pinned replies

fn pinned_rows(game: &Game) -> Vec<String> {
    (1..=12)
        .filter(|i| game.shown(&format!("GnomishRelayPinnedRow{i}")))
        .map(|i| game.text(&format!("return GnomishRelayPinnedRow{i}.text:GetText()")))
        .collect()
}

fn scroll(game: &Game) -> i64 {
    game.run("return GnomishRelayScroll:GetVerticalScroll()")
        .as_integer()
        .unwrap()
}

#[test]
fn pin_marks_a_reply_and_the_pinned_list_of_the_chat_shows_it() {
    let game = Game::start();
    game.exchange("fix it", LONG);
    assert_eq!(
        game.text("return GnomishRelayPinnedButton.label:GetText()"),
        "Pinned 0"
    );

    game.click_link("Pin");

    assert!(game.shows("Unpin"));
    assert_eq!(
        game.text("return GnomishRelayPinnedButton.label:GetText()"),
        "Pinned 1"
    );
    game.click("GnomishRelayPinnedButton");
    assert_eq!(pinned_rows(&game), ["Fixed the flaky test."]);
}

#[test]
fn only_agent_replies_have_a_pin() {
    let game = Game::start();
    game.send("hello");
    game.answer(Status::Error, b"Not sent.".to_vec());

    assert!(!game.shows("Pin"));
}

#[test]
fn unpin_takes_the_reply_off_the_list() {
    let game = Game::start();
    game.exchange("fix it", "Done.");
    game.click_link("Pin");

    game.click_link("Unpin");

    assert!(!game.shows("Unpin"));
    game.click("GnomishRelayPinnedButton");
    assert!(pinned_rows(&game).is_empty());
    assert_eq!(
        game.text("return GnomishRelayPinnedEmpty:GetText()"),
        "No pinned replies yet. Click Pin on a reply to keep it here."
    );
}

#[test]
fn a_pinned_reply_shows_its_codes_as_text() {
    let game = Game::start();
    game.exchange("fix it", "Done |cff00ff00ok|r");
    game.click_link("Pin");

    game.click("GnomishRelayPinnedButton");

    assert_eq!(pinned_rows(&game), ["Done ||cff00ff00ok||r"]);
}

#[test]
fn a_click_on_a_pinned_reply_jumps_to_it_and_marks_it() {
    let game = Game::start();
    game.exchange("fix it", LONG);
    game.click_link("Pin");
    for _ in 0..40 {
        game.send("more");
    }
    let bottom = scroll(&game);
    game.click("GnomishRelayPinnedButton");

    game.click("GnomishRelayPinnedRow1");

    assert!(scroll(&game) < bottom);
    assert!(game.shown("GnomishRelayMark"));
    assert!(!game.shown("GnomishRelayPinnedList"));
}

#[test]
fn the_pinned_list_shows_twelve_rows_and_the_wheel_scrolls_it() {
    let game = Game::start();
    for n in 1..=13 {
        game.exchange("next", &format!("Reply {n}"));
    }
    game.run(
        "local ns = ... for _, e in ipairs(ns.Window.SelectedChat().history) do \
           if e.role == 'agent' then e.pinned = true end end",
    );
    game.click("GnomishRelayPinnedButton");
    assert_eq!(pinned_rows(&game).len(), 12);
    assert_eq!(pinned_rows(&game)[0], "Reply 1");

    game.run("GnomishRelayPinnedList:GetScript('OnMouseWheel')(GnomishRelayPinnedList, -1)");

    assert_eq!(pinned_rows(&game)[0], "Reply 2");
    assert_eq!(pinned_rows(&game)[11], "Reply 13");
}

#[test]
fn a_click_outside_closes_the_pinned_list() {
    let game = Game::start();
    game.click("GnomishRelayPinnedButton");
    assert!(game.shown("GnomishRelayPinnedList"));

    game.run(
        "GnomishRelayPinnedList:GetScript('OnEvent')(GnomishRelayPinnedList, 'GLOBAL_MOUSE_DOWN')",
    );

    assert!(!game.shown("GnomishRelayPinnedList"));
}

#[test]
fn pins_stay_after_a_reload_and_each_chat_has_its_own() {
    let game = Game::start();
    game.exchange("fix it", "Fixed.");
    game.click_link("Pin");

    let game = game.reload();
    game.click("GnomishRelayPinnedButton");
    assert_eq!(pinned_rows(&game), ["Fixed."]);

    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");
    assert_eq!(
        game.text("return GnomishRelayPinnedButton.label:GetText()"),
        "Pinned 0"
    );
}

// Search

/// The first text of the entry that the gold band marks.
fn marked(game: &Game) -> String {
    let find: Function = game
        .lua
        .load(
            "local wow = ... \
             if not GnomishRelayMark:IsShown() then return 'nothing' end \
             local drawn, top = wow.Drawn(GnomishRelayTranscript) \
             for _, d in ipairs(drawn) do \
               if d.object == GnomishRelayMark then top = d.y + 2 end \
             end \
             for _, d in ipairs(drawn) do \
               if d.y == top and d.text then return d.text end \
             end",
        )
        .into_function()
        .unwrap();
    find.call(game.wow.clone()).unwrap()
}

fn search_for(game: &Game, text: &str) {
    game.click("GnomishRelaySearchButton");
    game.run(&format!("GnomishRelaySearchBox:SetText({text:?})"));
}

fn search_count(game: &Game) -> String {
    game.text("return GnomishRelaySearchCount:GetText()")
}

/// Three exchanges: the word "flaky" is in the first reply and in the last message.
fn three_exchanges(game: &Game) {
    game.exchange("why does ci fail?", "A flaky test.");
    game.exchange("and the docs?", "They are fine.");
    game.exchange("fix the flaky one", "Fixed.");
}

#[test]
fn search_opens_a_bar_with_the_focus_in_the_row_above_the_input() {
    let game = Game::start();
    let tall = height_of(&game, "GnomishRelayScroll");

    game.click("GnomishRelaySearchButton");

    assert!(game.shown("GnomishRelaySearch"));
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall - 28);
    let focused = game.run("return GnomishRelaySearchBox:HasFocus()");
    assert_eq!(focused.as_boolean(), Some(true));
    assert_eq!(
        game.text("return GnomishRelaySearchHint:GetText()"),
        "Search this chat"
    );
}

#[test]
fn a_search_ignores_case_and_jumps_to_the_newest_match() {
    let game = Game::start();
    three_exchanges(&game);

    search_for(&game, "FLAKY");

    assert_eq!(search_count(&game), "2 of 2");
    assert_eq!(marked(&game), "|cff69ccf0[You]|r: fix the flaky one");
}

#[test]
fn previous_and_next_step_through_the_matches_and_wrap_around() {
    let game = Game::start();
    three_exchanges(&game);
    search_for(&game, "flaky");

    game.click("GnomishRelaySearchPrevious");
    assert_eq!(search_count(&game), "1 of 2");
    assert!(marked(&game).starts_with("|cffff7d0a[Claude]"));

    game.click("GnomishRelaySearchPrevious");
    assert_eq!(search_count(&game), "2 of 2");

    game.click("GnomishRelaySearchNext");
    assert_eq!(search_count(&game), "1 of 2");
}

#[test]
fn a_search_with_no_match_says_so_and_marks_nothing() {
    let game = Game::start();
    three_exchanges(&game);

    search_for(&game, "banana");

    assert_eq!(search_count(&game), "No matches");
    assert_eq!(marked(&game), "nothing");
}

#[test]
fn a_match_in_the_last_paragraph_of_a_long_reply_marks_the_reply() {
    let game = Game::start();
    game.exchange("fix it", LONG);
    game.exchange("thanks", "You're welcome.");

    search_for(&game, "last line");

    assert_eq!(search_count(&game), "1 of 1");
    assert!(marked(&game).starts_with("|cffff7d0a[Claude]"));
}

#[test]
fn enter_gives_the_keys_back_and_keeps_the_bar() {
    let game = Game::start();
    three_exchanges(&game);
    search_for(&game, "flaky");

    game.run("GnomishRelaySearchBox:GetScript('OnEnterPressed')(GnomishRelaySearchBox)");

    let focused = game.run("return GnomishRelaySearchBox:HasFocus()");
    assert_eq!(focused.as_boolean(), Some(false));
    assert!(game.shown("GnomishRelaySearch"));
    assert_eq!(search_count(&game), "2 of 2");
}

#[test]
fn escape_closes_the_search_and_removes_the_mark() {
    let game = Game::start();
    three_exchanges(&game);
    let tall = height_of(&game, "GnomishRelayScroll");
    search_for(&game, "flaky");

    game.run("GnomishRelaySearchBox:GetScript('OnEscapePressed')(GnomishRelaySearchBox)");

    assert!(!game.shown("GnomishRelaySearch"));
    assert_eq!(marked(&game), "nothing");
    assert_eq!(height_of(&game, "GnomishRelayScroll"), tall);
    let focused = game.run("return GnomishRelaySearchBox:HasFocus()");
    assert_eq!(focused.as_boolean(), Some(false));
}

#[test]
fn close_and_a_change_of_chat_close_the_search() {
    let game = Game::start();
    three_exchanges(&game);
    search_for(&game, "flaky");
    game.click("GnomishRelaySearchClose");
    assert!(!game.shown("GnomishRelaySearch"));

    search_for(&game, "flaky");
    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");

    assert!(!game.shown("GnomishRelaySearch"));
}

#[test]
fn the_search_bar_comes_before_the_reload_banner() {
    let game = Game::start();
    game.run(
        "local ns = ... table.insert(ns.Messages.Db().outbox, { chat = 'c', id = 1 }) \
         ns.Window.Refresh()",
    );

    game.click("GnomishRelaySearchButton");

    assert!(game.shown("GnomishRelaySearch"));
    assert!(!game.shown("GnomishRelayBanner"));
}

#[test]
fn the_search_key_binding_opens_the_window_and_the_search() {
    let game = Game::start();
    game.run("GnomishRelayFrame:Hide()");

    game.run("GnomishRelay_Search()");

    assert!(game.shown("GnomishRelayFrame"));
    assert!(game.shown("GnomishRelaySearch"));
    let focused = game.run("return GnomishRelaySearchBox:HasFocus()");
    assert_eq!(focused.as_boolean(), Some(true));
}

#[test]
fn a_long_folder_name_leaves_room_at_the_right_of_the_header() {
    let game = Game::start();
    game.send("hi");

    game.run(&format!(
        "local ns = ... ns.Window.SelectedChat().cwd = '/home/{}' ns.Window.Refresh()",
        "very-long-folder-name/".repeat(10)
    ));

    let width = game
        .run("return GnomishRelayFolderButton:GetWidth()")
        .as_integer()
        .unwrap();
    assert!(width <= 900 - 400 - 28 - 170 - 140, "{width}");
}

/// Types `text` in the input and presses Enter, as the player sends a message.
fn send_typed(game: &Game, text: &str) {
    type_into(game, "GnomishRelayInput", text, "OnEnterPressed");
}

/// Presses an arrow key in the input: "UP" or "DOWN".
fn press(game: &Game, key: &str) {
    game.run(&format!(
        "GnomishRelayInput:SetFocus() \
         GnomishRelayInput:GetScript('OnArrowPressed')(GnomishRelayInput, '{key}')"
    ));
}

fn input_text(game: &Game) -> String {
    game.text("return GnomishRelayInput:GetText()")
}

#[test]
fn up_shows_the_last_sent_message_with_the_cursor_at_its_end() {
    let game = Game::start();
    send_typed(&game, "run the tests");

    press(&game, "UP");

    assert_eq!(input_text(&game), "run the tests");
    let cursor = game.run("return GnomishRelayInput:GetCursorPosition()");
    assert_eq!(cursor.as_integer(), Some(13));
}

#[test]
fn each_up_goes_one_message_further_back_and_down_comes_forward() {
    let game = Game::start();
    send_typed(&game, "first");
    send_typed(&game, "second");
    send_typed(&game, "third");

    press(&game, "UP");
    press(&game, "UP");
    press(&game, "UP");
    assert_eq!(input_text(&game), "first");
    press(&game, "UP");
    assert_eq!(input_text(&game), "first", "Up at the oldest does nothing");
    press(&game, "DOWN");
    assert_eq!(input_text(&game), "second");
}

#[test]
fn down_past_the_newest_message_puts_back_the_typed_text() {
    let game = Game::start();
    send_typed(&game, "first");
    game.run("GnomishRelayInput:SetText('half a thought')");

    press(&game, "UP");
    assert_eq!(input_text(&game), "first");
    press(&game, "DOWN");
    assert_eq!(input_text(&game), "half a thought");
    press(&game, "DOWN");
    assert_eq!(input_text(&game), "half a thought");
}

#[test]
fn each_chat_recalls_only_its_own_messages() {
    let game = Game::start();
    send_typed(&game, "in the first chat");
    let first = game.chat_id();
    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");

    press(&game, "UP");
    assert_eq!(input_text(&game), "");

    game.run(&format!("local ns = ... ns.Window.Open('{first}')"));
    press(&game, "UP");
    assert_eq!(input_text(&game), "in the first chat");
}

#[test]
fn a_send_starts_again_at_the_newest_message() {
    let game = Game::start();
    send_typed(&game, "first");
    send_typed(&game, "second");
    press(&game, "UP");
    press(&game, "UP");

    send_typed(&game, "third");
    press(&game, "UP");

    assert_eq!(input_text(&game), "third");
}

#[test]
fn a_change_of_chat_starts_again_at_the_newest_message() {
    let game = Game::start();
    send_typed(&game, "first");
    send_typed(&game, "second");
    let chat = game.chat_id();
    press(&game, "UP");
    press(&game, "UP");

    game.run("local ns = ... ns.Window.NewChat() ns.Window.CloseBrowser()");
    game.run(&format!("local ns = ... ns.Window.Open('{chat}')"));
    game.run("GnomishRelayInput:SetText('')");
    press(&game, "UP");

    assert_eq!(input_text(&game), "second");
}

#[test]
fn the_earlier_messages_stay_after_a_reload() {
    let game = Game::start();
    send_typed(&game, "first");
    send_typed(&game, "second");

    let game = game.reload();
    press(&game, "UP");
    press(&game, "UP");

    assert_eq!(input_text(&game), "first");
}

#[test]
fn same_messages_in_a_row_count_once_and_a_failed_message_counts() {
    let game = Game::start();
    send_typed(&game, "first");
    send_typed(&game, "again");
    send_typed(&game, "again");
    game.answer(Status::Error, b"Not sent.".to_vec());

    press(&game, "UP");
    assert_eq!(input_text(&game), "again");
    press(&game, "UP");
    assert_eq!(input_text(&game), "first");
}

#[test]
fn a_commit_message_does_not_count() {
    let game = Game::start();
    send_typed(&game, "first");
    game.run("local ns = ... ns.Transport.Git(ns.Window.SelectedChat(), 'commit', 'Fix the bug')");

    press(&game, "UP");

    assert_eq!(input_text(&game), "first");
}

#[test]
fn the_input_gets_the_arrow_keys_without_alt() {
    let game = Game::start();

    let mode = game.run("return GnomishRelayInput.altArrowKeyMode");

    assert_eq!(mode.as_boolean(), Some(false));
}
