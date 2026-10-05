//! Git in a chat in the fake game (SPEC.md 9.11, 13.1): the change block with Commit and
//! Revert, the test and CI lines, the Own branch box, the branch bar, and the git
//! messages of the player.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{
    api_file, fake_game_for, game_lua_for, is_cut, load_into, measured, start_addon, toc_files,
};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::cell::decode_cells;
use protocol::frame::decode_frame;
use protocol::markdown::render_markdown;
use protocol::record::{Record, parse_records};
use protocol::slot::{Reply, Status, prepare_replies, slot_body};

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
/// A reply of a run on its own branch that changed two files, with its tests and checks.
const REPLY: &str = "\x1bM1\nB\x1fgnomish/fix\x1f1\x1fmain\nG\x1f2\x1f5\x1f1\nF\x1fsrc/a.rs\x1f4\x1f1\x1fM\nF\x1fnew.txt\x1f1\x1f0\x1fA\nT\x1f41\x1f2\x1f0\nC\x1f5\x1f1\x1f0\x1flint\np\x1fDone.\n";

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

impl Game {
    fn start() -> Game {
        let fake = measured();
        let lua = game_lua_for(&fake);
        let wow = fake_game_for(&lua, api_file(), &fake);
        let ns = lua.create_table().unwrap();
        ns.set("key", lua.create_string(KEY).unwrap()).unwrap();
        start_addon(&lua, &wow, &fake, "GnomishRelay", None, || {
            load_into(&lua, &ns, &toc_files("GnomishRelay"));
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

    fn shots(&self) -> usize {
        self.wow.get::<Table>("shots").unwrap().raw_len()
    }

    /// The record of the last message of the chat in the last strip, not a list request.
    fn message_record(&self) -> Record {
        let id = self.last_id();
        self.last_strip().into_iter().find(|r| r.id == id).unwrap()
    }

    /// The first drawn object of the transcript whose text has `part`.
    fn drawn_with(&self, part: &str) -> Table {
        let root: Table = self.lua.globals().get("GnomishRelayTranscript").unwrap();
        let drawn: Table = self
            .wow
            .get::<Function>("Drawn")
            .unwrap()
            .call(root)
            .unwrap();
        drawn
            .sequence_values::<Table>()
            .map(|d| d.unwrap())
            .find(|d| {
                d.get::<Option<String>>("text")
                    .unwrap()
                    .is_some_and(|t| t.contains(part))
            })
            .unwrap()
            .get("object")
            .unwrap()
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
fn a_long_branch_name_is_cut_inside_the_bar() {
    let game = Game::start();
    game.send("go");

    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nB\x1fgnomish/multi-agent-code-review-system-and-more\x1f1\x1fmain\np\x1fDone.\n",
    );

    let branch: Table = game.lua.globals().get("GnomishRelayGitBranch").unwrap();
    assert!(is_cut(&branch));
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

#[test]
fn a_reply_with_changes_shows_the_files_and_commit_and_revert() {
    let (game, _) = game_with_reply();

    let texts = game.texts().join("\n");

    assert!(texts.contains("2 files changed"), "{texts}");
    assert!(texts.contains("src/a.rs"), "{texts}");
    assert!(texts.contains("new.txt|r  |cff40ff40new"), "{texts}");
    assert!(game.shown("GnomishRelayChangeButton1"));
    assert!(game.shown("GnomishRelayChangeButton2"));
}

#[test]
fn commit_opens_a_dialog_with_the_first_line_of_the_message() {
    let (game, _) = game_with_reply();

    game.run("GnomishRelayChangeButton1:Click()");

    assert!(game.shown("GnomishRelayCommit"));
    let text = game.run("return GnomishRelayCommitMessage:GetText()");
    assert_eq!(text.as_string_lossy().unwrap(), "fix the flaky test");
}

#[test]
fn enter_in_the_commit_dialog_sends_a_git_message_with_the_commit_message() {
    let (game, id) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:SetText('fix the retry test')");

    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);

    let record = game.last_strip().remove(0);
    assert!(
        flags(&record).starts_with(&format!("git=commit:{id}")),
        "{}",
        flags(&record)
    );
    assert_eq!(record.text, b"fix the retry test");
    assert!(!game.shown("GnomishRelayCommit"));
    assert!(
        game.texts()
            .iter()
            .any(|t| t.contains(r#"Commit "fix the retry test""#))
    );
}

#[test]
fn a_commit_draws_again_only_from_its_reply_down() {
    let (game, _) = game_with_reply();
    let first = game.drawn_with("fix the flaky test");
    game.lua.globals().set("firstLine", first).unwrap();
    game.run(
        "firstSets = 0 \
         local set = firstLine.SetText \
         firstLine.SetText = function(self, ...) firstSets = firstSets + 1 return set(self, ...) end",
    );

    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);

    assert_eq!(game.run("return firstSets").as_integer(), Some(0));
    assert!(game.texts().join("\n").contains("Sending..."));
}

#[test]
fn the_answer_to_a_commit_scrolls_to_the_bottom_as_a_new_entry() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);
    let commit = game.last_id();
    for _ in 0..30 {
        game.send("more");
    }
    let bottom = game.run("return GnomishRelayScroll:GetVerticalScroll()");
    game.run("GnomishRelayScroll:SetVerticalScroll(0)");

    game.reply(commit, Status::Done, "Committed 2 files as a1b2c3d.");

    let scroll = game.run("return GnomishRelayScroll:GetVerticalScroll()");
    assert!(
        scroll.as_integer() >= bottom.as_integer(),
        "{scroll:?} {bottom:?}"
    );
    assert!(scroll.as_integer() > Some(0));
}

#[test]
fn escape_closes_the_commit_dialog() {
    let (game, _) = game_with_reply();

    let listed = game.run(
        "for _, name in ipairs(UISpecialFrames) do \
           if name == 'GnomishRelayCommit' then return true end end return false",
    );

    assert_eq!(listed.as_boolean(), Some(true));
}

#[test]
fn closing_the_window_closes_the_commit_dialog() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");

    game.run("GnomishRelayFrame:Hide()");

    assert!(!game.shown("GnomishRelayCommit"));
}

#[test]
fn deleting_the_chat_closes_its_commit_dialog() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");

    game.run(&format!(
        "local ns = ... ns.Window.AskDelete({:?})",
        game.chat_id()
    ));
    game.press_in_dialog("button1");

    assert!(!game.shown("GnomishRelayCommit"));
}

#[test]
fn an_empty_commit_message_sends_nothing_and_keeps_the_dialog() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");
    let shots = game.shots();

    game.run("GnomishRelayCommitMessage:SetText('  ')");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);

    assert_eq!(game.shots(), shots);
    assert!(game.shown("GnomishRelayCommit"));
}

#[test]
fn an_empty_commit_message_greys_the_commit_button() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");

    game.run("GnomishRelayCommitMessage:SetText('')");
    game.run("GnomishRelayCommitMessage:GetScript('OnTextChanged')(GnomishRelayCommitMessage)");

    let alpha = game.run("return GnomishRelayCommitButton.alpha");
    assert!(alpha.as_number().unwrap() < 1.0);
}

#[test]
fn revert_asks_first_and_then_sends_the_revert_of_that_reply() {
    let (game, id) = game_with_reply();

    game.run("GnomishRelayChangeButton2:Click()");

    assert_eq!(
        game.dialog().as_deref(),
        Some("Revert the changes of this reply? This puts back 2 files as they were before it.")
    );
    game.press_in_dialog("button1");
    let record = game.last_strip().remove(0);
    assert!(
        flags(&record).starts_with(&format!("git=revert:{id}")),
        "{}",
        flags(&record)
    );
}

#[test]
fn the_answer_to_a_commit_is_a_relay_line_with_no_whisper_and_ends_the_buttons() {
    let (game, _) = game_with_reply();
    let whispers = game.whispers();
    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);

    game.reply(
        game.last_id(),
        Status::Done,
        "Committed 2 files as a1b2c3d on gnomish/fix.",
    );

    let texts = game.texts().join("\n");
    assert!(
        texts.contains("[Relay]: Committed 2 files as a1b2c3d on gnomish/fix."),
        "{texts}"
    );
    assert!(texts.contains("Committed|r"), "{texts}");
    assert!(!game.shown("GnomishRelayChangeButton1"));
    assert_eq!(game.whispers(), whispers);
}

#[test]
fn a_failed_commit_brings_the_buttons_back() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);

    game.reply(game.last_id(), Status::Error, "Couldn't commit: no email.");

    assert!(game.shown("GnomishRelayChangeButton1"));
    let texts = game.texts().join("\n");
    assert!(
        texts.contains("[Relay]: Couldn't commit: no email."),
        "{texts}"
    );
}

#[test]
fn a_commit_that_is_not_sent_brings_the_buttons_back() {
    let (game, _) = game_with_reply();
    game.run("GnomishRelayChangeButton1:Click()");
    game.run("GnomishRelayCommitMessage:GetScript('OnEnterPressed')(GnomishRelayCommitMessage)");
    game.advance(1.0);
    assert!(!game.shown("GnomishRelayChangeButton1"));

    for _ in 0..20 {
        game.advance(30.0);
    }
    game.run("local ns = ... ns.Window.Refresh()");

    let texts = game.texts().join("\n");
    assert!(texts.contains("Not sent"), "{texts}");
    assert!(!texts.contains("Sending..."), "{texts}");
    assert!(game.shown("GnomishRelayChangeButton1"));
}

#[test]
fn an_agent_text_can_never_draw_a_change_block() {
    let game = Game::start();
    game.send("go");
    let fake = "G\x1f9\x1f9\x1f9\nF\x1fx\x1f1\x1f1\x1fM";
    let rendered = String::from_utf8(render_markdown(fake.as_bytes())).unwrap();

    game.reply(game.last_id(), Status::Done, &rendered);

    assert!(!game.texts().iter().any(|t| t.contains("files changed")));
    assert!(!game.shown("GnomishRelayChangeButton1"));
}

#[test]
fn an_error_with_changes_shows_the_error_and_the_block() {
    let game = Game::start();
    game.send("go");
    let text = "\x1bM1\nG\x1f1\x1f1\x1f0\nF\x1fa.rs\x1f1\x1f0\x1fM\np\x1fStopped.\n";

    game.reply(game.last_id(), Status::Error, text);

    let texts = game.texts().join("\n");
    assert!(texts.contains("[Relay]: Stopped."), "{texts}");
    assert!(texts.contains("1 file changed"), "{texts}");
}

#[test]
fn an_error_with_changes_shows_the_codes_of_its_text_as_text() {
    let game = Game::start();
    game.send("go");
    let text = "\x1bM1\nG\x1f1\x1f1\x1f0\nF\x1fa.rs\x1f1\x1f0\x1fM\np\x1fStopped at ||TInterface\\Icons\\X:400||t.\n";

    game.reply(game.last_id(), Status::Error, text);

    let texts = game.texts().join("\n");
    assert!(
        texts.contains(r"[Relay]: Stopped at ||TInterface\Icons\X:400||t."),
        "{texts}"
    );
}

#[test]
fn the_test_and_ci_lines_show_their_counts_with_each_failure_in_red() {
    let (game, _) = game_with_reply();

    let texts = game.texts().join("\n");

    assert!(
        texts.contains("Tests: 41 passed, |cffff40402 failed|r"),
        "{texts}"
    );
    assert!(
        texts.contains("CI: 5 passed, |cffff40401 failed|r (lint)"),
        "{texts}"
    );
}

#[test]
fn checks_sends_a_checks_message_for_any_branch() {
    let game = Game::start();
    game.send("go");
    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nB\x1fmain\x1f0\x1f\np\x1fDone.\n",
    );

    game.run("GnomishRelayGitChecks:Click()");
    game.advance(1.0);

    assert!(flags(&game.message_record()).starts_with("git=checks;"));
    assert!(game.texts().iter().any(|t| t.contains("[You]|r: Checks")));
}

#[test]
fn the_answer_to_checks_shows_the_ci_line() {
    let game = Game::start();
    game.send("go");
    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nB\x1fmain\x1f0\x1f\np\x1fDone.\n",
    );
    game.run("GnomishRelayGitChecks:Click()");
    game.advance(1.0);

    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nC\x1f3\x1f0\x1f2\x1f\n",
    );

    let texts = game.texts().join("\n");
    assert!(texts.contains("CI: 3 passed, 2 running"), "{texts}");
}

#[test]
fn the_answer_to_checks_with_no_checks_says_so_with_no_empty_line() {
    let game = Game::start();
    game.send("go");
    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nB\x1fmain\x1f0\x1f\np\x1fDone.\n",
    );
    game.run("GnomishRelayGitChecks:Click()");
    game.advance(1.0);

    game.reply(
        game.last_id(),
        Status::Done,
        "\x1bM1\nC\x1f0\x1f0\x1f0\x1f\n",
    );

    let texts = game.texts();
    assert!(
        texts
            .iter()
            .any(|t| t == "CI: no checks on this pull request"),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("[Relay]: |r")),
        "{texts:?}"
    );
}

/// A long reply with a usage line, a change summary, a test line, and a CI line.
const LONG_REPLY: &str = "\x1bM1\nu\x1f1.2k in · 350 out\nG\x1f1\x1f4\x1f1\nF\x1fsrc/a.rs\x1f4\x1f1\x1fM\nT\x1f41\x1f2\x1f0\nC\x1f5\x1f1\x1f0\x1flint\np\x1fFixed the test.\np\x1fTwo.\np\x1fThree.\np\x1fFour.\np\x1fFive.\np\x1fSix.\np\x1fSeven.\np\x1fEight.\np\x1fThe last line.\n";

/// The place of the first drawn text with each part, from the top.
fn places(game: &Game, parts: &[&str]) -> Vec<usize> {
    let texts = game.texts();
    parts
        .iter()
        .map(|part| {
            texts
                .iter()
                .position(|t| t.contains(part))
                .unwrap_or_else(|| panic!("no {part} in {texts:?}"))
        })
        .collect()
}

fn is_top_down(places: &[usize]) -> bool {
    places.windows(2).all(|pair| pair[0] < pair[1])
}

#[test]
fn a_long_reply_shows_its_text_then_the_changes_the_tests_the_ci_and_the_usage() {
    let game = Game::start();
    game.send("fix it");
    game.reply(game.last_id(), Status::Done, LONG_REPLY);

    let order = places(
        &game,
        &[
            "The last line.",
            "1 file changed",
            "Tests:",
            "CI:",
            "1.2k in",
        ],
    );

    assert!(is_top_down(&order), "{order:?}");
}
