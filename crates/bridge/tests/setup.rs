//! Setup for two products on temp folders (SPEC.md 11.3 and 9.7, decision 15): each
//! setup makes only the files of its own product, and both can live on one computer.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use bridge::app_files::key_addon_name;
use bridge::config::Kind;
use bridge::config_text::RelayPart;
use bridge::install::{ADDON, Installed, KEY_FILE, TIMEWAYS, key_addon_lua};
use bridge::model::ModelChoice;
use bridge::model_setup::FoundModel;
use bridge::receive::{KeySet, RELAY_KEY_FILE, TIMEWAYS_KEY_FILE};
use bridge::setup::{
    self, Changed, ConfigParts, Folders, KeyChoice, Product, install_files, repair_timeways_key,
};
use bridge::slots::slot_name;
use protocol::apps::App;
use protocol::slot::SLOTS;

/// Whether the config of a test has the relay part.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Relay {
    On,
    Off,
}

struct Computer {
    root: tempfile::TempDir,
    folders: Folders,
}

impl Computer {
    /// A home with a code folder, a config folder, and a game with the addons of `with`.
    fn new(with: &[&str]) -> Computer {
        let root = tempfile::tempdir().unwrap();
        let addons = root.path().join("wow/Interface/AddOns");
        fs::create_dir_all(&addons).unwrap();
        fs::create_dir_all(root.path().join("Code")).unwrap();
        for addon in with {
            let dir = addons.join(addon);
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join(format!("{addon}.toc")), "## Title: x\n").unwrap();
        }
        let folders = Folders {
            config: root.path().join("config"),
            addons,
        };
        Computer { root, folders }
    }

    fn home(&self) -> &Path {
        self.root.path()
    }

    fn wow(&self) -> PathBuf {
        self.home().join("wow")
    }

    fn key(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.folders.config.join(file)).ok()
    }

    fn addon_file(&self, addon: &str, file: &str) -> Option<String> {
        fs::read_to_string(self.folders.addons.join(addon).join(file)).ok()
    }

    fn key_addon(&self, app: App) -> Option<String> {
        self.addon_file(key_addon_name(app), KEY_FILE)
    }

    fn has_slots(&self, app: App) -> bool {
        [1, SLOTS]
            .iter()
            .all(|n| self.folders.addons.join(slot_name(app, *n)).is_dir())
    }

    fn any_slot(&self, app: App) -> bool {
        self.folders.addons.join(slot_name(app, 1)).exists()
    }

    /// The config that setup writes with `relay` and the models of `story`.
    fn config(&self, relay: Relay, story: Option<&[FoundModel]>) -> bridge::config::Config {
        let agents = [("claude", Kind::Claude, ["claude"].as_slice())];
        let roots = ["~/Code".to_owned()];
        let wow = self.wow();
        let parts = ConfigParts {
            wow: Some(&wow),
            relay: (relay == Relay::On).then_some(RelayPart {
                agents: &agents,
                harnesses: &[],
                roots: &roots,
                local_ports: &[],
            }),
            new_agents: &[],
            story,
        };
        let text = setup::config_text(None, &parts).unwrap().unwrap();
        setup::write_config(&self.folders.config, &text, self.home()).unwrap()
    }
}

fn install(computer: &Computer, product: Product, keys: KeyChoice) -> Changed {
    install_files(&computer.folders, product, keys).unwrap()
}

#[test]
fn relay_setup_with_a_timeways_folder_installs_only_the_relay() {
    let computer = Computer::new(&[TIMEWAYS]);

    let changed = install(&computer, Product::Relay, KeyChoice::Keep);
    let config = computer.config(Relay::On, None);

    assert_eq!(changed.key_addon, Installed::New);
    assert!(changed.new_slots);
    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    assert_eq!(
        computer.key_addon(App::Relay),
        Some(key_addon_lua(App::Relay, &relay_key).unwrap())
    );
    assert_eq!(computer.addon_file(ADDON, KEY_FILE), None);
    assert!(computer.has_slots(App::Relay));
    assert!(!computer.any_slot(App::Timeways), "no Timeways slots");
    assert_eq!(computer.key_addon(App::Timeways), None);
    assert_eq!(computer.key(TIMEWAYS_KEY_FILE), None);
    assert_eq!(computer.addon_file(TIMEWAYS, KEY_FILE), None);
    assert!(config.relay.is_some());
    assert!(config.story.is_none());
}

#[test]
fn a_second_setup_of_each_product_changes_nothing() {
    let computer = Computer::new(&[TIMEWAYS]);
    let unchanged = Changed {
        key_addon: Installed::Unchanged,
        new_slots: false,
    };
    for product in [Product::Relay, Product::Timeways] {
        install(&computer, product, KeyChoice::Keep);

        let again = install(&computer, product, KeyChoice::Keep);

        assert_eq!(again, unchanged, "{product:?}");
    }
}

#[test]
fn both_setups_on_one_computer_give_two_keys_and_both_products() {
    let computer = Computer::new(&[TIMEWAYS]);

    install(&computer, Product::Relay, KeyChoice::Keep);
    let changed = install(&computer, Product::Timeways, KeyChoice::Keep);
    let config = computer.config(Relay::On, Some(&[FoundModel::Claude]));

    assert_eq!(changed.key_addon, Installed::New);
    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    let timeways_key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_ne!(relay_key, timeways_key);
    assert_eq!(timeways_key.len(), 64);
    assert_eq!(
        computer.key_addon(App::Timeways),
        Some(key_addon_lua(App::Timeways, &timeways_key).unwrap())
    );
    assert_eq!(
        computer.key_addon(App::Relay),
        Some(key_addon_lua(App::Relay, &relay_key).unwrap())
    );
    assert!(computer.has_slots(App::Relay));
    assert!(computer.has_slots(App::Timeways));
    assert!(
        KeySet::load(&computer.folders.config)
            .unwrap()
            .has_timeways()
    );
    assert!(config.relay.is_some());
    assert!(matches!(
        config.story.unwrap().model.choice,
        ModelChoice::Claude { .. }
    ));
}

#[test]
fn timeways_setup_installs_no_relay_and_no_agent() {
    let computer = Computer::new(&[TIMEWAYS]);

    let changed = install(&computer, Product::Timeways, KeyChoice::Keep);
    let config = computer.config(Relay::Off, Some(&[]));

    assert_eq!(changed.key_addon, Installed::New);
    assert!(changed.new_slots);
    assert!(!computer.folders.addons.join(ADDON).exists());
    assert_eq!(computer.key_addon(App::Relay), None);
    assert!(!computer.any_slot(App::Relay));
    assert!(computer.has_slots(App::Timeways));
    // The bridge needs the relay key to start, so it exists with no addon.
    assert!(computer.key(RELAY_KEY_FILE).is_some());
    assert!(config.relay.is_none());
    assert_eq!(config.story.unwrap().model.choice, ModelChoice::None);
}

#[test]
fn the_timeways_slots_depend_on_timeways() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Product::Timeways, KeyChoice::Keep);
    let name = slot_name(App::Timeways, 1);
    let toc = computer.addon_file(&name, &format!("{name}.toc")).unwrap();
    assert!(toc.contains("## Dependencies: Timeways\n"), "{toc}");
}

#[test]
fn a_new_key_replaces_only_the_key_and_the_key_addon_of_its_product() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Product::Relay, KeyChoice::Keep);
    install(&computer, Product::Timeways, KeyChoice::Keep);
    let old_relay = computer.key(RELAY_KEY_FILE).unwrap();
    let old_timeways = computer.key(TIMEWAYS_KEY_FILE).unwrap();

    let relay = install(&computer, Product::Relay, KeyChoice::New);

    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    assert_ne!(relay_key, old_relay);
    assert_eq!(computer.key(TIMEWAYS_KEY_FILE).unwrap(), old_timeways);
    assert_eq!(relay.key_addon, Installed::Updated);
    assert_eq!(
        computer.key_addon(App::Relay),
        Some(key_addon_lua(App::Relay, &relay_key).unwrap())
    );

    let timeways = install(&computer, Product::Timeways, KeyChoice::New);

    let timeways_key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_ne!(timeways_key, old_timeways);
    assert_ne!(timeways_key, relay_key);
    assert_eq!(computer.key(RELAY_KEY_FILE).unwrap(), relay_key);
    assert_eq!(timeways.key_addon, Installed::Updated);
    assert_eq!(
        computer.key_addon(App::Timeways),
        Some(key_addon_lua(App::Timeways, &timeways_key).unwrap())
    );
}

#[cfg(unix)]
#[test]
fn the_keys_are_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Product::Timeways, KeyChoice::Keep);
    for file in [RELAY_KEY_FILE, TIMEWAYS_KEY_FILE] {
        let mode = fs::metadata(computer.folders.config.join(file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{file}");
    }
}

#[test]
fn equal_keys_stop_setup() {
    let computer = Computer::new(&[TIMEWAYS]);
    fs::create_dir_all(&computer.folders.config).unwrap();
    let key = "ab".repeat(32);
    fs::write(computer.folders.config.join(RELAY_KEY_FILE), &key).unwrap();
    fs::write(computer.folders.config.join(TIMEWAYS_KEY_FILE), &key).unwrap();

    for product in [Product::Relay, Product::Timeways] {
        let error = install_files(&computer.folders, product, KeyChoice::Keep).unwrap_err();

        assert!(format!("{error:#}").contains("the same"), "{error:#}");
    }
}

#[test]
fn the_bridge_writes_a_missing_timeways_key_again_and_no_file_of_timeways() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Product::Timeways, KeyChoice::Keep);
    let dir = computer.folders.addons.join(TIMEWAYS);
    fs::remove_file(computer.folders.addons.join("Timeways_Key").join(KEY_FILE)).unwrap();
    fs::remove_file(dir.join("Timeways.toc")).unwrap();

    let repaired = repair_timeways_key(&computer.folders.config, &computer.folders.addons);

    assert_eq!(repaired.unwrap(), Some(Installed::Updated));
    let key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_eq!(
        computer.key_addon(App::Timeways),
        Some(key_addon_lua(App::Timeways, &key).unwrap())
    );
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        0,
        "the bridge never writes the TOC"
    );
}

#[test]
fn setup_for_timeways_with_no_timeways_folder_writes_its_key_its_key_addon_and_its_slots() {
    let computer = Computer::new(&[]);

    let changed = install(&computer, Product::Timeways, KeyChoice::Keep);

    assert_eq!(changed.key_addon, Installed::New);
    assert!(changed.new_slots);
    let key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_eq!(
        computer.key_addon(App::Timeways),
        Some(key_addon_lua(App::Timeways, &key).unwrap())
    );
    assert!(computer.has_slots(App::Timeways));
    assert!(
        !computer.folders.addons.join(TIMEWAYS).exists(),
        "setup never makes the Timeways folder"
    );
}

#[test]
fn the_bridge_writes_a_missing_timeways_key_addon_also_with_no_timeways_folder() {
    let computer = Computer::new(&[]);
    install(&computer, Product::Timeways, KeyChoice::Keep);
    fs::remove_file(computer.folders.addons.join("Timeways_Key").join(KEY_FILE)).unwrap();

    let repaired = repair_timeways_key(&computer.folders.config, &computer.folders.addons);

    assert_eq!(repaired.unwrap(), Some(Installed::Updated));
    let key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_eq!(
        computer.key_addon(App::Timeways),
        Some(key_addon_lua(App::Timeways, &key).unwrap())
    );
    assert!(!computer.folders.addons.join(TIMEWAYS).exists());
}

#[test]
fn with_no_timeways_key_the_start_of_the_desktop_app_writes_nothing_for_timeways() {
    let computer = Computer::new(&[]);
    install(&computer, Product::Relay, KeyChoice::Keep);
    let config = &computer.folders.config;
    assert_eq!(
        repair_timeways_key(config, &computer.folders.addons).unwrap(),
        None
    );
    fs::create_dir(computer.folders.addons.join(TIMEWAYS)).unwrap();
    assert_eq!(
        repair_timeways_key(config, &computer.folders.addons).unwrap(),
        None,
        "the bridge never makes a key"
    );
    assert!(computer.addon_file(TIMEWAYS, KEY_FILE).is_none());
    assert!(computer.key_addon(App::Timeways).is_none());
}

#[cfg(unix)]
#[test]
fn a_linked_timeways_folder_stays_a_link_and_its_key_goes_into_the_key_addon() {
    let computer = Computer::new(&[]);
    let repo = computer.home().join("timeways-repo");
    fs::create_dir(&repo).unwrap();
    std::os::unix::fs::symlink(&repo, computer.folders.addons.join(TIMEWAYS)).unwrap();

    install(&computer, Product::Timeways, KeyChoice::Keep);

    assert_eq!(fs::read_dir(&repo).unwrap().count(), 0);
    assert!(computer.key_addon(App::Timeways).is_some());
    let link = fs::symlink_metadata(computer.folders.addons.join(TIMEWAYS)).unwrap();
    assert!(link.file_type().is_symlink());
}

#[test]
fn install_makes_the_slots_of_each_product_of_the_computer_and_not_of_a_folder() {
    let computer = Computer::new(&[TIMEWAYS]);
    let relay = computer.config(Relay::On, None);
    assert_eq!(
        setup::products_of(&relay, &computer.folders.config),
        [Product::Relay],
        "a Timeways folder alone is no Timeways"
    );
    install(&computer, Product::Timeways, KeyChoice::Keep);
    let both = setup::products_of(&relay, &computer.folders.config);
    assert_eq!(both, [Product::Relay, Product::Timeways]);
    let timeways = computer.config(Relay::Off, Some(&[]));
    let alone = setup::products_of(&timeways, &computer.folders.config);
    assert_eq!(alone, [Product::Timeways]);
    fs::remove_dir_all(computer.folders.addons.join(slot_name(App::Timeways, 1))).unwrap();

    setup::install_all_slots(&computer.folders.addons, &alone).unwrap();

    assert!(computer.has_slots(App::Timeways));
    assert!(!computer.any_slot(App::Relay));
}

#[test]
fn a_relay_config_gets_a_story_section_once_timeways_is_installed() {
    let computer = Computer::new(&[]);
    computer.config(Relay::On, None);
    let old = fs::read_to_string(computer.folders.config.join("config.toml")).unwrap();
    let parts = ConfigParts {
        wow: None,
        relay: None,
        new_agents: &[],
        story: Some(&[FoundModel::Claude]),
    };

    let text = setup::config_text(Some(&old), &parts).unwrap().unwrap();
    let config = setup::write_config(&computer.folders.config, &text, computer.home()).unwrap();

    assert!(text.starts_with(&old), "every old key stays");
    assert!(config.relay.is_some());
    assert!(config.story.is_some());
}

#[test]
fn a_config_that_does_not_load_is_never_written() {
    let computer = Computer::new(&[]);
    fs::create_dir_all(&computer.folders.config).unwrap();
    let bad = "default_agent = \"echo\"\n[wow]\npath = \"~/wow\"\n";
    assert!(setup::write_config(&computer.folders.config, bad, computer.home()).is_err());
    assert!(!computer.folders.config.join("config.toml").exists());
}

#[test]
fn setup_makes_no_relay_addon_folder() {
    let computer = Computer::new(&[]);

    install(&computer, Product::Relay, KeyChoice::Keep);

    assert!(!computer.folders.addons.join(ADDON).exists());
    assert!(computer.key_addon(App::Relay).is_some());
}

#[test]
fn setup_leaves_the_relay_addon_from_curseforge_as_it_is() {
    let computer = Computer::new(&[ADDON]);
    let dir = computer.folders.addons.join(ADDON);

    install(&computer, Product::Relay, KeyChoice::Keep);
    install(&computer, Product::Relay, KeyChoice::New);

    assert_eq!(
        computer.addon_file(ADDON, "GnomishRelay.toc").as_deref(),
        Some("## Title: x\n")
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn a_second_setup_adds_a_new_agent_and_keeps_the_default() {
    let computer = Computer::new(&[]);
    let roots = ["~/Code".to_owned()];
    let first = RelayPart {
        agents: &[],
        harnesses: &[],
        roots: &roots,
        local_ports: &[],
    };
    let old = bridge::config_text::relay_config(Some(&computer.wow()), &first);
    let config = setup::write_config(&computer.folders.config, &old, computer.home()).unwrap();
    let found = [
        ("claude", Kind::Claude, ["claude"].as_slice()),
        ("codex", Kind::Codex, ["codex"].as_slice()),
    ];
    let new = setup::new_agents(&found, Some(&config));
    let parts = ConfigParts {
        wow: None,
        relay: None,
        new_agents: &new,
        story: None,
    };

    let text = setup::config_text(Some(&old), &parts).unwrap().unwrap();
    let config = setup::write_config(&computer.folders.config, &text, computer.home()).unwrap();

    assert!(text.starts_with(&old), "every old key stays");
    let relay = config.relay.unwrap();
    assert_eq!(relay.policy.default_agent, "echo");
    let names: Vec<&str> = relay.agents.keys().map(String::as_str).collect();
    assert_eq!(names, ["claude", "codex", "echo"]);
}

#[test]
fn only_agents_that_the_config_lacks_are_new() {
    let computer = Computer::new(&[]);
    let config = computer.config(Relay::On, None);
    let found = [
        ("claude", Kind::Claude, ["claude"].as_slice()),
        ("codex", Kind::Codex, ["codex"].as_slice()),
    ];

    let new = setup::new_agents(&found, Some(&config));

    let names: Vec<&str> = new.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(names, ["codex"]);
    assert!(setup::new_agents(&found, None).is_empty());
}
