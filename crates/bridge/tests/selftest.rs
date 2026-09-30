//! The self-test addon in the fake game, and `selftest collect` on what it leaves
//! behind (SPEC.md 14.3). The real run happens in the game; this checks the path.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use bridge::fixture::{self, Fake, PLACEHOLDER};
use bridge::selftest::{self, Parts, SAVED_FILE};
use bridge::strip::Image;
use bridge::vectors::{self, Shot};
use common::{
    fake_game_for, fire, game_lua_for, load_addon, measured, screenshot_png, signed_frame,
    start_addon, strip_rows,
};
use mlua::{Function, Lua, Table};

const ADDON: &str = "GnomishRelaySelfTest";
const STRIP: &str = "GnomishRelaySelfTestStrip";
const FILES: &[&str] = &[
    "Load.lua",
    "App.lua",
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Health.lua",
    "Strip.lua",
    "Json.lua",
    "Client.lua",
    "Fonts.lua",
    "AddOns.lua",
    "Secrets.lua",
    "Timing.lua",
    "Shots.lua",
    "SelfTest.lua",
];
/// The functions that only the self-test calls. The fake game of the relay has none of them.
const SELFTEST_GLOBALS: &str = r#"
local wow = ...
GetTimePreciseSec = GetTime
GetServerTime = time
function GetFramerate() return 60 end
function GetScreenWidth() return 1024 end
function GetScreenHeight() return 576 end
function date() return "092626_120000" end
function UnitName() return "Tester" end
function UnitExists() return false end
function UnitHealth() return 100 end
function UnitPower() return 50 end
function UnitGroupRolesAssigned() return "NONE" end
function UnitDetailedThreatSituation() return nil end
function IsInGroup() return false end
function issecretvalue() return false end
C_CombatLog = { IsCombatLogRestricted = function() return false end }
C_CVar = {
    GetCVar = function(name) return wow.cvars[name] end,
    SetCVar = function(name, value) wow.cvars[name] = value return true end,
}
"#;
/// `PLAYER_ENTERING_WORLD` starts the run, five seconds later. The run takes about 30 seconds.
const RUN: f64 = 90.0;

struct Game {
    lua: Lua,
    wow: Table,
}

impl Game {
    fn start(saved: Option<&[u8]>, epoch: i64) -> Game {
        let fake = measured();
        let lua = game_lua_for(&fake);
        let wow = fake_game_for(&lua, "addon/tests/selftest-api.lua", &fake);
        wow.set("epoch", epoch).unwrap();
        wow.set("strips", lua.create_sequence_from([STRIP]).unwrap())
            .unwrap();
        lua.load(SELFTEST_GLOBALS).call::<()>(wow.clone()).unwrap();
        let missing: Table = wow.get("missingAddOns").unwrap();
        missing.set("GnomishRelaySelfTest_Missing", true).unwrap();
        let old: Table = wow.get("outOfDate").unwrap();
        old.set("GnomishRelaySelfTest_Old", true).unwrap();
        let no_font: Table = wow.get("missingFiles").unwrap();
        no_font
            .set(
                "Interface\\AddOns\\GnomishRelaySelfTest\\NoSuchFont.ttf",
                true,
            )
            .unwrap();
        let ns = lua.create_table().unwrap();
        start_addon(&lua, &wow, &fake, ADDON, saved, || {
            load_addon(&lua, ADDON, &ns, FILES);
        });
        let game = Game { lua, wow };
        game.enter_world(&fake, saved.is_some());
        game
    }

    /// The placeholder has no `PLAYER_ENTERING_WORLD`. The self-test needs it.
    fn enter_world(&self, fake: &Fake, reload: bool) {
        if !fake
            .login_events
            .iter()
            .any(|e| e == "PLAYER_ENTERING_WORLD")
        {
            fire(
                &self.lua,
                &self.wow,
                "PLAYER_ENTERING_WORLD",
                (!reload, reload),
            );
        }
    }

    fn advance(&self, seconds: f64) {
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(seconds).unwrap();
    }

    /// The bytes of the file. A Lua string can hold any byte, and the format probe does.
    fn saved_variables(&self) -> Vec<u8> {
        let save: Function = self.wow.get("Save").unwrap();
        let text: mlua::String = save.call("GnomishRelaySelfTestDB").unwrap();
        text.as_bytes().to_vec()
    }

    fn epoch(&self) -> i64 {
        self.lua.load("return time()").eval().unwrap()
    }

    /// The cells of each screenshot of the self-test strip, oldest first.
    fn shots(&self) -> Vec<Vec<Vec<u8>>> {
        let shots: Table = self
            .wow
            .get::<Table>("shotsOf")
            .unwrap()
            .get(STRIP)
            .unwrap();
        shots
            .sequence_values::<Vec<Vec<u8>>>()
            .map(Result::unwrap)
            .collect()
    }

    fn printed(&self) -> Vec<String> {
        self.wow.get("printed").unwrap()
    }
}

/// A first login, the run, and a /reload: the saved file then knows the load order.
fn two_sessions() -> (Game, Vec<u8>) {
    let first = Game::start(None, 1_790_300_000);
    first.advance(RUN);
    let saved = first.saved_variables();
    let second = Game::start(Some(&saved), first.epoch());
    second.advance(1.0);
    let saved = second.saved_variables();
    (first, saved)
}

fn parts(saved: &[u8]) -> Parts {
    Parts::read(&String::from_utf8_lossy(saved)).unwrap()
}

fn shots_of(saved: &[u8]) -> Vec<Shot> {
    parts(saved).shots().unwrap()
}

#[test]
fn the_run_draws_every_golden_strip_with_the_public_test_key() {
    let game = Game::start(None, 1_790_300_000);
    game.advance(RUN);

    let shots = shots_of(&game.saved_variables());
    let pictures = game.shots();
    assert_eq!(shots.len(), 8, "six sizes, the records, and the probe");
    assert_eq!(pictures.len(), shots.len());
    for (shot, rows) in shots.iter().zip(&pictures) {
        let image = Image::from_png(&screenshot_png(rows)).unwrap();
        let frame = vectors::test_frame(&image).unwrap().expect("a test strip");
        assert_eq!(
            vectors::shot_of(&frame, &shots),
            Some(shot),
            "{}",
            shot.name
        );
    }
    assert!(
        game.printed()
            .iter()
            .any(|l| l.contains("done. Type /reload"))
    );
}

#[test]
fn the_first_session_does_not_know_the_load_order_and_asks_for_one_more_reload() {
    let first = Game::start(None, 1_790_300_000);
    first.advance(RUN);
    let parts = parts(&first.saved_variables());

    let built = fixture::build(&parts.results, &parts.load, None, true);

    let error = format!("{:#}", built.unwrap_err());
    assert!(error.contains("/reload"), "{error}");
    let second = Game::start(Some(&first.saved_variables()), first.epoch());
    assert!(
        second
            .printed()
            .iter()
            .any(|l| l.contains("/reload once more"))
    );
}

#[test]
fn after_a_reload_the_results_make_a_fixture_that_the_fake_game_can_read() {
    let (_, saved) = two_sessions();
    let parts = parts(&saved);

    let fixture = fixture::build(&parts.results, &parts.load, None, true).unwrap();

    assert!(!fixture.placeholder);
    assert_eq!(fixture.build, "1.60.1.70009");
    // The self-test in the fake game measures the fake game itself. The test fires
    // PLAYER_ENTERING_WORLD after the login events of the fixture.
    let expected = measured();
    let mut got = fixture.fake.clone();
    assert!((got.shot_delay - expected.shot_delay).abs() < 0.01);
    assert_eq!(
        got.login_events.pop().as_deref(),
        Some("PLAYER_ENTERING_WORLD")
    );
    got.shot_delay = expected.shot_delay;
    assert_eq!(got, expected);
    assert_eq!(
        fixture.measured["shots"][0].get("payload"),
        None,
        "the payloads go into the manifest only"
    );
}

#[test]
fn the_run_leaves_the_screenshot_format_as_it_found_it() {
    let game = Game::start(None, 1_790_300_000);
    let cvars: Table = game.wow.get("cvars").unwrap();
    cvars.set("screenshotFormat", "jpeg").unwrap();

    game.advance(RUN);

    assert_eq!(cvars.get::<String>("screenshotFormat").unwrap(), "jpeg");
    let parts = parts(&game.saved_variables());
    assert_eq!(parts.results["set_screenshot_format"]["read_back"], "png");
}

/// A game folder as WoW leaves it after the run: the saved file, and one screenshot per
/// strip with the time of its shot. Also a screenshot of the player, and a relay strip.
fn game_folder(root: &Path, game: &Game, saved: &[u8]) {
    let account = root.join("WTF").join("Account").join("ACCOUNT1");
    let saved_dir = account.join("SavedVariables");
    fs::create_dir_all(&saved_dir).unwrap();
    fs::write(saved_dir.join(SAVED_FILE), saved).unwrap();
    let screenshots = root.join("Screenshots");
    fs::create_dir_all(&screenshots).unwrap();
    let shots = shots_of(saved);
    let mut pictures: Vec<(String, Vec<u8>, u32)> = shots
        .iter()
        .zip(game.shots())
        .map(|(shot, rows)| {
            let name = format!("WoWScrnShot_{}.png", shot.frame_id);
            (name, screenshot_png(&rows), shot.unix.unwrap())
        })
        .collect();
    let at = shots[0].unix.unwrap();
    pictures.push(("WoWScrnShot_player.png".into(), screenshot_png(&[]), at));
    let relay = signed_frame(at, b"a relay message", &[7; 32]);
    let relay_png = screenshot_png(&strip_rows(&relay));
    pictures.push(("WoWScrnShot_relay.png".into(), relay_png, at));
    for (name, png, unix) in pictures {
        let path = screenshots.join(name);
        fs::write(&path, png).unwrap();
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(unix)))
            .unwrap();
    }
}

fn repo_folder(root: &Path) {
    let fixtures = root.join("tests").join("fixtures");
    fs::create_dir_all(&fixtures).unwrap();
    fs::write(fixtures.join(PLACEHOLDER), "{}").unwrap();
}

#[test]
fn collect_writes_the_fixture_and_a_golden_vector_for_each_strip() {
    let (first, saved) = two_sessions();
    let game_dir = tempfile::tempdir().unwrap();
    game_folder(game_dir.path(), &first, &saved);
    let repo = tempfile::tempdir().unwrap();
    repo_folder(repo.path());

    let collected = selftest::collect(game_dir.path(), repo.path()).unwrap();

    assert_eq!(collected.build, "1.60.1.70009");
    assert_eq!(collected.vectors, 8);
    assert!(collected.missing.is_empty(), "{:?}", collected.missing);
    let fixtures = repo.path().join("tests").join("fixtures");
    assert!(!fixtures.join(PLACEHOLDER).exists());
    let written = fixture::read(&fixture::newest(&fixtures).unwrap()).unwrap();
    assert_eq!(written.build, "1.60.1.70009");
    let vectors = repo
        .path()
        .join("tests")
        .join("vectors")
        .join("1.60.1.70009");
    assert_eq!(vectors::check_all(&vectors).unwrap(), 8);
    assert_eq!(fs::read(vectors.join(SAVED_FILE)).unwrap(), saved);
}

#[test]
fn collect_never_takes_a_screenshot_from_outside_the_time_of_the_run() {
    let (first, saved) = two_sessions();
    let game_dir = tempfile::tempdir().unwrap();
    game_folder(game_dir.path(), &first, &saved);
    let old = game_dir
        .path()
        .join("Screenshots")
        .join("WoWScrnShot_1.png");
    let file = fs::File::options().write(true).open(&old).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
    let repo = tempfile::tempdir().unwrap();
    repo_folder(repo.path());

    let collected = selftest::collect(game_dir.path(), repo.path()).unwrap();

    assert_eq!(collected.missing, ["len-0000"]);
    assert_eq!(collected.vectors, 7);
}

#[test]
fn collect_with_no_self_test_run_says_what_to_do() {
    let game_dir = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();

    let error = selftest::collect(game_dir.path(), repo.path())
        .err()
        .unwrap();

    assert!(format!("{error:#}").contains("selftest-link.sh"));
}

#[test]
fn results_signed_with_another_key_are_refused() {
    let (_, saved) = two_sessions();
    let saved = String::from_utf8_lossy(&saved).into_owned();
    let results = bridge::saved::hex_fields(&saved, "results").remove(0);
    let results = String::from_utf8(results).unwrap();
    let public = bridge::ids::hex(vectors::TEST_KEY);
    let secret = bridge::ids::hex(b"a secret key, not the public one!");
    let signed_by_secret = results.replace(&public, &secret);
    let saved = saved.replace(
        &bridge::ids::hex(results.as_bytes()),
        &bridge::ids::hex(signed_by_secret.as_bytes()),
    );

    let error = Parts::read(&saved).err().unwrap();

    assert!(format!("{error:#}").contains("another key"));
}

/// `%d` in Lua 5.1 goes through a C `long`, which has 32 bits on Windows, the OS of WoW.
#[test]
fn the_json_writer_keeps_integers_above_32_bits() {
    let lua = common::lua(common::Bits::Unsigned);
    let ns = lua.create_table().unwrap();
    load_addon(&lua, ADDON, &ns, &["Json.lua"]);
    let encode: Function = ns.get::<Table>("Json").unwrap().get("Encode").unwrap();

    let text: String = encode
        .call(
            lua.create_sequence_from([2_147_483_648.0, 9_007_199_254_740_991.0, -12.0])
                .unwrap(),
        )
        .unwrap();

    assert_eq!(text, "[2147483648,9007199254740991,-12]");
}
