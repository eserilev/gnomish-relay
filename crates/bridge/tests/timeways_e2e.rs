//! The real bridge with the real story program of Timeways, and no game (SPEC.md 9.7 and
//! 9.8). The test addon of `shared_transport.rs` sends batches that the batch code of the
//! Timeways addon (`Json.lua`, `Inputs.lua`, `Outbox.lua`) makes. `scripts/e2e-timeways.sh`
//! builds `timeways-story` and `timeways-pack` from a Timeways checkout and runs these
//! tests. With no Timeways checkout they skip, so `cargo test` and CI stay green.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bridge::config::Policy;
use bridge::receive::{KeySet, StripKey};
use bridge::relay::Folders;
use bridge::run::{Bridge, Paths, now};
use bridge::slots::{self, BODY_FILE, Files, LIVE_FILE, RESTORE_FILE};
use bridge::story::{STORY_DIR, StorySpec};
use bridge::story_sandbox::Sandbox;
use common::{Bits, hex, load_addon, lua, repo_file, screenshot_png};
use mlua::{Function, Lua, Table};
use protocol::apps::App;
use serde_json::Value as Json;

const RELAY_KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
const TIMEWAYS_KEY: &[u8] = b"fedcba9876543210fedcba9876543210";
const STRIP: &str = "TimewaysStrip";

/// The binaries and the addon folder of a Timeways checkout.
const STORY_VAR: &str = "TIMEWAYS_STORY";
const PACK_VAR: &str = "TIMEWAYS_PACK";
const ADDON_VAR: &str = "TIMEWAYS_ADDON";

const SHARED: &[&str] = &[
    "Sha256.lua",
    "Codec.lua",
    "Saved.lua",
    "Health.lua",
    "Strip.lua",
    "Slots.lua",
    "Messages.lua",
];
/// The batch code of the Timeways addon, from its checkout.
const BATCH_FILES: &[&str] = &["Json.lua", "Inputs.lua", "Outbox.lua"];

/// The App.lua of the test addon: the Timeways names of SPEC.md 9.7, decision 5.
const TIMEWAYS_APP: &str = r#"
local _, ns = ...
ns.App = {
	title = "Timeways",
	version = 1,
	helloChat = "story",
	slotPrefix = "Timeways_S%04d",
	slotData = "Timeways_SlotData",
	restore = "Timeways_Restore",
	live = "Timeways_Live",
	strip = "TimewaysStrip",
	saved = "TimewaysDB",
}
"#;
/// The seam `ns.Link` of the Timeways addon, as in `shared_transport.rs`. It keeps each
/// final reply in `ns.replies`.
const TIMEWAYS_LINK: &str = r#"
local _, ns = ...
local CHAT = { id = "story" }
ns.replies = {}
ns.Link = {
	Fits = function(text)
		return ns.Messages.Fits(CHAT, text)
	end,
	Send = function(text)
		return ns.Messages.Send(CHAT, text) ~= nil
	end,
}
ns.Messages.OnReply = function(_, _, status, text)
	table.insert(ns.replies, status .. ": " .. text)
end
ns.Messages.Init()
C_Timer.NewTicker(1, ns.Messages.Tick)
"#;

/// Invented lore. The Westfall passage links a place that the character never visits
/// before the question, so the spoiler limit keeps it out of the answer.
const PASSAGES: &str = r#"{"text": "Hogger is a huge gnoll who leads the Riverpaw raiders in the west of Elwynn Forest.", "source": "https://example.test/lore/hogger", "places": ["Elwynn Forest"], "npcs": ["Hogger"]}
{"text": "Marshal Dughan keeps the peace in Goldshire and pays a bounty for Riverpaw gnoll paws.", "source": "https://example.test/lore/dughan", "places": ["Goldshire"], "npcs": ["Marshal Dughan"]}
{"text": "Riverpaw gnolls from Westfall hide in the hills of Hogger's camp.", "source": "https://example.test/lore/westfall", "places": ["Westfall"]}
"#;

/// The first batch: the character, four kinds of game events, and a question last.
const FIRST_BATCH: &str = r#"local ns = ...
ns.Outbox.SetCharacter(ns.Inputs.Character("Stormrage", "Anduin"))
ns.Outbox.Add(ns.Inputs.Zone(time(), "Elwynn Forest", "Goldshire"))
ns.Outbox.Add(ns.Inputs.Npc(time(), "Marshal Dughan"))
ns.Outbox.Add(ns.Inputs.Npc(time(), "Hogger"))
ns.Outbox.Add(ns.Inputs.Level(time(), 10))
ns.Outbox.Add(ns.Inputs.Defeated(time(), "Hogger"))
ns.Outbox.Add(ns.Inputs.Question(time(), "Who is Hogger?", "Hogger"))
ns.Outbox.Flush()"#;
const JOURNAL_BATCH: &str = r"local ns = ...
ns.Outbox.Add(ns.Inputs.JournalAsked(0))
ns.Outbox.Flush()";
const TALK_BATCH: &str = r#"local ns = ...
ns.Outbox.Add(ns.Inputs.Talk(time(), "Marshal Dughan", "Any news of the gnolls?"))
ns.Outbox.Flush()"#;
const EVENTS_BATCH: &str = r#"local ns = ...
ns.Outbox.Add(ns.Inputs.Zone(time(), "Westfall", "Sentinel Hill"))
ns.Outbox.Add(ns.Inputs.Npc(time(), "Gryan Stoutmantle"))
ns.Outbox.Add(ns.Inputs.Level(time(), 11))
ns.Outbox.Flush()"#;

/// Where the Timeways checkout is, from the environment.
struct Timeways {
    story: PathBuf,
    pack: PathBuf,
    addon: PathBuf,
}

impl Timeways {
    /// `None` with a line that says why: the test then skips.
    fn from_env() -> Option<Timeways> {
        let var = |name| std::env::var_os(name).map(PathBuf::from);
        let (Some(story), Some(pack), Some(addon)) =
            (var(STORY_VAR), var(PACK_VAR), var(ADDON_VAR))
        else {
            eprintln!(
                "skipped: set {STORY_VAR}, {PACK_VAR}, and {ADDON_VAR}, or run scripts/e2e-timeways.sh"
            );
            return None;
        };
        Some(Timeways { story, pack, addon })
    }
}

/// A home, a game, and the folders of the bridge. It is not under `/tmp`, because the
/// sandbox hides all of `/tmp`.
struct Machine {
    root: tempfile::TempDir,
    config: PathBuf,
    data: PathBuf,
    pack: PathBuf,
    game: Paths,
}

impl Machine {
    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn story_folder(&self) -> PathBuf {
        self.data.join("timeways").join(STORY_DIR)
    }

    fn slot_file(&self, app: App, slot: usize, file: &str) -> PathBuf {
        self.game
            .addons
            .join(slots::slot_name(app, slot))
            .join(file)
    }
}

fn machine() -> Machine {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = root.path().join("home");
    let wow = root.path().join("wow");
    let config = home.join(".config/gnomish-relay");
    let data = home.join(".local/share/gnomish-relay");
    let game = Paths {
        addons: wow.join("Interface/AddOns"),
        screenshots: wow.join("Screenshots"),
        accounts: wow.join("WTF/Account"),
        state: data.clone(),
    };
    for dir in [&config, &game.addons, &game.screenshots, &game.accounts] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for app in [App::Relay, App::Timeways] {
        slots::install(&game.addons, app, &Files::empty(app, 0)).unwrap();
    }
    Machine {
        pack: home.join(".local/share/timeways/lore.sqlite"),
        root,
        config,
        data,
        game,
    }
}

/// Writes the invented passages and builds the pack with the real `timeways-pack`.
fn build_pack(timeways: &Timeways, machine: &Machine) {
    let passages = machine.root.path().join("passages.jsonl");
    std::fs::write(&passages, PASSAGES).unwrap();
    std::fs::create_dir_all(machine.pack.parent().unwrap()).unwrap();
    let out = std::process::Command::new(&timeways.pack)
        .arg(&passages)
        .arg(&machine.pack)
        .output()
        .unwrap();
    assert!(out.status.success(), "timeways-pack: {out:?}");
}

/// The `[story]` section of the config, loaded the way the bridge loads it at start.
fn story_spec(timeways: &Timeways, machine: &Machine, model: &str) -> StorySpec {
    let text = format!(
        "[wow]\npath = '{}'\n\n[story]\nprogram = '{}'\nlore_pack = '{}'\ntimeout_seconds = 180\n{model}",
        machine.root.path().join("wow").display(),
        timeways.story.display(),
        machine.pack.display(),
    );
    let config = bridge::config::parse(&text, &machine.home()).unwrap();
    let story = config.story.expect("a [story] section");
    StorySpec::from_config(&story, &machine.config, &machine.data, &machine.home())
        .unwrap()
        .expect("a story program")
}

fn key(bytes: &[u8]) -> StripKey {
    StripKey::from_hex(&hex(bytes)).unwrap()
}

/// A relay lane too, so the test can show that no Timeways batch reaches it.
fn start_bridge(machine: &Machine, spec: StorySpec) -> Bridge {
    let paths = Paths {
        addons: machine.game.addons.clone(),
        screenshots: machine.game.screenshots.clone(),
        accounts: machine.game.accounts.clone(),
        state: machine.game.state.clone(),
    };
    let keys = KeySet::new(key(RELAY_KEY), Some(key(TIMEWAYS_KEY))).unwrap();
    let home = machine.home().to_string_lossy().as_bytes().to_vec();
    let policy = Policy {
        folders: Folders {
            roots: vec![home.clone()],
            base: home,
        },
        agents: std::collections::BTreeMap::new(),
        default_agent: "claude".into(),
    };
    Bridge::new(paths, policy, keys, std::collections::BTreeMap::new())
        .unwrap()
        .with_story(spec)
}

/// The fake game with the test addon and the batch code of the Timeways addon.
struct Game {
    lua: Lua,
    wow: Table,
    ns: Table,
}

fn start_game(timeways: &Timeways) -> Game {
    let lua = lua(Bits::Unsigned);
    let api: Table = lua.load(repo_file("addon/tests/api.lua")).call(()).unwrap();
    let wow: Table = lua
        .load(repo_file("addon/tests/wow.lua"))
        .call(api)
        .unwrap();
    wow.set("strips", lua.create_sequence_from([STRIP]).unwrap())
        .unwrap();
    wow.set("epoch", now()).unwrap();
    let ns = lua.create_table().unwrap();
    ns.set("key", lua.create_string(TIMEWAYS_KEY).unwrap())
        .unwrap();
    lua.load(TIMEWAYS_APP)
        .call::<()>(("Timeways", ns.clone()))
        .unwrap();
    load_addon(&lua, "Timeways", &ns, SHARED);
    lua.load(TIMEWAYS_LINK)
        .call::<()>(("Timeways", ns.clone()))
        .unwrap();
    for file in BATCH_FILES {
        let text = std::fs::read_to_string(timeways.addon.join(file)).unwrap();
        lua.load(text)
            .set_name(*file)
            .call::<()>(("Timeways", ns.clone()))
            .unwrap();
    }
    Game { lua, wow, ns }
}

impl Game {
    fn run(&self, code: &str) {
        let batch: Function = self.lua.load(code).into_function().unwrap();
        batch.call::<()>(self.ns.clone()).unwrap();
    }

    fn advance(&self, seconds: f64) {
        let advance: Function = self.wow.get("Advance").unwrap();
        advance.call::<()>(seconds).unwrap();
    }

    fn replies(&self) -> Vec<String> {
        self.ns.get("replies").unwrap()
    }

    fn shots(&self) -> Vec<Vec<Vec<u8>>> {
        let shots: Option<Table> = self
            .wow
            .get::<Table>("shotsOf")
            .unwrap()
            .get(STRIP)
            .unwrap();
        let Some(shots) = shots else {
            return Vec::new();
        };
        shots
            .sequence_values::<Vec<Vec<u8>>>()
            .map(Result::unwrap)
            .collect()
    }

    fn next_slot(&self) -> usize {
        let messages: Table = self.ns.get("Messages").unwrap();
        let stats: Table = messages.get::<Function>("Stats").unwrap().call(()).unwrap();
        stats.get("nextSlot").unwrap()
    }

    /// Puts the files of one slot on disk where the fake `LoadAddOn` reads them.
    fn put_slot_files(&self, machine: &Machine, slot: usize) {
        let files = self.lua.create_table().unwrap();
        for (key, name) in [
            ("body", BODY_FILE),
            ("restore", RESTORE_FILE),
            ("live", LIVE_FILE),
        ] {
            let Ok(bytes) = std::fs::read(machine.slot_file(App::Timeways, slot, name)) else {
                return;
            };
            files
                .set(key, self.lua.create_string(bytes).unwrap())
                .unwrap();
        }
        let all: Table = self.wow.get("files").unwrap();
        all.set("Timeways", files).unwrap();
    }
}

/// The game, the bridge, and the story program, with the clock of the game on real time.
struct World {
    machine: Machine,
    game: Game,
    bridge: Bridge,
    shots_sent: usize,
    last_tick: Instant,
}

impl World {
    /// Moves screenshots to the bridge and slot files to the game until `count` replies
    /// came, then returns the last one as JSON.
    fn reply(&mut self, count: usize, limit: Duration) -> Json {
        let start = Instant::now();
        while self.game.replies().len() < count {
            assert!(
                start.elapsed() < limit,
                "no reply {count} in {limit:?}; replies: {:?}",
                self.game.replies()
            );
            self.tick();
        }
        let last = self.game.replies().pop().unwrap();
        let json = last
            .strip_prefix("done: ")
            .unwrap_or_else(|| panic!("not a done reply: {last}"));
        serde_json::from_str(json).unwrap_or_else(|_| panic!("not JSON: {json}"))
    }

    fn tick(&mut self) {
        std::thread::sleep(Duration::from_millis(25));
        self.game.advance(self.last_tick.elapsed().as_secs_f64());
        self.last_tick = Instant::now();
        self.send_new_shots();
        self.bridge.step();
        self.game
            .put_slot_files(&self.machine, self.game.next_slot());
    }

    fn send_new_shots(&mut self) {
        let shots = self.game.shots();
        for rows in &shots[self.shots_sent..] {
            self.shots_sent += 1;
            let name = format!("WoWScrnShot_{}.png", self.shots_sent);
            std::fs::write(
                self.machine.game.screenshots.join(name),
                screenshot_png(rows),
            )
            .unwrap();
        }
    }
}

/// Every file below `dir`, as paths relative to it.
fn files_below(dir: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(folder) = open.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                open.push(path);
            } else {
                found.insert(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    found
}

/// The four replies of one run, and the machine to check afterwards.
struct Outcome {
    lore: Json,
    journal: Json,
    talk: Json,
    events: Json,
    sandbox: Sandbox,
    files_before: BTreeSet<PathBuf>,
    machine: Machine,
}

fn run_all_batches(timeways: &Timeways, model: &str, limit: Duration) -> Outcome {
    let machine = machine();
    build_pack(timeways, &machine);
    let spec = story_spec(timeways, &machine, model);
    let sandbox = spec.sandbox.clone();
    eprintln!("story sandbox: {}", sandbox.name());
    let files_before = files_below(machine.root.path());
    let bridge = start_bridge(&machine, spec);
    let game = start_game(timeways);
    let mut world = World {
        machine,
        game,
        bridge,
        shots_sent: 0,
        last_tick: Instant::now(),
    };
    world.game.run(FIRST_BATCH);
    let lore = world.reply(1, limit);
    world.game.run(JOURNAL_BATCH);
    let journal = world.reply(2, limit);
    world.game.run(TALK_BATCH);
    let talk = world.reply(3, limit);
    world.game.run(EVENTS_BATCH);
    let events = world.reply(4, limit);
    for reply in [&lore, &journal, &talk, &events] {
        eprintln!("reply: {reply}");
    }
    Outcome {
        lore,
        journal,
        talk,
        events,
        sandbox,
        files_before,
        machine: world.machine,
    }
}

fn names_in(list: &Json, key: &str) -> Vec<String> {
    let Some(items) = list.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item[key].as_str().map(str::to_owned))
        .collect()
}

fn has_level_deed(journal: &Json, level: i64) -> bool {
    let Some(deeds) = journal["deeds"].as_array() else {
        return false;
    };
    deeds
        .iter()
        .any(|deed| deed["kind"] == "level" && deed["to"] == level)
}

fn assert_sandbox_note_only_without_sandbox(outcome: &Outcome) {
    let has_note = outcome.lore.get("note").is_some();
    assert_eq!(
        has_note,
        outcome.sandbox == Sandbox::None,
        "{}",
        outcome.lore
    );
}

/// The files that the bridge writes outside the game folder.
const BRIDGE_FILES: &[&str] = &["state.json", "timeways/state.json"];

fn is_expected_file(machine: &Machine, path: &Path) -> bool {
    let bridge_file = BRIDGE_FILES.iter().any(|f| path == machine.data.join(f));
    bridge_file
        || path.starts_with(machine.root.path().join("wow"))
        || path.starts_with(machine.story_folder())
}

/// New files outside the game folder, the story folder, and the files of the bridge.
fn stray_files(outcome: &Outcome) -> Vec<PathBuf> {
    let machine = &outcome.machine;
    let root = machine.root.path();
    files_below(root)
        .difference(&outcome.files_before)
        .map(|path| root.join(path))
        .filter(|path| !is_expected_file(machine, path))
        .collect()
}

/// The relay slots and the relay state hold nothing of the Timeways batches.
fn relay_files_mention(machine: &Machine, word: &str) -> bool {
    let body = std::fs::read(machine.slot_file(App::Relay, 1, BODY_FILE)).unwrap();
    let state = std::fs::read(machine.data.join("state.json")).unwrap_or_default();
    let has = |bytes: &[u8]| String::from_utf8_lossy(bytes).contains(word);
    has(&body) || has(&state)
}

#[test]
#[ignore = "needs a Timeways checkout: run scripts/e2e-timeways.sh"]
fn the_real_story_program_answers_real_batches_through_the_real_bridge_with_no_model() {
    let Some(timeways) = Timeways::from_env() else {
        return;
    };

    let outcome = run_all_batches(&timeways, "", Duration::from_mins(1));

    let lore = &outcome.lore;
    assert_eq!(lore["type"], "lore_answer");
    assert!(lore["text"].is_null(), "no model, so no model text: {lore}");
    let sources = names_in(&lore["passages"], "source");
    assert!(
        sources.contains(&"https://example.test/lore/hogger".into()),
        "{lore}"
    );
    assert!(
        !sources.contains(&"https://example.test/lore/westfall".into()),
        "{lore}"
    );
    assert!(
        names_in(&lore["passages"], "text")[0].contains("Hogger"),
        "{lore}"
    );
    assert_sandbox_note_only_without_sandbox(&outcome);

    let journal = &outcome.journal;
    assert_eq!(journal["type"], "journal");
    assert!(
        names_in(&journal["places"], "name").contains(&"Elwynn Forest".into()),
        "{journal}"
    );
    assert!(
        names_in(&journal["people"], "name").contains(&"Marshal Dughan".into()),
        "{journal}"
    );
    assert!(has_level_deed(journal, 10), "{journal}");

    let talk = &outcome.talk;
    assert_eq!(talk["type"], "talk_answer");
    assert_eq!(talk["npc"], "Marshal Dughan");
    assert!(talk["text"].is_null(), "no model, so no words: {talk}");

    let events = &outcome.events;
    assert_eq!(events["type"], "events_seen");
    assert!(events["companion"].is_null(), "{events}");

    let worlds = files_below(&outcome.machine.story_folder());
    assert!(worlds.iter().any(|p| p.starts_with("worlds")), "{worlds:?}");
    assert_eq!(stray_files(&outcome), Vec::<PathBuf>::new());
    for word in ["Anduin", "Hogger", "Dughan"] {
        assert!(
            !relay_files_mention(&outcome.machine, word),
            "{word} reached the relay"
        );
    }
}

#[test]
#[ignore = "needs a Timeways checkout and `claude` on PATH: run scripts/e2e-timeways.sh --claude"]
fn the_real_story_program_gets_words_from_claude_through_the_real_bridge() {
    let Some(timeways) = Timeways::from_env() else {
        return;
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    if bridge::program::find_program("claude", &path, false).is_none() {
        eprintln!("skipped: no claude on PATH");
        return;
    }

    let model = "model = \"claude\"\nclaude_model = \"haiku\"\nmodel_timeout_seconds = 120\n";
    let outcome = run_all_batches(&timeways, model, Duration::from_mins(5));

    let words = [
        &outcome.lore["text"],
        &outcome.talk["text"],
        &outcome.events["companion"],
    ];
    assert!(
        words.iter().any(|text| text.is_string()),
        "no model words came back: {words:?}"
    );
}
