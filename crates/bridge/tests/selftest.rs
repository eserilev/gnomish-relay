//! The self-test addon in the fake game, and `selftest collect` on what it leaves
//! behind (SPEC.md 14.3). The real run happens in the game; this checks the path.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use bridge::calibration::Verdict;
use bridge::fixture::{self, Capture};
use bridge::line_choice::{self, LineChoice, Reason};
use bridge::selftest::{self, Parts, SAVED_FILE};
use bridge::strip::Image;
use bridge::vectors::{self, Shot};
use common::{
    HEIGHT, WIDTH, encode_png, fake_game_for, game_lua_for, load_addon, measured,
    measured_at_test_screen, screenshot_png, signed_frame, start_addon, strip_rows, toc_files,
};
use mlua::{Function, Lua, Table};
use png::{BitDepth, ColorType};

const ADDON: &str = "GnomishRelaySelfTest";
const STRIP: &str = "GnomishRelaySelfTestStrip";
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
        let fake = measured_at_test_screen();
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
            load_addon(&lua, ADDON, &ns, &toc_files(ADDON));
        });
        Game { lua, wow }
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

    /// Each screenshot of the self-test strip as a PNG of 1280x720, oldest first.
    fn pictures(&self) -> Vec<Vec<u8>> {
        self.pictures_with(<[u8]>::to_vec)
    }

    /// The PNGs after `change` of the pixels, as a scaler of the game could do.
    fn pictures_with(&self, change: impl Fn(&[u8]) -> Vec<u8>) -> Vec<Vec<u8>> {
        let pictures: Table = self
            .wow
            .get::<Table>("picturesOf")
            .unwrap()
            .get(STRIP)
            .unwrap();
        pictures
            .sequence_values::<Table>()
            .map(|picture| {
                let rgb = change(&rgb_of(&picture.unwrap()));
                encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb)
            })
            .collect()
    }

    fn printed(&self) -> Vec<String> {
        self.wow.get("printed").unwrap()
    }
}

/// The rectangles of one picture of the fake game on a grey scene.
fn rgb_of(picture: &Table) -> Vec<u8> {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut rgb = vec![70u8; width * height * 3];
    for rect in picture.sequence_values::<Table>().map(Result::unwrap) {
        let at = |key: &str| rect.get::<usize>(key).unwrap();
        let color: Vec<u8> = rect.get("color").unwrap();
        for y in (at("y")..at("y") + at("height")).filter(|&y| y < height) {
            for x in (at("x")..at("x") + at("width")).filter(|&x| x < width) {
                let i = (y * width + x) * 3;
                rgb[i..i + 3].copy_from_slice(&color);
            }
        }
    }
    rgb
}

/// Each pixel takes a quarter from its left and its right neighbor.
fn blur(rgb: &[u8]) -> Vec<u8> {
    let row = WIDTH as usize * 3;
    let mut out = rgb.to_vec();
    for (i, value) in out.iter_mut().enumerate() {
        let x = i % row;
        let left = if x >= 3 { rgb[i - 3] } else { rgb[i] };
        let right = if x + 3 < row { rgb[i + 3] } else { rgb[i] };
        let sum = u16::from(left) + 2 * u16::from(rgb[i]) + u16::from(right);
        *value = u8::try_from(sum / 4).unwrap();
    }
    out
}

/// The strips that show in a picture: every shot but the probe, which hides its strip
/// in the hook of `Screenshot()`. A client that captures at the call still shows it.
fn strips_in_pictures() -> usize {
    match measured_at_test_screen().capture {
        Capture::Call => 14,
        Capture::AfterHandler => 13,
    }
}

/// The build of the fake game, as the newest fixture measured it.
fn fake_build() -> String {
    let info = measured().build_info;
    format!("{}.{}", info.version, info.build)
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
    let pictures = game.pictures();
    assert_eq!(
        shots.len(),
        14,
        "six sizes, the records, six lines, and the probe"
    );
    assert_eq!(pictures.len(), strips_in_pictures());
    for (shot, png_bytes) in shots.iter().zip(&pictures) {
        let image = Image::from_png(png_bytes).unwrap();
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
    let probe_in_picture = strips_in_pictures() == 14;

    let fixture = fixture::build(&parts.results, &parts.load, None, probe_in_picture).unwrap();

    assert_eq!(fixture.build, fake_build());
    // The self-test in the fake game measures the fake game itself.
    let expected = measured_at_test_screen();
    let mut got = fixture.fake.clone();
    assert!((got.shot_delay - expected.shot_delay).abs() < 0.01);
    got.shot_delay = expected.shot_delay;
    assert_eq!(got, expected);
    assert_eq!(
        fixture.measured["shots"][0].get("payload"),
        None,
        "the payloads go into the manifest only"
    );
}

/// Forever 1.60.1.70205 blocks the register with a popup that pcall does not see.
#[test]
fn with_a_restricted_combat_log_the_run_registers_no_combat_log_event() {
    let game = Game::start(None, 1_790_300_000);
    game.lua
        .load("C_CombatLog.IsCombatLogRestricted = function() return true end")
        .exec()
        .unwrap();

    game.advance(RUN);

    let parts = parts(&game.saved_variables());
    let event = &parts.results["secrets"]["t8_combat_log_event"];
    assert_eq!(event["status"], "needs an open combat log");
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
    game_folder_with(root, saved, game.pictures());
}

fn game_folder_with(root: &Path, saved: &[u8], pictures: Vec<Vec<u8>>) {
    let account = root.join("WTF").join("Account").join("ACCOUNT1");
    let saved_dir = account.join("SavedVariables");
    fs::create_dir_all(&saved_dir).unwrap();
    fs::write(saved_dir.join(SAVED_FILE), saved).unwrap();
    let screenshots = root.join("Screenshots");
    fs::create_dir_all(&screenshots).unwrap();
    let shots = shots_of(saved);
    let mut pictures: Vec<(String, Vec<u8>, u32)> = shots
        .iter()
        .zip(pictures)
        .map(|(shot, png_bytes)| {
            let name = format!("WoWScrnShot_{}.png", shot.frame_id);
            (name, png_bytes, shot.unix.unwrap())
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
}

#[test]
fn collect_writes_the_fixture_and_a_golden_vector_for_each_strip() {
    let (first, saved) = two_sessions();
    let game_dir = tempfile::tempdir().unwrap();
    game_folder(game_dir.path(), &first, &saved);
    let repo = tempfile::tempdir().unwrap();
    repo_folder(repo.path());

    let collected = selftest::collect(game_dir.path(), repo.path()).unwrap();

    assert_eq!(collected.build, fake_build());
    assert_eq!(collected.vectors, strips_in_pictures());
    assert!(collected.missing.is_empty(), "{:?}", collected.missing);
    let fixtures = repo.path().join("tests").join("fixtures");
    let written = fixture::read(&fixture::newest(&fixtures).unwrap()).unwrap();
    assert_eq!(written.build, fake_build());
    let vectors = repo.path().join("tests").join("vectors").join(fake_build());
    assert_eq!(vectors::check_all(&vectors).unwrap(), strips_in_pictures());
    assert_eq!(fs::read(vectors.join(SAVED_FILE)).unwrap(), saved);
    let manifest = vectors::read_manifest(&vectors).unwrap();
    assert!(
        manifest
            .vectors
            .iter()
            .all(|v| v.width < WIDTH as usize && v.height < HEIGHT as usize),
        "each vector keeps only the corner of its strip"
    );
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
    assert_eq!(collected.vectors, strips_in_pictures() - 1);
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

#[test]
fn collect_finds_every_line_mode_clean_and_chooses_mode_1_for_the_screen_of_the_run() {
    let (first, saved) = two_sessions();
    let game_dir = tempfile::tempdir().unwrap();
    game_folder(game_dir.path(), &first, &saved);
    let repo = tempfile::tempdir().unwrap();
    repo_folder(repo.path());
    let data = tempfile::tempdir().unwrap();

    let collected = selftest::collect(game_dir.path(), repo.path()).unwrap();
    collected.save_line(data.path()).unwrap();

    assert_eq!(collected.lines.len(), 6);
    assert!(
        collected
            .lines
            .iter()
            .all(|(_, v)| matches!(v, Verdict::Clean { .. }))
    );
    let choice = LineChoice {
        mode: 1,
        width: WIDTH,
        height: HEIGHT,
        reason: None,
    };
    assert_eq!(line_choice::load(data.path()).unwrap(), [choice]);
    let report = collected.line_report().join("\n");
    assert!(
        report.starts_with("Strip line modes, for a screen of 1280x720:"),
        "{report}"
    );
    assert!(report.contains("mode 6 (2 px, 6 bits): clean"), "{report}");
    assert!(
        report.contains(
            "Chosen: mode 1 (1 px, 24 bits). The strip is now a line 1 px tall and 200 px wide"
        ),
        "{report}"
    );
}

#[test]
fn collect_of_blurred_screenshots_keeps_the_old_strip_and_says_why() {
    let (first, saved) = two_sessions();
    let game_dir = tempfile::tempdir().unwrap();
    game_folder_with(game_dir.path(), &saved, first.pictures_with(blur));
    let repo = tempfile::tempdir().unwrap();
    repo_folder(repo.path());
    let data = tempfile::tempdir().unwrap();
    line_choice::remember(
        data.path(),
        LineChoice {
            mode: 1,
            width: WIDTH,
            height: HEIGHT,
            reason: None,
        },
    )
    .unwrap();

    let collected = selftest::collect(game_dir.path(), repo.path()).unwrap();
    collected.save_line(data.path()).unwrap();

    let none = LineChoice {
        mode: 0,
        width: WIDTH,
        height: HEIGHT,
        reason: Some(Reason::Blur),
    };
    assert_eq!(collected.line, Some(none));
    assert_eq!(line_choice::load(data.path()).unwrap(), [none]);
    let report = collected.line_report().join("\n");
    assert!(
        report.contains("mode 1 (1 px, 24 bits): not found"),
        "{report}"
    );
    assert!(report.contains("mode 4 (2 px, 24 bits): blur"), "{report}");
    assert!(report.contains("No mode reads exactly"), "{report}");
}
