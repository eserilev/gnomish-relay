//! Git in a chat in the fake game (SPEC.md 9.10, 13.1): the Own branch box, the branch
//! bar, and the git messages of the player.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{fake_game_for, game_lua_for, load_into, measured, start_addon};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::cell::decode_cells;
use protocol::frame::decode_frame;
use protocol::record::{Record, parse_records};
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
    "Changes.lua",
    "Transcript.lua",
    "Folders.lua",
    "Browser.lua",
    "GitBar.lua",
    "BridgeSettings.lua",
    "RulesGroup.lua",
    "SettingsTab.lua",
    "DiagTab.lua",
    "Window.lua",
    "Popup.lua",
    "NoticeFrames.lua",
    "Core.lua",
];
/// A reply of a run on its own branch.
const REPLY: &str = "\x1bM1\nB\x1fgnomish/fix\x1f1\x1fmain\np\x1fDone.\n";

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

impl Game {
    fn start() -> Game {
        let fake = measured();
        let lua = game_lua_for(&fake);
        let wow = fake_game_for(&lua, "addon/tests/api.lua", &fake);
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        start_addon(&lua, &wow, &fake, "GnomishRelay", None, || {
            load_into(&lua, &ns, FILES);
        });
        let game = Game { lua, wow, ns };
        game.run("local ns = ... ns.Window.Open()");
        game
    }

    fn run(&self, code: &str) -> Value {
        self.lua.load(code).call(self.ns.clone()).unwrap()
    }

    fn advance(&self, seconds: f64) {
        self.wow
            .get::<Function>("Advance")
            .unwrap()
            .call::<()>(seconds)
            .unwrap();
    }

    fn send(&self, text: &str) {
        self.run(&format!("local ns = ... ns.Window.Send({text:?})"));
        self.advance(1.0);
    }

    fn chat(&self) -> Table {
        let db: Table = self.lua.globals().get("GnomishRelayDB").unwrap();
        db.get::<Table>("chats").unwrap().get::<Table>(1).unwrap()
    }

    fn chat_id(&self) -> String {
        self.chat().get("id").unwrap()
    }

    /// The id of the last message of the chat.
    fn last_id(&self) -> u32 {
        let history: Table = self.chat().get("history").unwrap();
        (1..=history.raw_len())
            .rev()
            .map(|i| history.get::<Table>(i).unwrap())
            .find(|e| e.get::<String>("role").unwrap() == "user")
            .unwrap()
            .get("id")
            .unwrap()
    }

    /// Puts one reply into every slot, and polls.
    fn reply(&self, id: u32, status: Status, text: &str) {
        let now = u32::try_from(self.run("return time()").as_integer().unwrap()).unwrap();
        let reply = Reply {
            chat: self.chat_id().into_bytes(),
            id,
            status,
            text: text.as_bytes().to_vec(),
        };
        let body = slot_body(App::Relay, now, &prepare_replies(&[reply]));
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
        self.advance(5.0);
        self.run("local ns = ... ns.Window.Refresh()");
    }

    /// The records of the last screenshot, with the calibration rows left out.
    fn last_strip(&self) -> Vec<Record> {
        let shots: Table = self.wow.get("shots").unwrap();
        let rows: Table = shots.get(shots.raw_len()).unwrap();
        let mut cells: Vec<u8> = (1..=rows.raw_len())
            .skip(2)
            .flat_map(|r| rows.get::<Vec<u8>>(r).unwrap())
            .collect();
        cells.truncate(cells.len() / 8 * 8);
        let wire = decode_cells(&cells).unwrap();
        let frame = decode_frame(&wire).ok().unwrap();
        parse_records(&frame.payload).ok().unwrap()
    }

    /// The record of the last message of the chat in the last strip, not a list request.
    fn message_record(&self) -> Record {
        let id = self.last_id();
        self.last_strip().into_iter().find(|r| r.id == id).unwrap()
    }

    /// The texts that the transcript shows.
    fn texts(&self) -> Vec<String> {
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

    fn shown(&self, name: &str) -> bool {
        self.run(&format!("return {name} ~= nil and {name}:IsShown()"))
            .as_boolean()
            .unwrap()
    }

    fn dialog(&self) -> Option<String> {
        self.wow
            .get::<Option<Table>>("dialog")
            .unwrap()
            .map(|d| d.get("text").unwrap())
    }

    fn press_in_dialog(&self, button: &str) {
        self.wow
            .get::<Function>("PressInDialog")
            .unwrap()
            .call::<()>(button)
            .unwrap();
        self.advance(1.0);
    }

    fn whispers(&self) -> usize {
        let printed: Vec<String> = self.wow.get("printed").unwrap();
        printed.iter().filter(|l| l.contains("whispers:")).count()
    }
}

fn flags(record: &Record) -> String {
    String::from_utf8_lossy(&record.flags).into_owned()
}

/// A chat with one message and a reply of its run.
fn game_with_reply() -> (Game, u32) {
    let game = Game::start();
    game.send("fix the flaky test\nand explain it");
    let id = game.last_id();
    game.reply(id, Status::Done, REPLY);
    (game, id)
}

/// A folder tree where `app` is a repository, and a new chat that picked it.
fn game_in_repository() -> Game {
    let game = Game::start();
    game.run(
        "local ns = ... ns.Store.db.folders = { id = 1, text = '~/Code\\n0\\t~/Code\\t\\n1\\tapp\\tg' }",
    );
    game.run("local ns = ... ns.Window.NewChat() ns.Window.ChooseFolder('app', 'app', false)");
    game
}

#[test]
fn a_new_chat_in_a_repository_offers_own_branch_and_sends_it_with_every_message() {
    let game = game_in_repository();
    assert!(game.shown("GnomishRelayOwnBranch"));
    assert!(
        !game
            .run("return GnomishRelayOwnBranch:GetChecked()")
            .as_boolean()
            .unwrap_or(false)
    );

    game.run("GnomishRelayOwnBranch:Click()");
    game.send("go");
    game.advance(2.0);

    let record = game.message_record();
    assert!(flags(&record).contains("branch=1"), "{}", flags(&record));
    assert!(!game.shown("GnomishRelayOwnBranch"));
}

#[test]
fn own_branch_starts_on_when_another_chat_has_the_folder() {
    let game = game_in_repository();
    game.send("first chat");

    game.run("local ns = ... ns.Window.NewChat() ns.Window.ChooseFolder('app', 'app', false)");

    assert!(game.shown("GnomishRelayOwnBranch"));
    assert!(
        game.run("return GnomishRelayOwnBranch:GetChecked()")
            .as_boolean()
            .unwrap()
    );
}

#[test]
fn a_folder_that_is_no_repository_offers_no_own_branch() {
    let game = Game::start();
    game.run(
        "local ns = ... ns.Store.db.folders = { id = 1, text = '~/Code\\n0\\t~/Code\\t\\n1\\tnotes\\t' }",
    );

    game.run("local ns = ... ns.Window.NewChat() ns.Window.ChooseFolder('notes', 'notes', false)");

    assert!(!game.shown("GnomishRelayOwnBranch"));
}

#[test]
fn an_own_branch_shows_merge_and_discard_and_discard_asks_first() {
    let (game, _) = game_with_reply();

    assert!(game.shown("GnomishRelayGitMerge"));
    assert!(game.shown("GnomishRelayGitBranch"));
    game.run("GnomishRelayGitDiscard:Click()");

    assert_eq!(
        game.dialog().as_deref(),
        Some("Discard this chat's branch? This deletes gnomish/fix and its folder.")
    );
    game.press_in_dialog("button1");
    assert!(flags(&game.message_record()).starts_with("git=discard;"));
}

#[test]
fn merge_sends_a_merge_that_shows_as_a_message_of_the_player() {
    let (game, _) = game_with_reply();

    game.run("GnomishRelayGitMerge:Click()");
    game.advance(1.0);

    assert!(flags(&game.message_record()).starts_with("git=merge;"));
    assert!(game.texts().iter().any(|t| t.contains("[You]|r: Merge")));
}

#[test]
fn a_plain_branch_shows_its_name_and_no_merge() {
    let game = Game::start();
    game.send("go");

    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nB\x1fmain\x1f0\x1f\np\x1fDone.\n",
    );

    assert!(game.shown("GnomishRelayGitBranch"));
    assert!(!game.shown("GnomishRelayGitMerge"));
}

#[test]
fn the_answer_to_a_discard_is_a_relay_line_with_no_whisper_and_ends_the_branch() {
    let (game, _) = game_with_reply();
    let whispers = game.whispers();
    game.run("GnomishRelayGitDiscard:Click()");
    game.press_in_dialog("button1");

    game.reply(game.last_id(), Status::Done, "Discarded gnomish/fix.");

    let texts = game.texts().join("\n");
    assert!(texts.contains("[Relay]: Discarded gnomish/fix."), "{texts}");
    assert_eq!(game.whispers(), whispers);
    assert!(!game.shown("GnomishRelayGitMerge"));
}
