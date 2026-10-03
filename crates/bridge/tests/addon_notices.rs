//! The notifications of terminal sessions in a fake game (SPEC.md 10.4): the chat line,
//! the sound, the toast, the bell, the list, the faster polls, and the settings.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{api_file, bytes, fake_game_for, game_lua_for, load_into, measured, start_addon};
use mlua::{Function, Lua, Table, Value};
use protocol::apps::App;
use protocol::live::{Notice, NoticeKind, Notices, Source, live_body, prepare_notices};
use protocol::notice::notice_text;
use protocol::slot::slot_body;

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
    "Atlases.lua",
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
    "ChatMenu.lua",
    "MiniChat.lua",
    "Popup.lua",
    "NoticeFrames.lua",
    "SetupNeeded.lua",
    "Core.lua",
];
const TOAST_SOUND: i64 = 18019;
const WHISPER_SOUND: i64 = 3081;

struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

impl Game {
    fn start() -> Game {
        Game::boot(None)
    }

    /// A client that lacks `atlases`, as TBC Anniversary lacks some of Forever.
    fn start_without_atlases(atlases: &[&str]) -> Game {
        Game::boot_with(None, |wow| {
            let missing: Table = wow.get("missingAtlases").unwrap();
            for atlas in atlases {
                missing.set(*atlas, true).unwrap();
            }
        })
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
        Game::boot_with(saved, |_| {})
    }

    /// `before` changes the fake game before the addon files load.
    fn boot_with(saved: Option<&str>, before: impl FnOnce(&Table)) -> Game {
        let fake = measured();
        let lua = game_lua_for(&fake);
        let wow = fake_game_for(&lua, api_file(), &fake);
        before(&wow);
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
        Game { lua, wow, ns }
    }

    fn run(&self, code: &str) -> Value {
        self.lua.load(code).call(self.ns.clone()).unwrap()
    }

    fn advance(&self, seconds: f64) {
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(seconds).unwrap();
    }

    fn now(&self) -> u32 {
        u32::try_from(self.run("return time()").as_integer().unwrap()).unwrap()
    }

    /// Puts a body and a live file with `notices` into every slot, as the bridge does.
    fn publish(&self, busy: u32, open: u32, list: &[Notice]) {
        let body = slot_body(App::Relay, self.now(), &[]);
        let notices = Notices {
            busy,
            open,
            list: prepare_notices(list),
        };
        let live = live_body(App::Relay, &[], &[], &notices);
        self.wow
            .set("body", self.lua.create_string(body).unwrap())
            .unwrap();
        self.wow
            .set("live", self.lua.create_string(live).unwrap())
            .unwrap();
    }

    fn poll(&self) {
        self.run("local ns = ... ns.Transport.Poll()");
    }

    fn publish_and_poll(&self, busy: u32, open: u32, list: &[Notice]) {
        self.publish(busy, open, list);
        self.poll();
    }

    fn printed(&self) -> Vec<String> {
        self.wow.get::<Vec<String>>("printed").unwrap()
    }

    fn lines_with(&self, part: &str) -> Vec<String> {
        self.printed()
            .into_iter()
            .filter(|l| l.contains(part))
            .collect()
    }

    fn sounds(&self) -> Vec<i64> {
        self.wow.get::<Vec<i64>>("sounds").unwrap()
    }

    fn shown(&self, name: &str) -> bool {
        self.run(&format!("return {name}:IsShown()"))
            .as_boolean()
            .unwrap()
    }

    fn set(&self, code: &str) {
        self.run(&format!("local ns = ... {code}"));
    }

    fn click(&self, name: &str) {
        self.run(&format!("{name}:Click()"));
    }

    fn list_len(&self) -> i64 {
        self.run("local ns = ... return #ns.Notices.List()")
            .as_integer()
            .unwrap()
    }

    fn loaded_slots(&self) -> usize {
        self.wow
            .get::<Table>("loaded")
            .unwrap()
            .pairs::<String, bool>()
            .count()
    }

    /// The seconds between the polls of the game, from a clock that ticks each second.
    fn poll_gaps(&self, seconds: usize) -> Vec<usize> {
        self.poll_gaps_with(seconds, |_| {})
    }

    /// As `poll_gaps`, with a bridge that writes a fresh body each 60 s, as the real one does.
    fn poll_gaps_while_the_bridge_runs(&self, seconds: usize, busy: u32, open: u32) -> Vec<usize> {
        self.poll_gaps_with(seconds, |second| {
            if second % 60 == 0 {
                self.publish(busy, open, &[]);
            }
        })
    }

    fn poll_gaps_with(&self, seconds: usize, each_second: impl Fn(usize)) -> Vec<usize> {
        let mut at = Vec::new();
        let mut last = self.loaded_slots();
        for second in 1..=seconds {
            each_second(second);
            self.advance(1.0);
            let now = self.loaded_slots();
            if now != last {
                at.push(second);
                last = now;
            }
        }
        at.windows(2).map(|w| w[1] - w[0]).collect()
    }
}

fn notice(id: u32, kind: NoticeKind, repo: &str, took: u32, text: &str) -> Notice {
    Notice {
        id,
        at: 1_790_211_000,
        source: Source::Claude,
        kind,
        repo: repo.as_bytes().to_vec(),
        took,
        text: text.as_bytes().to_vec(),
    }
}

fn waiting(id: u32) -> Notice {
    notice(
        id,
        NoticeKind::Waiting,
        "gnomish-relay",
        0,
        "Claude needs your permission to use Bash",
    )
}

fn finished(id: u32, repo: &str, took: u32) -> Notice {
    notice(id, NoticeKind::Finished, repo, took, "All tests pass.")
}

#[test]
fn a_waiting_notice_shows_the_line_the_toast_sound_the_toast_and_a_glowing_bell() {
    let game = Game::start();

    game.publish_and_poll(1, 1, &[waiting(1)]);

    let lines = game.lines_with("Waiting for you");
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains(
            "|Hgnomishrelaynotices|h[Claude · gnomish-relay] Waiting for you: Claude needs your permission to use Bash|h"
        ),
        "{lines:?}"
    );
    assert!(lines[0].contains("|A:minimap-genericevent-hornicon-small"));
    assert!(!lines[0].contains("whispers"), "never a whisper");
    assert_eq!(game.sounds(), [TOAST_SOUND]);
    assert!(game.shown("GnomishRelayToast"));
    assert!(game.shown("GnomishRelayBell"));
    assert!(
        game.run("return GnomishRelayBell.glow:IsShown()")
            .as_boolean()
            .unwrap()
    );
}

#[test]
fn the_bell_shows_the_horn_of_a_minimap_event() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);

    let atlas = game.run("return GnomishRelayBell.icon.atlas");

    assert_eq!(atlas.to_string().unwrap(), "minimap-genericevent-hornicon");
}

#[test]
fn without_the_horn_atlas_the_bell_and_the_line_show_the_notification_icon() {
    let game = Game::start_without_atlases(&[
        "minimap-genericevent-hornicon",
        "minimap-genericevent-hornicon-small",
    ]);

    game.publish_and_poll(1, 1, &[waiting(1)]);

    let lines = game.lines_with("Waiting for you");
    assert!(
        lines[0].contains("|A:communities-icon-notification:14:14|a"),
        "{lines:?}"
    );
    let atlas = game.run("return GnomishRelayBell.icon.atlas");
    assert_eq!(atlas.to_string().unwrap(), "communities-icon-notification");
}

#[test]
fn the_toast_names_the_agent_and_the_repo_and_goes_after_8_seconds() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);

    let title = game.run("return GnomishRelayToast.title:GetText()");
    assert_eq!(
        title.as_string().unwrap().to_str().unwrap(),
        "Claude is waiting · gnomish-relay"
    );
    game.advance(7.0);
    assert!(game.shown("GnomishRelayToast"));
    game.advance(2.0);
    assert!(!game.shown("GnomishRelayToast"));
}

#[test]
fn a_finished_notice_gets_the_whisper_sound_and_no_toast() {
    let game = Game::start();

    game.publish_and_poll(0, 1, &[finished(1, "lighthouse", 240)]);

    assert_eq!(
        game.lines_with("[Claude · lighthouse] Finished in 4 min: All tests pass.")
            .len(),
        1
    );
    assert_eq!(game.sounds(), [WHISPER_SOUND]);
    assert!(!game.shown("GnomishRelayToast"));
    assert!(game.shown("GnomishRelayBell"));
    assert!(
        !game
            .run("return GnomishRelayBell.glow:IsShown()")
            .as_boolean()
            .unwrap()
    );
}

#[test]
fn a_failed_notice_says_how_long_the_turn_ran() {
    let game = Game::start();
    let failed = notice(1, NoticeKind::Failed, "timeways", 120, "overloaded");
    game.publish_and_poll(0, 1, &[failed]);
    assert_eq!(
        game.lines_with("[Claude · timeways] Failed after 2 min: overloaded")
            .len(),
        1
    );
}

#[test]
fn nothing_shows_twice_also_across_a_reload() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.publish_and_poll(1, 1, &[waiting(1)]);
    assert_eq!(game.lines_with("Waiting for you").len(), 1);

    let game = game.reload();
    game.publish_and_poll(1, 1, &[waiting(1)]);

    assert!(game.lines_with("Waiting for you").is_empty());
    assert!(game.sounds().is_empty());
    assert!(game.shown("GnomishRelayBell"), "the list still holds it");
}

#[test]
fn an_answered_notice_leaves_the_list_at_the_next_poll() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);

    game.publish_and_poll(1, 1, &[]);

    assert_eq!(game.list_len(), 0);
    assert!(!game.shown("GnomishRelayBell"));
}

#[test]
fn short_finished_work_shows_nowhere() {
    let game = Game::start();

    game.publish_and_poll(0, 1, &[finished(1, "quick", 30)]);

    assert!(game.lines_with("quick").is_empty());
    assert_eq!(game.list_len(), 0);
    assert!(game.sounds().is_empty());
    assert!(!game.shown("GnomishRelayBell"));
}

#[test]
fn finished_work_of_unknown_length_passes_every_setting_but_never() {
    let game = Game::start();
    game.set("ns.Store.db.notifyFinished = 'over3'");
    game.publish_and_poll(0, 1, &[finished(1, "restarted", 0)]);
    assert_eq!(game.lines_with("[Claude · restarted] Finished: ").len(), 1);

    game.set("ns.Store.db.notifyFinished = 'never'");
    game.publish_and_poll(0, 1, &[finished(2, "hidden", 0), finished(3, "long", 9000)]);
    assert!(game.lines_with("hidden").is_empty());
    assert!(game.lines_with("long").is_empty());

    game.set("ns.Store.db.notifyFinished = 'always'");
    game.publish_and_poll(0, 1, &[finished(4, "short", 5)]);
    assert_eq!(game.lines_with("Finished in 5 s").len(), 1);
}

#[test]
fn a_lower_finished_setting_never_alerts_notices_that_the_old_setting_hid() {
    let game = Game::start();
    game.set("ns.Store.db.notifyFinished = 'over3'");
    game.publish_and_poll(0, 1, &[finished(1, "quick", 90)]);

    game.set("ns.Store.db.notifyFinished = 'always'");
    game.publish_and_poll(0, 1, &[finished(1, "quick", 90)]);

    assert!(game.lines_with("quick").is_empty());
    assert!(game.sounds().is_empty());
    assert_eq!(game.list_len(), 1, "the list shows it now");
}

#[test]
fn three_finished_notices_of_one_poll_share_one_line() {
    let game = Game::start();

    game.publish_and_poll(
        0,
        3,
        &[
            finished(1, "gnomish-relay", 100),
            finished(2, "lighthouse", 100),
            finished(3, "timeways", 100),
        ],
    );

    let lines = game.lines_with("agents finished");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("3 agents finished: gnomish-relay, lighthouse, timeways"));
    assert_eq!(game.sounds(), [WHISPER_SOUND], "one sound for each poll");
}

#[test]
fn a_shared_line_with_a_failed_notice_counts_the_failed_ones() {
    let game = Game::start();

    game.publish_and_poll(
        0,
        2,
        &[
            notice(1, NoticeKind::Failed, "lighthouse", 300, "overloaded"),
            finished(2, "gnomish-relay", 100),
        ],
    );

    let lines = game.lines_with("agents done");
    assert_eq!(lines.len(), 1, "{:?}", game.printed());
    assert!(lines[0].contains("2 agents done (1 failed): lighthouse, gnomish-relay"));
}

#[test]
fn a_long_text_is_cut_at_120_bytes_and_never_inside_a_doubled_pipe() {
    let game = Game::start();
    let text = format!("{}||x", "a".repeat(119));
    game.publish_and_poll(1, 1, &[notice(1, NoticeKind::Waiting, "r", 0, &text)]);
    let line = game.lines_with("Waiting for you").remove(0);
    assert!(
        line.contains(&format!("{}...|h", "a".repeat(119))),
        "{line}"
    );
}

#[test]
fn a_cut_keeps_a_whole_character_that_ends_at_the_limit_and_drops_a_split_one() {
    let game = Game::start();
    let whole = format!("{}é tail", "a".repeat(118));
    let split = format!("{}é tail", "b".repeat(119));

    game.publish_and_poll(
        1,
        2,
        &[
            notice(1, NoticeKind::Waiting, "r", 0, &whole),
            notice(2, NoticeKind::Waiting, "r", 0, &split),
        ],
    );

    let lines = game.lines_with("Waiting for you");
    assert!(
        lines[0].contains(&format!("{}é...|h", "a".repeat(118))),
        "{lines:?}"
    );
    assert!(
        lines[1].contains(&format!("{}...|h", "b".repeat(119))),
        "{lines:?}"
    );
}

#[test]
fn in_combat_the_line_shows_but_the_toast_and_the_sound_wait() {
    let game = Game::start();
    game.wow.set("combat", true).unwrap();

    game.publish_and_poll(1, 2, &[waiting(1), finished(2, "lighthouse", 300)]);

    assert_eq!(game.lines_with("Waiting for you").len(), 1);
    assert!(game.sounds().is_empty());
    assert!(!game.shown("GnomishRelayToast"));

    game.publish_and_poll(1, 2, &[finished(2, "lighthouse", 300)]);
    game.wow.set("combat", false).unwrap();
    game.run("local ns = ... ns.Notices.CombatEnded()");

    assert_eq!(
        game.sounds(),
        [WHISPER_SOUND],
        "the answered waiting notice gets nothing"
    );
    assert!(!game.shown("GnomishRelayToast"));
}

#[test]
fn the_end_of_combat_brings_the_held_toast() {
    let game = Game::start();
    game.wow.set("combat", true).unwrap();
    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.wow.set("combat", false).unwrap();

    common::fire(&game.lua, &game.wow, "PLAYER_REGEN_ENABLED", ());

    assert_eq!(game.sounds(), [TOAST_SOUND]);
    assert!(game.shown("GnomishRelayToast"));
}

#[test]
fn a_running_turn_polls_every_60_seconds_and_an_open_session_every_3_minutes() {
    let game = Game::start();
    game.advance(10.0);
    game.publish_and_poll(1, 1, &[]);
    game.poll_gaps_while_the_bridge_runs(700, 1, 1);
    let gaps = game.poll_gaps_while_the_bridge_runs(400, 1, 1);
    assert!(gaps.iter().all(|g| *g == 60), "{gaps:?}");

    game.publish_and_poll(0, 1, &[]);
    game.poll_gaps_while_the_bridge_runs(700, 0, 1);
    let gaps = game.poll_gaps_while_the_bridge_runs(800, 0, 1);
    assert!(gaps.iter().all(|g| *g == 180), "{gaps:?}");
}

#[test]
fn an_offline_bridge_stops_the_faster_polls() {
    let game = Game::start();
    game.advance(10.0);
    game.publish_and_poll(1, 1, &[]);

    game.advance(700.0);
    let gaps = game.poll_gaps(1300);

    assert!(gaps.iter().all(|g| *g == 600), "{gaps:?}");
}

#[test]
fn few_slots_left_print_one_line_and_the_last_slot_hides_the_bell() {
    let game = Game::start();
    let loaded: Table = game.wow.get("loaded").unwrap();
    for n in 1..=980 {
        loaded.set(format!("GnomishRelay_S{n:04}"), true).unwrap();
    }
    game.run("local ns = ... ns.Transport.Init()");

    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.poll();

    assert_eq!(game.lines_with("slots run low").len(), 1, "one line only");
    assert!(game.shown("GnomishRelayBell"));
    for _ in 0..18 {
        game.poll();
    }
    assert_eq!(game.list_len(), 0);
    assert!(!game.shown("GnomishRelayBell"), "no stale notice stays");
}

#[test]
fn with_notifications_off_nothing_shows_and_the_polls_stay_slow() {
    let game = Game::start();
    game.set("ns.Store.db.notifyOn = false");

    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.advance(700.0);
    let gaps = game.poll_gaps(1300);

    assert!(game.lines_with("Waiting").is_empty());
    assert!(game.sounds().is_empty());
    assert!(!game.shown("GnomishRelayBell"));
    assert!(!game.shown("GnomishRelayToast"));
    assert!(gaps.iter().all(|g| *g == 600), "{gaps:?}");
}

#[test]
fn each_alert_has_its_own_setting() {
    let game = Game::start();
    game.set(
        "ns.Store.db.notifyChat = false ns.Store.db.notifySound = false ns.Store.db.notifyToast = false",
    );

    game.publish_and_poll(1, 1, &[waiting(1)]);

    assert!(game.lines_with("Waiting").is_empty());
    assert!(game.sounds().is_empty());
    assert!(!game.shown("GnomishRelayToast"));
    assert!(
        game.shown("GnomishRelayBell"),
        "the bell has no setting of its own"
    );
}

#[test]
fn clear_empties_the_list_hides_the_bell_and_the_notice_never_comes_back() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.click("GnomishRelayBell");
    assert!(game.shown("GnomishRelayNotices"));

    game.click("GnomishRelayNoticesClear");
    game.publish_and_poll(1, 1, &[waiting(1)]);

    assert_eq!(game.list_len(), 0);
    assert!(!game.shown("GnomishRelayBell"));
    assert!(!game.shown("GnomishRelayNotices"));
    game.publish_and_poll(1, 1, &[waiting(1), waiting(2)]);
    assert_eq!(game.list_len(), 1, "a newer notice still comes");
}

#[test]
fn clear_sits_in_the_title_row_so_a_long_list_never_hides_it() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.click("GnomishRelayBell");

    let point = game.run(
        "local point, relative = GnomishRelayNoticesClear:GetPoint() return point .. ' ' .. tostring(relative == GnomishRelayNoticesClose)",
    );

    assert_eq!(
        point.as_string().unwrap().to_str().unwrap(),
        "RIGHT true",
        "left of the close button"
    );
}

#[test]
fn the_bell_and_the_line_open_the_list_and_escape_can_close_it() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);

    game.click("GnomishRelayBell");
    assert!(game.shown("GnomishRelayNotices"));
    game.click("GnomishRelayBell");
    assert!(!game.shown("GnomishRelayNotices"));

    game.run("SetItemRef('gnomishrelaynotices')");
    assert!(game.shown("GnomishRelayNotices"));
    let special: Vec<String> = game.lua.globals().get("UISpecialFrames").unwrap();
    assert!(special.contains(&"GnomishRelayNotices".to_owned()));
}

#[test]
fn a_row_shows_the_repo_the_state_the_age_and_folds_its_full_text() {
    let game = Game::start();
    let text = format!("Start. {} End.", "word ".repeat(40));
    game.publish_and_poll(
        0,
        1,
        &[notice(1, NoticeKind::Finished, "lighthouse", 240, &text)],
    );
    game.advance(120.0);
    game.click("GnomishRelayBell");

    let head = game.run("return GnomishRelayNotice1.head:GetText()");
    let head = head.as_string().unwrap().to_str().unwrap().to_owned();
    assert!(head.contains("lighthouse"), "{head}");
    assert!(head.contains("Finished · 4 min"), "{head}");
    assert!(head.contains("ago"), "{head}");
    let short = game.run("return GnomishRelayNotice1.text:GetText()");
    assert!(
        !short
            .as_string()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("End.")
    );

    game.click("GnomishRelayNotice1");
    let full = game.run("return GnomishRelayNotice1.text:GetText()");
    assert!(
        full.as_string()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("End.")
    );
    game.click("GnomishRelayNotice1");
    let folded = game.run("return GnomishRelayNotice1.text:GetText()");
    assert!(
        !folded
            .as_string()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("End.")
    );
}

#[test]
fn a_row_cuts_a_long_repo_so_the_state_and_the_age_still_show() {
    let game = Game::start();
    let repo = "gnomish-relay-experiments-2024-q3-and-more";
    game.publish_and_poll(0, 1, &[finished(1, repo, 240)]);
    game.click("GnomishRelayBell");

    let head = game.run("return GnomishRelayNotice1.head:GetText()");
    let head = head.as_string().unwrap().to_str().unwrap().to_owned();

    assert!(head.contains("gnomish-relay-experiment..."), "{head}");
    assert!(!head.contains(repo), "{head}");
    assert!(head.contains("Finished · 4 min"), "{head}");
}

#[test]
fn a_drag_moves_the_bell_along_the_edge_and_a_reload_keeps_its_angle() {
    let game = Game::start();
    game.publish_and_poll(1, 1, &[waiting(1)]);
    game.wow
        .set(
            "cursor",
            game.lua.create_sequence_from([1200, 700]).unwrap(),
        )
        .unwrap();

    game.run("GnomishRelayBell.scripts.OnDragStart(GnomishRelayBell)");
    game.run("GnomishRelayBell.scripts.OnUpdate(GnomishRelayBell, 0)");
    game.run("GnomishRelayBell.scripts.OnDragStop(GnomishRelayBell)");

    let angle = game.run("local ns = ... return math.floor(ns.Store.db.bellAngle + 0.5)");
    assert_eq!(angle.as_integer(), Some(90));
    assert_eq!(bell_y(&game), 80, "on the edge, above the center");
    let game = game.reload();
    assert_eq!(bell_y(&game), 80);
}

/// The offset of the bell from the center of the minimap, up.
fn bell_y(game: &Game) -> i64 {
    game.run("local _, _, _, _, y = GnomishRelayBell:GetPoint() return math.floor(y + 0.5)")
        .as_integer()
        .unwrap()
}

fn with_hooks(game: &Game, state: &str) {
    game.set(&format!(
        "ns.Store.db.settings = {{ id = 99, at = time(), text = 'version\\t0.2.0\\nhook\\tclaude\\t{state}\\nhook\\tcodex\\toff' }}"
    ));
}

#[test]
fn the_settings_group_shows_only_after_hooks_install() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('settings')");
    with_hooks(&game, "off");
    game.run("local ns = ... ns.SettingsTab.Refresh()");
    assert!(!game.shown("GnomishRelaySettingsNotify"));
    assert_eq!(rules_top(&game), -296);

    with_hooks(&game, "on");
    game.run("local ns = ... ns.SettingsTab.Refresh()");

    assert!(game.shown("GnomishRelaySettingsNotify"));
    assert_eq!(rules_top(&game), -390, "the rules move below the group");
}

/// The top of the "Always Allowed" rows on the Settings page.
fn rules_top(game: &Game) -> i64 {
    game.run("local _, _, _, _, y = GnomishRelayRules:GetPoint() return y")
        .as_integer()
        .unwrap()
}

#[test]
fn the_notifications_box_turns_everything_off_and_greys_the_other_rows() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('settings')");
    with_hooks(&game, "on");
    game.run("local ns = ... ns.SettingsTab.Refresh()");
    game.publish_and_poll(1, 1, &[waiting(1)]);
    assert!(game.shown("GnomishRelayBell"));

    game.click("GnomishRelaySettingsNotifyOn");

    assert_eq!(
        game.run("local ns = ... return ns.Store.db.notifyOn"),
        Value::Boolean(false)
    );
    assert!(!game.shown("GnomishRelayBell"));
    let alpha = game.run("return GnomishRelaySettingsNotifyChat.alpha");
    assert_eq!(alpha.as_number(), Some(0.35));
}

#[test]
fn the_finished_work_dropdown_sets_the_filter() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('settings')");
    with_hooks(&game, "on");
    game.run("local ns = ... ns.SettingsTab.Refresh()");

    game.click("GnomishRelaySettingsFinished");
    game.click("GnomishRelaySettingsFinishedChoice3");

    assert_eq!(
        game.run("local ns = ... return ns.Store.db.notifyFinished"),
        Value::String(game.lua.create_string("over3").unwrap())
    );
    let text = game.run("return GnomishRelaySettingsFinished:GetText()");
    assert_eq!(text.as_string().unwrap().to_str().unwrap(), "Over 3 min");
}

#[test]
fn a_new_finished_setting_filters_the_list_at_once_with_no_alert() {
    let game = Game::start();
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('settings')");
    with_hooks(&game, "on");
    game.run("local ns = ... ns.SettingsTab.Refresh()");
    game.set("ns.Store.db.notifyFinished = 'always'");
    game.publish_and_poll(0, 1, &[finished(1, "quick", 30)]);
    assert!(game.shown("GnomishRelayBell"));

    game.click("GnomishRelaySettingsFinished");
    game.click("GnomishRelaySettingsFinishedChoice3");
    assert_eq!(game.list_len(), 0);
    assert!(!game.shown("GnomishRelayBell"));

    game.click("GnomishRelaySettingsFinished");
    game.click("GnomishRelaySettingsFinishedChoice1");
    assert_eq!(game.list_len(), 1);
    assert_eq!(game.lines_with("quick").len(), 1, "no second line");
    assert_eq!(game.sounds(), [WHISPER_SOUND], "no second sound");
}

#[test]
fn diag_shows_the_hooks_the_sessions_and_the_last_notification_after_hooks_install() {
    let game = Game::start();
    with_hooks(&game, "on");
    game.publish_and_poll(1, 2, &[waiting(1)]);
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('diag')");

    let texts: Vec<String> = (1..=26)
        .filter_map(|i| {
            let value = game.run(&format!("return GnomishRelayDiagLine{i}.value:GetText()"));
            value.as_string().map(|s| s.to_str().unwrap().to_owned())
        })
        .collect();

    assert!(
        texts.iter().any(|t| t == "Claude on · Codex off"),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("1 running · 2 open")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t.ends_with(" ago")), "{texts:?}");
}

#[test]
fn diag_shows_a_moved_hook_with_the_fix_when_no_hook_is_on() {
    let game = Game::start();
    with_hooks(&game, "moved");
    game.run("local ns = ... ns.Window.Open() ns.Window.ShowTab('diag')");

    let texts: Vec<String> = (1..=26)
        .filter_map(|i| {
            let value = game.run(&format!("return GnomishRelayDiagLine{i}.value:GetText()"));
            value.as_string().map(|s| s.to_str().unwrap().to_owned())
        })
        .collect();

    assert!(
        texts
            .iter()
            .any(|t| t.contains("Claude moved: run gnomish-relay hooks install · Codex off")),
        "{texts:?}"
    );
}

#[test]
fn six_hundred_random_live_files_never_break_the_addon() {
    let game = Game::start();
    for seed in 0..600u64 {
        let text = String::from_utf8(notice_text(
            String::from_utf8_lossy(&bytes(seed, 40)).as_bytes(),
            600,
        ))
        .unwrap();
        let kinds = [
            NoticeKind::Waiting,
            NoticeKind::Finished,
            NoticeKind::Failed,
        ];
        let kind = kinds[usize::try_from(seed % 3).unwrap()];
        let first = u32::try_from(seed * 30).unwrap() + 1;
        let list: Vec<Notice> = (0..u32::try_from(seed % 25).unwrap())
            .map(|n| notice(first + n, kind, &text, n * 20, &text))
            .collect();
        let busy = u32::try_from(seed % 3).unwrap();
        game.publish_and_poll(busy, busy + 1, &list);
        game.advance(1.0);
    }
    let broken = "GnomishRelay_Live = { notices = { busy = 'x', list = { 5, { id = 'a' }, { id = 1, kind = 'waiting' } } } }";
    game.wow
        .set("live", game.lua.create_string(broken).unwrap())
        .unwrap();
    game.poll();
    assert_eq!(game.list_len(), 0);
}
