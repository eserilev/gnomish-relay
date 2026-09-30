//! The bridge on real folders: a screenshot in, the echo agent, a slot file out.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bridge::acp::AcpAgent;
use bridge::activity::text_hash;
use bridge::agent::{Agent, Control, Echo, Run};
use bridge::config::{Permission, Policy, path_bytes};
use bridge::desktop::{Approvals, Prompt, Verdict};
use bridge::gate::Gate;
use bridge::raise::Raiser;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::relay::Job;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{BODY_FILE, LIVE_FILE, slot_name};
use common::{hex, install_window, screenshot_png, signed_frame, strip_rows};
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
    step_while(bridge, Duration::from_secs(30), done)
}

fn step_while(bridge: &mut Bridge, limit: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        bridge.step();
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
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
fn a_normal_screenshot_and_a_strip_with_a_bad_tag_stay_untouched() {
    let f = folders();
    let mut bridge = bridge(&f);
    let user = f.screenshots.join("WoWScrnShot_user.png");
    let forged = f.screenshots.join("WoWScrnShot_forged.png");
    fs::write(&user, screenshot_png(&[])).unwrap();
    fs::write(
        &forged,
        strip_png(b"another key, 32 bytes long......", "rm -rf ~"),
    )
    .unwrap();
    // The bridge scans every file of the folder in one step, so this echo comes after it
    // looked at the other two.
    let valid = f.screenshots.join("WoWScrnShot_valid.png");
    fs::write(&valid, strip_png(KEY, "marker")).unwrap();

    assert!(step_until(&mut bridge, || slot_body(&f.addons)
        .contains("echo: marker")));
    assert!(user.exists());
    assert!(forged.exists());
    assert!(!slot_body(&f.addons).contains("rm -rf"));
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
        gate: Gate {
            roots: vec![root.path().canonicalize().unwrap()],
            config_dir: f.state.join("config"),
            data_dir: f.state.clone(),
            allow: Arc::default(),
            approvals: Approvals::new(&f.state, Prompt::Off),
            sandbox: bridge::command_sandbox::CommandSandbox::none(),
            wall: bridge::agent_wall::AgentWall::none(),
            always: bridge::always_rules::AlwaysRules::none(),
            home: std::env::temp_dir(),
        },
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

/// An agent with saved sessions, or one whose list fails.
struct Sessions(Result<Vec<bridge::agent::SessionInfo>, String>);

impl Agent for Sessions {
    fn run(&self, _job: &Job, _control: &Control) -> Run {
        Run {
            reply: Ok(String::new()),
            session: None,
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
    let found = vec![bridge::agent::SessionInfo {
        id: "a1".into(),
        cwd: std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join("app")
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
        slot_body(&addons).contains("outside the allowed roots")
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
    assert!(step_until(&mut bridge, || slot_body(&addons)
        .contains("Folder not made: its parent is missing.")));

    assert!(!root.join("none").exists());
    assert!(!slot_body(&f.addons).contains("echo: hello"));
}
