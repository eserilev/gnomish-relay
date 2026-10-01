//! The bridge on real folders: a screenshot in, the echo agent, a slot file out.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bridge::acp::AcpAgent;
use bridge::activity::text_hash;
use bridge::agent::{Agent, Control, Echo, Run};
use bridge::config::{Permission, Policy};
use bridge::daily_usage;
use bridge::desktop::{Approvals, Prompt, Verdict};
use bridge::folder_path::{path_bytes, real_path};
use bridge::full_auto::FullAutoAsker;
use bridge::gate::Gate;
use bridge::ids::hex;
use bridge::line_choice::{self, LineChoice};
use bridge::raise::Raiser;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::relay::Job;
use bridge::roots::Roots;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{BODY_FILE, LIVE_FILE, RESTORE_FILE, slot_name};
use bridge::trust::Truster;
use bridge::usage::Usage;
use bridge::vectors::TEST_KEY;
use common::{install_window, screenshot_png, signed_frame, strip_rows};
use protocol::apps::App;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

struct Dirs {
    _root: tempfile::TempDir,
    addons: std::path::PathBuf,
    screenshots: std::path::PathBuf,
    accounts: std::path::PathBuf,
    state: std::path::PathBuf,
}

fn folders() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let addons = root.path().join("Interface/AddOns");
    let screenshots = root.path().join("Screenshots");
    fs::create_dir_all(&addons).unwrap();
    let accounts = root.path().join("WTF/Account");
    let state = root.path().join("data");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&screenshots).unwrap();
    fs::create_dir_all(&accounts).unwrap();
    install_window(&addons, App::Relay);
    Dirs {
        _root: root,
        addons,
        screenshots,
        accounts,
        state,
    }
}

/// A run starts only in a folder that exists inside a root (SPEC.md 6.2, rule 10).
fn policy() -> Policy {
    let base = path_bytes(&std::env::temp_dir().canonicalize().unwrap());
    Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    }
}

fn bridge(f: &Dirs) -> Bridge {
    bridge_with(f, Arc::new(Echo))
}

fn bridge_with(f: &Dirs, agent: Arc<dyn Agent>) -> Bridge {
    bridge_in(f, policy(), agent)
}

fn bridge_in(f: &Dirs, policy: Policy, agent: Arc<dyn Agent>) -> Bridge {
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
        accounts: f.accounts.clone(),
        state: f.state.clone(),
        config: f.state.join("config"),
    };
    let agents = [("claude".to_owned(), agent)].into();
    Bridge::new(paths, policy, relay_keys(), agents).unwrap()
}

fn relay_keys() -> KeySet {
    KeySet::new(StripKey::from_hex(&hex(KEY)).unwrap(), None).unwrap()
}

fn frame(key: &[u8], text: &str) -> Vec<u8> {
    frame_with_id(key, 7, text)
}

fn frame_with_id(key: &[u8], id: u32, text: &str) -> Vec<u8> {
    let payload = format!("tok\x1fc1\x1f{id}\x1f\x1f\x1f\x1f{text}");
    signed_frame(now(), payload.as_bytes(), key)
}

fn strip_png(key: &[u8], text: &str) -> Vec<u8> {
    screenshot_png(&strip_rows(&frame(key, text)))
}

/// The bridge takes the frames of one file in their order, in one step.
fn write_saved_variables(f: &Dirs, frames: &[Vec<u8>]) {
    let dir = f.accounts.join("ACCOUNT1/SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let entries: Vec<String> = frames
        .iter()
        .map(|frame| format!("\t\t{{ [\"frame\"] = \"{}\" }},\n", hex(frame)))
        .collect();
    let outbox = entries.concat();
    let text = format!("GnomishRelayDB = {{\n\t[\"outbox\"] = {{\n{outbox}\t}},\n}}\n");
    fs::write(dir.join("GnomishRelay.lua"), text).unwrap();
}

fn slot_body(addons: &Path) -> String {
    fs::read_to_string(addons.join(slot_name(App::Relay, 1)).join(BODY_FILE)).unwrap()
}

/// Steps until `done` holds. A publish syncs 60 files, which is slow on Windows.
fn step_until(bridge: &mut Bridge, done: impl Fn() -> bool) -> bool {
    common::step_until_within(bridge, Duration::from_secs(30), done)
}

#[test]
fn a_strip_screenshot_comes_back_as_an_echo_in_the_slots() {
    let f = folders();
    let mut bridge = bridge(&f);
    let strip = f.screenshots.join("WoWScrnShot_1.png");
    fs::write(&strip, strip_png(KEY, "hi from the game")).unwrap();

    let answered = step_until(&mut bridge, || {
        slot_body(&f.addons).contains("echo: hi from the game")
    });
    assert!(answered, "{}", slot_body(&f.addons));
    assert!(!strip.exists(), "a valid strip is deleted");
}

#[test]
fn a_taken_strip_is_the_last_strip_of_the_status() {
    let f = folders();
    let mut bridge = bridge(&f);
    let before = now();
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "hi"),
    )
    .unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons).contains("echo: hi")));

    let last = bridge::status::last_strip(&f.state).unwrap();
    assert!(last >= before && last <= now(), "{last}");
}

#[test]
fn a_normal_screenshot_stays_untouched() {
    let f = folders();
    let mut bridge = bridge(&f);
    let user = f.screenshots.join("WoWScrnShot_user.png");
    fs::write(&user, screenshot_png(&[])).unwrap();
    // The bridge scans every file of the folder in one step, so this echo comes after it
    // looked at the other one.
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(user.exists());
}

#[test]
fn a_strip_signed_with_another_key_is_deleted_and_never_runs() {
    let f = folders();
    let mut bridge = bridge(&f);
    let forged = f.screenshots.join("WoWScrnShot_forged.png");
    fs::write(
        &forged,
        strip_png(b"another key, 32 bytes long......", "rm -rf ~"),
    )
    .unwrap();
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(!forged.exists(), "its pixels hold a prompt");
    assert!(!slot_body(&f.addons).contains("rm -rf"));
}

#[test]
fn a_strip_of_the_self_test_stays_for_selftest_collect() {
    let f = folders();
    let mut bridge = bridge(&f);
    let test_strip = f.screenshots.join("WoWScrnShot_selftest.png");
    fs::write(&test_strip, strip_png(TEST_KEY, "golden")).unwrap();
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(test_strip.exists());
    assert!(!slot_body(&f.addons).contains("golden"));
}

#[test]
fn a_stale_strip_is_deleted_and_never_runs() {
    let f = folders();
    let mut bridge = bridge(&f);
    let payload = "tok\x1fc1\x1f7\x1f\x1f\x1f\x1fold prompt";
    let old_frame = signed_frame(now() - 600, payload.as_bytes(), KEY);
    let stale = f.screenshots.join("WoWScrnShot_stale.png");
    fs::write(&stale, screenshot_png(&strip_rows(&old_frame))).unwrap();
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(!stale.exists());
    assert!(!slot_body(&f.addons).contains("old prompt"));
}

#[test]
fn a_future_strip_is_deleted_and_never_runs() {
    let f = folders();
    let mut bridge = bridge(&f);
    let payload = "tok\x1fc1\x1f7\x1f\x1f\x1f\x1ftoo early";
    let early_frame = signed_frame(now() + 600, payload.as_bytes(), KEY);
    let future = f.screenshots.join("WoWScrnShot_future.png");
    fs::write(&future, screenshot_png(&strip_rows(&early_frame))).unwrap();
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(!future.exists());
    assert!(!slot_body(&f.addons).contains("too early"));
}

#[test]
fn the_body_counts_strips_with_a_bad_tag_until_a_good_strip_comes() {
    let f = folders();
    let mut bridge = bridge(&f);
    let wrong_key = b"another key, 32 bytes long......";
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(wrong_key, "old key"),
    )
    .unwrap();

    assert!(
        step_until(&mut bridge, || slot_body(&f.addons)
            .ends_with("GnomishRelay_SlotData.badTags = 1\n")),
        "{}",
        slot_body(&f.addons)
    );

    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        strip_png(KEY, "new key"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: new key")));
    assert!(!slot_body(&f.addons).contains("badTags"));
}

#[test]
fn the_body_carries_the_newest_strip_line_while_its_file_exists() {
    let f = folders();
    let choice = LineChoice {
        mode: 2,
        width: 1920,
        height: 1080,
        reason: None,
    };
    line_choice::remember(&f.state, choice).unwrap();
    let mut bridge = bridge(&f);
    let line = "GnomishRelay_SlotData.line = {mode = 2, width = 1920, height = 1080}\n";

    assert!(
        step_until(&mut bridge, || slot_body(&f.addons).ends_with(line)),
        "{}",
        slot_body(&f.addons)
    );

    fs::remove_file(f.state.join(line_choice::FILE)).unwrap();
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "after the file went"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: after the file went")));
    assert!(!slot_body(&f.addons).contains(".line ="));
}

#[test]
fn an_outbox_frame_in_the_saved_variables_comes_back_as_an_echo() {
    let f = folders();
    let mut bridge = bridge(&f);
    write_saved_variables(&f, &[frame(KEY, "sent by reload")]);

    let answered = step_until(&mut bridge, || {
        slot_body(&f.addons).contains("echo: sent by reload")
    });
    assert!(answered, "{}", slot_body(&f.addons));
}

#[test]
fn an_outbox_frame_with_a_bad_tag_never_runs() {
    let f = folders();
    let mut bridge = bridge(&f);
    let forged = frame(b"another key, 32 bytes long......", "rm -rf ~");
    write_saved_variables(&f, &[forged, frame_with_id(KEY, 8, "marker")]);

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(!slot_body(&f.addons).contains("rm -rf"));
}

struct Counting(AtomicUsize);

impl Agent for Counting {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        self.0.fetch_add(1, Ordering::SeqCst);
        Run {
            reply: Ok(format!("echo: {}", job.text)),
            session: None,
            usage: None,
        }
    }
}

#[test]
fn a_restarted_bridge_never_runs_an_outbox_frame_again() {
    let f = folders();
    let runs = Arc::new(Counting(AtomicUsize::new(0)));
    let once = frame(KEY, "only once");
    write_saved_variables(&f, std::slice::from_ref(&once));
    let mut first = bridge_with(&f, runs.clone());
    assert!(step_until(&mut first, || slot_body(&f.addons)
        .contains("echo: only once")));
    drop(first);
    // One chat runs its jobs in order, so a second run of the old frame comes before this echo.
    write_saved_variables(&f, &[once, frame_with_id(KEY, 8, "marker")]);

    let mut second = bridge_with(&f, runs.clone());
    assert!(step_until(&mut second, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert_eq!(runs.0.load(Ordering::SeqCst), 2);
    assert!(slot_body(&f.addons).contains("echo: only once"));
}

#[test]
fn a_damaged_state_file_stops_the_bridge_at_start() {
    let f = folders();
    fs::write(f.state.join("state.json"), "{").unwrap();
    let paths = Paths {
        addons: f.addons.clone(),
        screenshots: f.screenshots.clone(),
        accounts: f.accounts.clone(),
        state: f.state.clone(),
        config: f.state.join("config"),
    };
    let folders = policy();
    let agents = [("claude".to_owned(), Arc::new(Echo) as Arc<dyn Agent>)].into();
    assert!(Bridge::new(paths, folders, relay_keys(), agents).is_err());
}

fn acp_bridge(f: &Dirs, root: &tempfile::TempDir, script: &str) -> Bridge {
    let base = path_bytes(&root.path().canonicalize().unwrap());
    let policy = Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        agents: [("claude".to_owned(), Permission::AutoEdit)].into(),
        default_agent: "claude".into(),
    };
    let fake = AcpAgent {
        command: vec![env!("CARGO_BIN_EXE_fake-acp-agent").into(), script.into()],
        env: Vec::new(),
        modes: std::collections::BTreeMap::new(),
        timeout: Duration::from_secs(20),
        permission_timeout: Duration::from_secs(20),
        gate: Gate::bare(
            vec![root.path().canonicalize().unwrap()],
            f.state.join("config"),
            f.state.clone(),
        ),
        wall: bridge::agent_wall::AgentWall::none(),
    };
    bridge_in(f, policy, Arc::new(fake))
}

#[test]
fn a_strip_comes_back_with_the_reply_of_an_acp_agent() {
    let f = folders();
    let root = tempfile::tempdir().unwrap();
    let mut bridge = acp_bridge(&f, &root, "reply");
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "fix the build"),
    )
    .unwrap();

    let answered = step_until(&mut bridge, || {
        slot_body(&f.addons).contains("you said: fix the build")
    });
    assert!(answered, "{}", slot_body(&f.addons));
}

/// The first request in `Live.lua`, read the way the addon reads it.
fn live_request(addons: &Path) -> Option<(String, Vec<u8>)> {
    let code = fs::read(addons.join(slot_name(App::Relay, 1)).join(LIVE_FILE)).ok()?;
    let lua = mlua::Lua::new();
    lua.load(&code[..]).exec().ok()?;
    let live: mlua::Table = lua.globals().get("GnomishRelay_Live").ok()?;
    let request: mlua::Table = live.get::<mlua::Table>("permissions").ok()?.get(1).ok()?;
    let id: String = request.get("request").ok()?;
    let text: mlua::String = request.get("text").ok()?;
    Some((id, text.as_bytes().to_vec()))
}

#[test]
fn a_permission_request_waits_for_the_answer_from_the_game() {
    let f = folders();
    let root = tempfile::tempdir().unwrap();
    let mut bridge = acp_bridge(&f, &root, "permission");
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "clean up"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || live_request(&f.addons).is_some()));
    let (request, text) = live_request(&f.addons).unwrap();
    assert!(text.starts_with(b"rm -rf build"));

    let flags = format!("perm={request}:o1:{}", text_hash(&text));
    let payload = format!("tok\x1fc1\x1f0\x1f\x1f{flags}\x1f\x1f");
    let answer = signed_frame(now(), payload.as_bytes(), KEY);
    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        screenshot_png(&strip_rows(&answer)),
    )
    .unwrap();

    assert!(
        step_until(&mut bridge, || slot_body(&f.addons).contains("chose yes")),
        "{}",
        slot_body(&f.addons)
    );
    assert!(live_request(&f.addons).is_none());
}

#[test]
fn a_new_message_ends_a_waiting_request_and_runs_next() {
    let f = folders();
    let root = tempfile::tempdir().unwrap();
    let mut bridge = acp_bridge(&f, &root, "permission");
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "clean up"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || live_request(&f.addons).is_some()));
    let (first, _) = live_request(&f.addons).unwrap();

    let payload = "tok\x1fc1\x1f8\x1f\x1f\x1f\x1fno, keep the build";
    let second = signed_frame(now(), payload.as_bytes(), KEY);
    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        screenshot_png(&strip_rows(&second)),
    )
    .unwrap();

    let asked_again = step_until(&mut bridge, || {
        live_request(&f.addons).is_some_and(|(request, _)| request != first)
    });
    assert!(asked_again, "{}", live_text(&f.addons));
    let body = slot_body(&f.addons);
    assert!(body.contains("id = 7, status = \"error\""), "{body}");
    assert!(body.contains("id = 8, status = \"working\""), "{body}");
}

/// An agent that works until the test lets it go.
struct Held(Arc<std::sync::atomic::AtomicBool>);

impl Agent for Held {
    fn run(&self, _job: &Job, _control: &Control) -> Run {
        while !self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        Run {
            reply: Ok("let go".into()),
            session: None,
            usage: None,
        }
    }
}

fn live_text(addons: &Path) -> String {
    fs::read_to_string(addons.join(slot_name(App::Relay, 1)).join(LIVE_FILE)).unwrap_or_default()
}

#[test]
fn a_run_shows_its_level_as_its_first_progress_line() {
    let f = folders();
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut bridge = bridge_with(&f, Arc::new(Held(release.clone())));
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "work"),
    )
    .unwrap();

    let shown = step_until(&mut bridge, || {
        live_text(&f.addons).contains(r#"lines = {"Level: auto-edit", }"#)
    });
    release.store(true, Ordering::SeqCst);
    assert!(shown, "{}", live_text(&f.addons));
}

#[test]
fn a_message_over_the_parallel_limit_waits_and_says_so_in_the_game() {
    let f = folders();
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut bridge = bridge_with(&f, Arc::new(Held(release.clone()))).with_max_runs(1);
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "", "first"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || live_text(&f.addons)
        .contains("Level: auto-edit")));

    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        chat_strip("c2", 8, "", "second"),
    )
    .unwrap();
    let waits = step_until(&mut bridge, || {
        live_text(&f.addons).contains(r#"id = 8, lines = {"Waiting: 1 other chat is running", }"#)
    });
    release.store(true, Ordering::SeqCst);

    assert!(waits, "{}", live_text(&f.addons));
    assert!(step_until(&mut bridge, || {
        slot_body(&f.addons).contains("id = 8, status = \"done\"")
    }));
}

/// The saved variables of one WoW account, as WoW writes them at a `/reload`. A rewrite
/// gets a later time, so the watcher sees it also within the same second.
fn write_account(f: &Dirs, account: &str, token: &str, later_secs: u64) {
    let dir = f.accounts.join(account).join("SavedVariables");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("GnomishRelay.lua");
    let text = format!("GnomishRelayDB = {{\n\t[\"token\"] = \"{token}\",\n}}\n");
    fs::write(&file, text).unwrap();
    let time = std::time::SystemTime::now() + Duration::from_secs(later_secs);
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn account_strip(token: &str, chat: &str, id: u32, flags: &str, text: &str) -> Vec<u8> {
    let payload = format!("{token}\x1f{chat}\x1f{id}\x1f\x1f{flags}\x1f\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), KEY)))
}

fn slot_file(addons: &Path, n: usize, file: &str) -> String {
    fs::read_to_string(addons.join(slot_name(App::Relay, n)).join(file)).unwrap_or_default()
}

/// Two accounts, each with a finished reply that it has not read yet. The second window
/// starts at slot 31.
fn two_accounts_with_replies() -> (Dirs, Bridge) {
    let f = folders();
    for n in 31..=60 {
        fs::create_dir(f.addons.join(slot_name(App::Relay, n))).unwrap();
    }
    write_account(&f, "ACCOUNT1", "one", 0);
    write_account(&f, "ACCOUNT2", "two", 0);
    let mut bridge = bridge(&f);
    let strips = [
        account_strip("one", "relay", 0, "h;next=1", ""),
        account_strip("two", "relay", 0, "h;next=31", ""),
        account_strip("one", "c1", 7, "", "from one"),
        account_strip("two", "c2", 8, "", "from two"),
    ];
    for (n, strip) in strips.iter().enumerate() {
        fs::write(f.screenshots.join(format!("WoWScrnShot_{n}.png")), strip).unwrap();
        bridge.step();
    }
    assert!(step_until(&mut bridge, || both_replies(&f, 1) && both_replies(&f, 31)));
    (f, bridge)
}

fn both_replies(f: &Dirs, slot: usize) -> bool {
    let body = slot_file(&f.addons, slot, BODY_FILE);
    body.contains("echo: from one") && body.contains("echo: from two")
}

#[test]
fn two_accounts_that_play_at_once_both_get_their_replies_in_their_windows() {
    let (f, mut bridge) = two_accounts_with_replies();

    write_account(&f, "ACCOUNT2", "two", 60);
    bridge.step();

    assert!(both_replies(&f, 1));
}

#[test]
fn a_wipe_in_one_account_restores_its_chats_and_retires_only_its_old_token() {
    let (f, mut bridge) = two_accounts_with_replies();

    fs::write(
        f.screenshots.join("WoWScrnShot_9.png"),
        account_strip("fresh", "relay", 0, "h;next=1", ""),
    )
    .unwrap();
    let restored = step_until(&mut bridge, || {
        slot_file(&f.addons, 1, RESTORE_FILE).contains("token = \"fresh\"")
    });
    write_account(&f, "ACCOUNT1", "fresh", 60);
    let retired = step_until(&mut bridge, || {
        !slot_file(&f.addons, 1, BODY_FILE).contains("echo: from one")
    });

    assert!(restored);
    assert!(retired);
    assert!(slot_file(&f.addons, 1, BODY_FILE).contains("echo: from two"));
    assert!(slot_file(&f.addons, 31, BODY_FILE).contains("echo: from two"));
}

/// An agent whose every run costs 50 cents, and counts its runs.
struct Costly(AtomicUsize);

impl Agent for Costly {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        self.0.fetch_add(1, Ordering::SeqCst);
        Run {
            reply: Ok(format!("paid: {}", job.text)),
            session: None,
            usage: Some(Usage {
                input: 1234,
                cached: 0,
                output: 350,
                cost_usd: Some(0.5),
            }),
        }
    }
}

#[test]
fn a_reply_shows_the_usage_of_its_run_and_the_day_counts_it() {
    let f = folders();
    let mut bridge = bridge_with(&f, Arc::new(Costly(AtomicUsize::new(0))));

    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "", "work"),
    )
    .unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons).contains(
        "\\027M1\\010u\\0311.2k in \\194\\183 350 out \\194\\183 $0.50\\010"
    )));
    let days = fs::read_to_string(f.state.join(daily_usage::FILE)).unwrap();
    assert!(days.contains(&daily_usage::day_of(now())), "{days}");
    assert!(days.contains("\"cost_usd\":0.5"), "{days}");
}

#[test]
fn at_the_daily_cap_a_new_message_does_not_start_and_says_why() {
    let f = folders();
    let agent = Arc::new(Costly(AtomicUsize::new(0)));
    let mut bridge = bridge_with(&f, agent.clone()).with_cost_cap(Some(0.5));
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "", "first"),
    )
    .unwrap();
    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("id = 7, status = \"done\"")));

    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        chat_strip("c1", 8, "", "second"),
    )
    .unwrap();

    assert!(step_until(&mut bridge, || {
        slot_body(&f.addons)
        .contains("id = 8, status = \"error\", text = \"Not started: today's agent cost reached your $0.50 limit.")
    }));
    assert_eq!(agent.0.load(Ordering::SeqCst), 1);
}

/// An agent with saved sessions, or one whose list fails.
struct Sessions(Result<Vec<bridge::agent::SessionInfo>, String>);

impl Agent for Sessions {
    fn run(&self, _job: &Job, _control: &Control) -> Run {
        Run {
            reply: Ok(String::new()),
            session: None,
            usage: None,
        }
    }

    fn sessions(&self, _cwd: &str) -> Result<Vec<bridge::agent::SessionInfo>, String> {
        self.0.clone()
    }
}

fn list_strip(f: &Dirs) {
    let payload = b"tok\x1frelay\x1f8\x1f\x1flist\x1f\x1f";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_2.png"), png).unwrap();
}

#[test]
fn a_list_request_comes_back_with_the_sessions_of_the_agents() {
    let f = folders();
    let app = tempfile::tempdir().unwrap();
    let found = vec![bridge::agent::SessionInfo {
        id: "a1".into(),
        cwd: real_path(app.path())
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        title: "Fix bugs".into(),
        updated: now() - 7200,
    }];
    let mut bridge = bridge_with(&f, Arc::new(Sessions(Ok(found))));
    list_strip(&f);
    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons).contains("Fix bugs")));
    assert!(slot_body(&f.addons).contains(r#"chat = "relay", id = 8, status = "done""#));
}

#[test]
fn a_list_that_fails_in_every_agent_is_an_error() {
    let f = folders();
    let mut bridge = bridge_with(&f, Arc::new(Sessions(Err("not logged in".into()))));
    list_strip(&f);
    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons)
        .contains("claude: not logged in")));
    assert!(slot_body(&f.addons).contains(r#"status = "error""#));
}

/// An agent that answers with the level of its run.
struct LevelOf;

impl Agent for LevelOf {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        Run {
            reply: Ok(format!("ran at {}", job.permission.word())),
            session: None,
            usage: None,
        }
    }
}

const ASK_CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"claude\"\n\
    [wow]\npath = \"~/wow\"\n\
    [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"ask\"\n";

/// A bridge whose config allows `ask`, and whose raises answer through `approvals`.
fn raising_bridge(f: &Dirs, approvals: &Approvals) -> (Bridge, std::path::PathBuf) {
    let home = f.state.parent().unwrap().to_owned();
    fs::create_dir_all(home.join("Code")).unwrap();
    let config_dir = home.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    bridge::fs_safe::write_private(&config_dir, "config.toml", ASK_CONFIG).unwrap();
    let mut policy = policy();
    policy.agents.insert("claude".into(), Permission::Ask);
    let raiser = Raiser {
        approvals: approvals.clone(),
        config_dir: config_dir.clone(),
        home,
        permission_timeout: Duration::from_secs(20),
        free_commands: Vec::new(),
    };
    let bridge = bridge_in(f, policy, Arc::new(LevelOf)).with_raises(raiser);
    (bridge, config_dir.join("config.toml"))
}

fn chat_strip(chat: &str, id: u32, flags: &str, text: &str) -> Vec<u8> {
    let payload = format!("tok\x1f{chat}\x1f{id}\x1f\x1f{flags}\x1f\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), KEY)))
}

/// Answers every raise with `verdict`, and returns how many it saw.
fn answer_raises(
    approvals: &Approvals,
    verdict: Verdict,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<usize> {
    let approvals = approvals.clone();
    std::thread::spawn(move || {
        let mut seen = std::collections::BTreeSet::new();
        while !stop.load(Ordering::SeqCst) {
            for open in approvals.list() {
                if seen.insert(open.id.clone()) {
                    let _ = approvals.answer(&open.id, verdict);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        seen.len()
    })
}

#[test]
fn an_approved_raise_writes_the_config_and_the_run_uses_the_new_level() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let (mut bridge, config) = raising_bridge(&f, &approvals);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Approve, stop.clone());
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "level=auto-edit", "edit it"),
    )
    .unwrap();

    let done = step_until(&mut bridge, || slot_body(&f.addons).contains("ran at"));
    stop.store(true, Ordering::SeqCst);
    assert!(done);
    assert_eq!(answering.join().unwrap(), 1);
    assert!(
        slot_body(&f.addons).contains("ran at auto-edit"),
        "{}",
        slot_body(&f.addons)
    );
    assert!(
        fs::read_to_string(config)
            .unwrap()
            .contains("permission = \"auto-edit\"")
    );
}

#[test]
fn a_denied_raise_keeps_the_config_and_the_run_goes_on_at_its_level() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let (mut bridge, config) = raising_bridge(&f, &approvals);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Deny, stop.clone());
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "level=auto-edit", "edit it"),
    )
    .unwrap();

    let done = step_until(&mut bridge, || slot_body(&f.addons).contains("ran at"));
    stop.store(true, Ordering::SeqCst);
    assert!(done);
    assert_eq!(answering.join().unwrap(), 1);
    assert!(
        slot_body(&f.addons).contains("ran at ask"),
        "{}",
        slot_body(&f.addons)
    );
    assert_eq!(fs::read_to_string(config).unwrap(), ASK_CONFIG);
}

#[test]
fn a_chat_at_full_auto_raises_the_config_to_auto_edit_only() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let (mut bridge, config) = raising_bridge(&f, &approvals);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Approve, stop.clone());
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "level=full-auto", "edit it"),
    )
    .unwrap();

    let done = step_until(&mut bridge, || slot_body(&f.addons).contains("ran at"));
    stop.store(true, Ordering::SeqCst);
    assert!(done);
    assert_eq!(answering.join().unwrap(), 1);
    assert!(slot_body(&f.addons).contains("ran at auto-edit"));
    assert!(
        fs::read_to_string(config)
            .unwrap()
            .contains("permission = \"auto-edit\"")
    );
}

#[test]
fn two_chats_that_ask_for_more_at_once_get_one_dialog() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let (mut bridge, _config) = raising_bridge(&f, &approvals);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Deny, stop.clone());
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        chat_strip("c1", 7, "level=auto-edit", "one"),
    )
    .unwrap();
    fs::write(
        f.screenshots.join("WoWScrnShot_2.png"),
        chat_strip("c2", 8, "level=auto-edit", "two"),
    )
    .unwrap();

    let done = step_until(&mut bridge, || {
        slot_body(&f.addons).matches("ran at ask").count() == 2
    });
    stop.store(true, Ordering::SeqCst);
    assert!(done, "{}", slot_body(&f.addons));
    assert_eq!(answering.join().unwrap(), 1);
}

/// An agent that answers with the message and the level of its run.
struct SaysLevel;

impl Agent for SaysLevel {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        Run {
            reply: Ok(format!("{} ran at {}", job.text, job.permission.word())),
            session: None,
            usage: None,
        }
    }
}

/// A bridge with claude at `auto-edit` in the config. `walls` lists the agents whose
/// walls hold. With `None`, `allow_full_auto = false`.
fn full_auto_bridge(f: &Dirs, approvals: &Approvals, walls: Option<&[&str]>) -> Bridge {
    let bridge = bridge_in(f, policy(), Arc::new(SaysLevel));
    let Some(walls) = walls else {
        return bridge;
    };
    bridge.with_full_auto(FullAutoAsker {
        approvals: approvals.clone(),
        permission_timeout: Duration::from_secs(20),
        agents: walls.iter().map(|a| (*a).to_owned()).collect(),
    })
}

fn chat_strip_in(cwd: &str, id: u32, flags: &str, text: &str) -> Vec<u8> {
    let payload = format!("tok\x1fc1\x1f{id}\x1f{cwd}\x1f{flags}\x1fFix tests\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), KEY)))
}

/// Sends one message of the chat `c1` and steps until its reply comes.
fn send_and_wait(bridge: &mut Bridge, f: &Dirs, cwd: &str, id: u32, flags: &str) -> String {
    let text = format!("message {id}");
    fs::write(
        f.screenshots.join(format!("WoWScrnShot_{id}.png")),
        chat_strip_in(cwd, id, flags, &text),
    )
    .unwrap();
    let answered = step_until(bridge, || slot_body(&f.addons).contains(&text));
    assert!(answered, "{}", slot_body(&f.addons));
    let body = slot_body(&f.addons);
    let at = body.find(&format!("{text} ran at ")).unwrap();
    body[at..].split(['"', '\\']).next().unwrap().to_owned()
}

/// Answers every desktop request with `verdict` and keeps the text of each one.
fn answer_requests(
    approvals: &Approvals,
    verdict: Verdict,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<String>> {
    let approvals = approvals.clone();
    std::thread::spawn(move || {
        let mut seen = std::collections::BTreeMap::new();
        while !stop.load(Ordering::SeqCst) {
            for open in approvals.list() {
                if !seen.contains_key(&open.id) {
                    let _ = approvals.answer(&open.id, verdict);
                    seen.insert(open.id.clone(), open.text.clone());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        seen.into_values().collect()
    })
}

#[test]
fn the_first_switch_to_full_auto_asks_once_on_the_desktop_and_the_next_message_does_not() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());

    let first = send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");
    let second = send_and_wait(&mut bridge, &f, "", 8, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    let asked = answering.join().unwrap();
    assert_eq!(first, "message 7 ran at full-auto");
    assert_eq!(second, "message 8 ran at full-auto");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].starts_with("Let claude run anything with no question in the chat \"Fix tests\""),
        "{}",
        asked[0]
    );
    assert!(asked[0].contains("It stays in the sandbox"), "{}", asked[0]);
}

#[test]
fn a_denied_full_auto_runs_the_chat_at_auto_edit() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Deny, stop.clone());

    let first = send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");
    let second = send_and_wait(&mut bridge, &f, "", 8, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(first, "message 7 ran at auto-edit");
    assert_eq!(second, "message 8 ran at auto-edit");
    assert_eq!(
        answering.join().unwrap().len(),
        1,
        "a deny ends the dialogs of the chat"
    );
}

#[test]
fn a_full_auto_approval_survives_a_restart_of_the_bridge() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());
    let mut first = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    send_and_wait(&mut first, &f, "", 7, "level=full-auto");
    // The approval reaches `state.json` with the next publish.
    step_until(&mut first, || {
        fs::read_to_string(f.state.join("state.json")).is_ok_and(|s| s.contains("full_auto"))
    });
    drop(first);

    let mut restarted = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    let again = send_and_wait(&mut restarted, &f, "", 8, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(again, "message 8 ran at full-auto");
    assert_eq!(answering.join().unwrap().len(), 1);
}

#[test]
fn a_new_folder_asks_for_full_auto_again() {
    let f = folders();
    let other = f.state.parent().unwrap().join("other");
    fs::create_dir_all(&other).unwrap();
    let other = other.canonicalize().unwrap().to_string_lossy().into_owned();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());

    send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");
    let moved = send_and_wait(&mut bridge, &f, &other, 8, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    let asked = answering.join().unwrap();
    assert_eq!(moved, "message 8 ran at full-auto");
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked.iter().any(|text| text.contains(&other)), "{asked:?}");
}

#[test]
fn a_lower_level_forgets_the_full_auto_approval() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, Some(&["claude"]));
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());

    send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");
    let down = send_and_wait(&mut bridge, &f, "", 8, "level=auto-edit");
    let up = send_and_wait(&mut bridge, &f, "", 9, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(down, "message 8 ran at auto-edit");
    assert_eq!(up, "message 9 ran at full-auto");
    assert_eq!(
        answering.join().unwrap().len(),
        2,
        "the switch up asks again"
    );
}

#[test]
fn with_allow_full_auto_off_a_chat_at_full_auto_runs_at_auto_edit_with_no_dialog() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, None);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());

    let reply = send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(reply, "message 7 ran at auto-edit");
    assert!(answering.join().unwrap().is_empty());
}

#[test]
fn an_agent_whose_wall_leaks_gets_no_full_auto_dialog() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut bridge = full_auto_bridge(&f, &approvals, Some(&[]));
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_requests(&approvals, Verdict::Approve, stop.clone());

    let reply = send_and_wait(&mut bridge, &f, "", 7, "level=full-auto");

    stop.store(true, Ordering::SeqCst);
    assert_eq!(reply, "message 7 ran at auto-edit");
    assert!(answering.join().unwrap().is_empty());
}

/// A bridge whose root and default folder is the temp folder of `f`.
fn bridge_in_temp(f: &Dirs) -> (Bridge, std::path::PathBuf) {
    let root = f.state.parent().unwrap().canonicalize().unwrap();
    let base = path_bytes(&root);
    let policy = Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        ..policy()
    };
    (bridge_in(f, policy, Arc::new(Echo)), root)
}

#[test]
fn a_folder_list_comes_back_with_the_folder_tree_but_never_the_config_folder() {
    let f = folders();
    let (mut bridge, root) = bridge_in_temp(&f);
    fs::create_dir_all(root.join("Code/app/.git")).unwrap();
    fs::create_dir_all(f.state.join("config")).unwrap();
    let payload = b"tok\x1ffolders\x1f8\x1f\x1flist=folders\x1f\x1f";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_3.png"), png).unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons)
        .contains(r#"chat = "folders", id = 8, status = "done""#)));

    let body = slot_body(&f.addons);
    assert!(body.contains("\\009Code\\009"), "{body}");
    assert!(body.contains("\\009app\\009g"), "{body}");
    assert!(
        !body.contains("\\009data\\009"),
        "the data folder of the bridge is a deny folder"
    );
    assert!(!body.contains("config"), "{body}");
}

#[test]
fn a_folder_past_the_walk_depth_is_marked_and_lists_its_real_subfolders_on_request() {
    let f = folders();
    let (mut bridge, root) = bridge_in_temp(&f);
    fs::create_dir_all(root.join("Code/a/b/deep/inside")).unwrap();
    let list = b"tok\x1ffolders\x1f8\x1f\x1flist=folders\x1f\x1f";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), list, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_3.png"), png).unwrap();
    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons)
        .contains(r#"chat = "folders", id = 8, status = "done""#)));
    let body = slot_body(&f.addons);
    assert!(body.contains("\\009deep\\009"), "{body}");
    assert!(!body.contains("inside"), "{body}");
    assert!(
        body.contains("\\010?"),
        "the tree marks deep as not walked: {body}"
    );

    let below = b"tok\x1fsubfolders\x1f9\x1fCode/a/b/deep\x1flist=subfolders\x1f\x1f";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), below, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_4.png"), png).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&addons)
        .contains(r#"chat = "subfolders", id = 9, status = "done""#)));
    let body = slot_body(&f.addons);
    assert!(body.contains("\\0101\\009inside\\009"), "{body}");
}

#[test]
fn the_first_message_in_a_new_folder_makes_it_and_runs_in_it() {
    let f = folders();
    let (mut bridge, root) = bridge_in_temp(&f);
    fs::create_dir_all(root.join("Code")).unwrap();
    let payload = b"tok\x1fc1\x1f9\x1fCode/fresh\x1fn;mkdir=1\x1f\x1fhello";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_4.png"), png).unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons).contains("echo: hello")));

    assert!(root.join("Code/fresh").is_dir());
}

/// The relay checks the text of the folder. Only the start of the run sees the link.
#[cfg(unix)]
#[test]
fn a_chat_folder_that_is_a_link_out_of_the_roots_never_runs() {
    let f = folders();
    let (mut bridge, root) = bridge_in_temp(&f);
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("app")).unwrap();
    let payload = b"tok\x1fc1\x1f9\x1fapp\x1f\x1f\x1fhello";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_6.png"), png).unwrap();

    let addons = f.addons.clone();
    let ended = step_until(&mut bridge, || {
        slot_body(&addons).contains("outside allowed_roots")
    });

    assert!(ended, "{}", slot_body(&f.addons));
    assert!(!slot_body(&f.addons).contains("echo: hello"));
}

#[test]
fn a_new_folder_whose_parent_is_missing_ends_as_an_error_and_makes_nothing() {
    let f = folders();
    let (mut bridge, root) = bridge_in_temp(&f);
    let payload = b"tok\x1fc1\x1f9\x1fnone/fresh\x1fn;mkdir=1\x1f\x1fhello";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_5.png"), png).unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut bridge, || slot_body(&addons).contains(
        "Couldn't create the folder: the folder it goes in is gone."
    )));

    assert!(!root.join("none").exists());
    assert!(!slot_body(&f.addons).contains("echo: hello"));
}

const WAITING_FILE: &str = r#"{"v":1,"source":"claude","event":"waiting","session":"s1","repo":"app","text":"Allow Bash?"}"#;

#[test]
fn a_spool_file_reaches_the_live_file_and_leaves_the_folder() {
    let f = folders();
    let mut bridge = bridge(&f);
    let spool = f.state.join("notices");
    fs::write(spool.join("1.json"), WAITING_FILE).unwrap();
    let live = f.addons.join(slot_name(App::Relay, 1)).join(LIVE_FILE);

    let shown = common::step_until_within(&mut bridge, Duration::from_secs(5), || {
        fs::read_to_string(&live)
            .unwrap()
            .contains("text = \"Allow Bash?\"")
    });

    assert!(shown);
    assert_eq!(fs::read_dir(&spool).unwrap().count(), 0);
    assert!(f.state.join("notices.json").is_file());
}

#[test]
fn a_start_of_the_bridge_empties_the_spool_folder() {
    let f = folders();
    let spool = f.state.join("notices");
    fs::create_dir_all(&spool).unwrap();
    fs::write(spool.join("1.json"), WAITING_FILE).unwrap();

    let mut bridge = bridge(&f);
    bridge.step();

    let live = f.addons.join(slot_name(App::Relay, 1)).join(LIVE_FILE);
    assert!(!fs::read_to_string(live).unwrap().contains("Allow Bash?"));
}

const TRUST_CONFIG: &str = "allowed_roots = [\"~/Code\"]\ndefault_agent = \"claude\"\n\
    [wow]\npath = \"~/wow\"\n\
    [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"auto-edit\"\n";

struct Trusting {
    bridge: Bridge,
    home: std::path::PathBuf,
    config: std::path::PathBuf,
}

/// A bridge in the home folder of `f`, with the one root `~/Code`, whose folder
/// requests answer through `approvals` (SPEC.md 9.12).
fn trusting_bridge(f: &Dirs, approvals: &Approvals) -> Trusting {
    // As in `start`: `canonicalize` puts `\\?\` before the home on Windows, and then no
    // folder shows as `~`.
    let home = real_path(f.state.parent().unwrap()).unwrap();
    let code = home.join("Code");
    fs::create_dir_all(&code).unwrap();
    let config_dir = home.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    bridge::fs_safe::write_private(&config_dir, "config.toml", TRUST_CONFIG).unwrap();
    let base = path_bytes(&code);
    let policy = Policy {
        folders: Folders {
            roots: vec![base.clone()],
            base,
        },
        ..policy()
    };
    let truster = Truster {
        approvals: approvals.clone(),
        config_dir: config_dir.clone(),
        home: home.clone(),
        permission_timeout: Duration::from_secs(20),
        roots: Roots::new(vec![code]),
    };
    let bridge = bridge_in(f, policy, Arc::new(Echo)).with_trust(truster);
    Trusting {
        bridge,
        home,
        config: config_dir.join("config.toml"),
    }
}

fn folder_strip(chat: &str, id: u32, cwd: &str, text: &str) -> Vec<u8> {
    let payload = format!("tok\x1f{chat}\x1f{id}\x1f{cwd}\x1fn\x1f\x1f{text}");
    screenshot_png(&strip_rows(&signed_frame(now(), payload.as_bytes(), KEY)))
}

#[test]
fn a_chat_in_a_folder_under_no_root_waits_for_the_desktop_and_runs_after_approve() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut t = trusting_bridge(&f, &approvals);
    fs::create_dir_all(t.home.join("lighthouse")).unwrap();
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        folder_strip("c1", 7, "../lighthouse", "hi"),
    )
    .unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut t.bridge, || live_text(&addons)
        .contains(" command folder")));
    assert!(
        !slot_body(&f.addons).contains("echo: hi"),
        "no run before the click"
    );
    let open = approvals.list();
    assert_eq!(open.len(), 1);
    assert!(
        open[0]
            .text
            .starts_with("Let agents from WoW work in ~/lighthouse?"),
        "{}",
        open[0].text
    );
    approvals.answer(&open[0].id, Verdict::Approve).unwrap();

    assert!(step_until(&mut t.bridge, || slot_body(&addons).contains("echo: hi")));
    let config = fs::read_to_string(&t.config).unwrap();
    assert!(
        config.starts_with("allowed_roots = [\"~/Code\", \"~/lighthouse\"]\n"),
        "{config}"
    );
}

#[test]
fn a_denied_folder_ends_the_message_with_a_reply_and_no_run() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut t = trusting_bridge(&f, &approvals);
    fs::create_dir_all(t.home.join("lighthouse")).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Deny, stop.clone());
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        folder_strip("c1", 7, "../lighthouse", "hi"),
    )
    .unwrap();

    let addons = f.addons.clone();
    let done = step_until(&mut t.bridge, || {
        slot_body(&addons).contains("Denied on your desktop. Agents can't work in this folder.")
    });
    stop.store(true, Ordering::SeqCst);
    assert!(done, "{}", slot_body(&f.addons));
    assert_eq!(answering.join().unwrap(), 1);
    assert!(!slot_body(&f.addons).contains("echo: hi"));
    assert_eq!(fs::read_to_string(&t.config).unwrap(), TRUST_CONFIG);
}

#[test]
fn the_home_folder_a_hidden_folder_and_a_private_folder_are_refused_with_no_dialog() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut t = trusting_bridge(&f, &approvals);
    fs::create_dir_all(t.home.join(".secret")).unwrap();
    let cases = [
        ("c1", "..", "your whole home folder"),
        ("c2", "../.secret", "hidden or system folders"),
        ("c3", "../data", "it holds private files"),
        ("c4", "../..", "outside your home folder"),
    ];
    for (n, (chat, cwd, _)) in (1..).zip(cases) {
        fs::write(
            f.screenshots.join(format!("WoWScrnShot_{n}.png")),
            folder_strip(chat, n, cwd, "hi"),
        )
        .unwrap();
    }

    let addons = f.addons.clone();
    let done = step_until(&mut t.bridge, || {
        let body = slot_body(&addons);
        cases.iter().all(|(_, _, reason)| body.contains(reason))
    });

    assert!(done, "{}", slot_body(&f.addons));
    assert!(approvals.list().is_empty());
    assert!(!slot_body(&f.addons).contains("echo: hi"));
    assert_eq!(fs::read_to_string(&t.config).unwrap(), TRUST_CONFIG);
}

#[test]
fn a_flood_of_messages_for_new_folders_gets_one_dialog() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut t = trusting_bridge(&f, &approvals);
    for n in 1..=5 {
        fs::create_dir_all(t.home.join(format!("p{n}"))).unwrap();
        fs::write(
            f.screenshots.join(format!("WoWScrnShot_{n}.png")),
            folder_strip(&format!("c{n}"), n, &format!("../p{n}"), "hi"),
        )
        .unwrap();
    }

    let addons = f.addons.clone();
    let done = step_until(&mut t.bridge, || {
        slot_body(&addons)
            .matches("Another folder waits for your answer on your desktop.")
            .count()
            == 4
    });

    assert!(done, "{}", slot_body(&f.addons));
    assert_eq!(approvals.list().len(), 1);
}

#[test]
fn the_folder_list_shows_the_home_folder_but_no_hidden_or_private_folder() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let mut t = trusting_bridge(&f, &approvals);
    fs::create_dir_all(t.home.join("Code/app/.git")).unwrap();
    fs::create_dir_all(t.home.join("lighthouse")).unwrap();
    fs::create_dir_all(t.home.join(".secret/inside")).unwrap();
    fs::create_dir_all(t.home.join("snap/firefox")).unwrap();
    let payload = b"tok\x1ffolders\x1f8\x1f\x1flist=folders\x1f\x1f";
    let png = screenshot_png(&strip_rows(&signed_frame(now(), payload, KEY)));
    fs::write(f.screenshots.join("WoWScrnShot_3.png"), png).unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut t.bridge, || slot_body(&addons)
        .contains(r#"chat = "folders", id = 8, status = "done""#)));

    let body = slot_body(&f.addons);
    assert!(body.contains("\\009lighthouse\\009"), "{body}");
    assert!(body.contains("\\009app\\009g"), "{body}");
    for hidden in [".secret", "inside", "firefox", "\\009data\\009"] {
        assert!(!body.contains(hidden), "{hidden}: {body}");
    }
}

/// An agent with saved sessions that counts its attaches.
struct Saved {
    sessions: Vec<bridge::agent::SessionInfo>,
    attaches: Arc<AtomicUsize>,
}

impl Agent for Saved {
    fn run(&self, job: &Job, _control: &Control) -> Run {
        self.attaches.fetch_add(1, Ordering::SeqCst);
        Run {
            reply: Ok(format!("attached in {}", job.cwd)),
            session: None,
            usage: None,
        }
    }

    fn sessions(&self, _cwd: &str) -> Result<Vec<bridge::agent::SessionInfo>, String> {
        Ok(self.sessions.clone())
    }
}

fn saved_session(id: &str, folder: &Path) -> bridge::agent::SessionInfo {
    bridge::agent::SessionInfo {
        id: id.into(),
        cwd: folder.to_string_lossy().into_owned(),
        title: format!("work in {id}"),
        updated: now() - 7200,
    }
}

const NO_ROOTS_CONFIG: &str = "allowed_roots = []\ndefault_agent = \"claude\"\n\
    [wow]\npath = \"~/wow\"\n\
    [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"auto-edit\"\n";

/// A bridge in the home folder of `f` with no root, as after a setup that found no code
/// folder. Its folder requests answer through `approvals` (SPEC.md 9.12).
fn bridge_with_no_roots(f: &Dirs, approvals: &Approvals, agent: Saved) -> Trusting {
    let home = real_path(f.state.parent().unwrap()).unwrap();
    let config_dir = home.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    bridge::fs_safe::write_private(&config_dir, "config.toml", NO_ROOTS_CONFIG).unwrap();
    let policy = Policy {
        folders: Folders {
            roots: Vec::new(),
            base: path_bytes(&home),
        },
        ..policy()
    };
    let truster = Truster {
        approvals: approvals.clone(),
        config_dir: config_dir.clone(),
        home: home.clone(),
        permission_timeout: Duration::from_secs(20),
        roots: Roots::new(Vec::new()),
    };
    let bridge = bridge_in(f, policy, Arc::new(agent)).with_trust(truster);
    Trusting {
        bridge,
        home,
        config: config_dir.join("config.toml"),
    }
}

/// Sends a Resume list, and waits for its reply.
fn resume_list(f: &Dirs, bridge: &mut Bridge) -> String {
    list_strip(f);
    let addons = f.addons.clone();
    assert!(step_until(bridge, || slot_body(&addons)
        .contains(r#"chat = "relay", id = 8, status = "done""#)));
    slot_body(&f.addons)
}

#[test]
fn resume_with_no_roots_lists_sessions_in_the_home_folder_but_no_gone_hidden_or_private_one() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let home = real_path(f.state.parent().unwrap()).unwrap();
    for folder in ["lighthouse", ".secret", "snap/firefox"] {
        fs::create_dir_all(home.join(folder)).unwrap();
    }
    let sessions = ["lighthouse", "gone", ".secret", "snap/firefox", "data", ""]
        .iter()
        .zip(1..)
        .map(|(folder, n)| saved_session(&format!("s{n}"), &home.join(folder)))
        .collect();
    let agent = Saved {
        sessions,
        attaches: Arc::default(),
    };
    let mut t = bridge_with_no_roots(&f, &approvals, agent);

    let body = resume_list(&f, &mut t.bridge);

    assert!(body.contains("work in s1"), "{body}");
    for hidden in ["s2", "s3", "s4", "s5", "s6"] {
        assert!(
            !body.contains(&format!("work in {hidden}")),
            "{hidden}: {body}"
        );
    }
}

#[test]
fn picking_a_session_under_no_root_asks_on_the_desktop_and_attaches_after_approve() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let home = real_path(f.state.parent().unwrap()).unwrap();
    fs::create_dir_all(home.join("lighthouse")).unwrap();
    let attaches = Arc::new(AtomicUsize::new(0));
    let agent = Saved {
        sessions: vec![saved_session("s1", &home.join("lighthouse"))],
        attaches: attaches.clone(),
    };
    let mut t = bridge_with_no_roots(&f, &approvals, agent);
    resume_list(&f, &mut t.bridge);

    fs::write(
        f.screenshots.join("WoWScrnShot_3.png"),
        chat_strip("c9", 9, "attach=s1", ""),
    )
    .unwrap();

    let addons = f.addons.clone();
    assert!(step_until(&mut t.bridge, || live_text(&addons)
        .contains(" command folder")));
    assert_eq!(
        attaches.load(Ordering::SeqCst),
        0,
        "no attach before the click"
    );
    let open = approvals.list();
    assert_eq!(open.len(), 1);
    assert!(
        open[0]
            .text
            .starts_with("Let agents from WoW work in ~/lighthouse?"),
        "{}",
        open[0].text
    );
    approvals.answer(&open[0].id, Verdict::Approve).unwrap();
    assert!(step_until(&mut t.bridge, || slot_body(&addons).contains("attached in")));
    assert_eq!(attaches.load(Ordering::SeqCst), 1);
    let config = fs::read_to_string(&t.config).unwrap();
    assert!(
        config.starts_with("allowed_roots = [\"~/lighthouse\"]\ndefault_cwd = \"~\"\n"),
        "{config}"
    );
}

#[test]
fn a_denied_session_folder_attaches_nothing() {
    let f = folders();
    let approvals = Approvals::new(&f.state, Prompt::Off);
    let home = real_path(f.state.parent().unwrap()).unwrap();
    fs::create_dir_all(home.join("lighthouse")).unwrap();
    let attaches = Arc::new(AtomicUsize::new(0));
    let agent = Saved {
        sessions: vec![saved_session("s1", &home.join("lighthouse"))],
        attaches: attaches.clone(),
    };
    let mut t = bridge_with_no_roots(&f, &approvals, agent);
    resume_list(&f, &mut t.bridge);
    let stop = Arc::new(AtomicBool::new(false));
    let answering = answer_raises(&approvals, Verdict::Deny, stop.clone());

    fs::write(
        f.screenshots.join("WoWScrnShot_3.png"),
        chat_strip("c9", 9, "attach=s1", ""),
    )
    .unwrap();

    let addons = f.addons.clone();
    let done = step_until(&mut t.bridge, || {
        slot_body(&addons).contains("Denied on your desktop. Agents can't work in this folder.")
    });
    stop.store(true, Ordering::SeqCst);
    assert!(done, "{}", slot_body(&f.addons));
    assert_eq!(answering.join().unwrap(), 1);
    assert_eq!(attaches.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read_to_string(&t.config).unwrap(), NO_ROOTS_CONFIG);
}

/// stderr of the log, shared with the layer.
#[derive(Clone, Default)]
struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[test]
fn the_log_lines_of_a_message_carry_its_chat_id_agent_and_folder() {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    let f = folders();
    let mut bridge = bridge(&f);
    let stderr = LogBuffer::default();
    let layer = bridge::logging::LogLayer::new(Box::new(stderr.clone()), None);
    let filter = bridge::logging::level_filter(None).0;
    let subscriber = tracing_subscriber::Registry::default().with(layer.with_filter(filter));
    fs::write(
        f.screenshots.join("WoWScrnShot_1.png"),
        strip_png(KEY, "a private prompt"),
    )
    .unwrap();

    let written = tracing::subscriber::with_default(subscriber, || {
        step_until(&mut bridge, || {
            stderr.text().contains("reply c1 #7 written")
        })
    });

    let text = stderr.text();
    assert!(written, "{text}");
    let folder =
        String::from_utf8(path_bytes(&std::env::temp_dir().canonicalize().unwrap())).unwrap();
    let fields = format!(" chat=c1 message_id=7 agent=claude permission=auto-edit folder={folder}");
    for start in ["run c1 #7 ", "done c1 #7", "reply c1 #7 written"] {
        let line = text.lines().find(|l| l.contains(start)).unwrap();
        assert!(line.contains(&fields), "{line}");
    }
    let done = text.lines().find(|l| l.contains("done c1 #7")).unwrap();
    assert!(done.ends_with(" result=reply"), "{done}");
    assert!(!text.contains("a private prompt"), "{text}");
}
