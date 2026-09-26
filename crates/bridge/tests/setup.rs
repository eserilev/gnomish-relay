//! Setup for two apps on temp folders (SPEC.md 11.3 and 9.7, decision 15): the relay
//! alone as before, the relay and Timeways, and Timeways alone.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use bridge::config::Kind;
use bridge::config_text::RelayPart;
use bridge::install::{self, ADDON, Installed, KEY_FILE, TIMEWAYS, key_lua};
use bridge::model::ModelChoice;
use bridge::model_setup::FoundModel;
use bridge::receive::{KeySet, RELAY_KEY_FILE, TIMEWAYS_KEY_FILE};
use bridge::setup::{
    self, Changed, ConfigParts, Folders, KeyChoice, Relay, install_files, repair_timeways_key,
};
use bridge::slots::slot_name;
use protocol::apps::App;
use protocol::slot::SLOTS;

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
            wow: &wow,
            relay: (relay == Relay::On).then_some(RelayPart {
                agents: &agents,
                roots: &roots,
            }),
            story,
        };
        let text = setup::config_text(None, &parts).unwrap();
        setup::write_config(&self.folders.config, &text, self.home()).unwrap()
    }
}

fn install(computer: &Computer, relay: Relay, keys: KeyChoice) -> Changed {
    install_files(&computer.folders, relay, keys).unwrap()
}

#[test]
fn with_no_timeways_folder_setup_installs_the_relay_as_before() {
    let computer = Computer::new(&[]);

    let changed = install(&computer, Relay::On, KeyChoice::Keep);
    let config = computer.config(Relay::On, None);

    assert_eq!(changed.relay_addon, Some(Installed::New));
    assert_eq!(changed.timeways_key, None);
    assert!(changed.new_slots);
    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    assert_eq!(
        computer.addon_file(ADDON, KEY_FILE),
        Some(key_lua(&relay_key))
    );
    assert!(computer.has_slots(App::Relay));
    assert!(!computer.any_slot(App::Timeways), "no Timeways slots");
    assert_eq!(computer.key(TIMEWAYS_KEY_FILE), None);
    assert!(!computer.folders.addons.join(TIMEWAYS).exists());
    assert!(config.relay.is_some());
    assert!(config.story.is_none());
}

#[test]
fn a_second_setup_changes_nothing() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Relay::On, KeyChoice::Keep);

    let again = install(&computer, Relay::On, KeyChoice::Keep);

    assert_eq!(
        again,
        Changed {
            relay_addon: Some(Installed::Unchanged),
            timeways_key: Some(Installed::Unchanged),
            new_slots: false,
        }
    );
}

#[test]
fn with_both_folders_setup_installs_both_apps_with_two_keys() {
    let computer = Computer::new(&[TIMEWAYS]);

    let changed = install(&computer, Relay::On, KeyChoice::Keep);
    let config = computer.config(Relay::On, Some(&[FoundModel::Claude]));

    assert_eq!(changed.relay_addon, Some(Installed::New));
    assert_eq!(changed.timeways_key, Some(Installed::Updated));
    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    let timeways_key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_ne!(relay_key, timeways_key);
    assert_eq!(timeways_key.len(), 64);
    assert_eq!(
        computer.addon_file(TIMEWAYS, KEY_FILE),
        Some(key_lua(&timeways_key))
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
fn with_only_timeways_setup_installs_no_relay_and_no_agent() {
    let computer = Computer::new(&[TIMEWAYS]);

    let changed = install(&computer, Relay::Off, KeyChoice::Keep);
    let config = computer.config(Relay::Off, Some(&[]));

    assert_eq!(changed.relay_addon, None);
    assert_eq!(changed.timeways_key, Some(Installed::Updated));
    assert!(changed.new_slots);
    assert!(!computer.folders.addons.join(ADDON).exists());
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
    install(&computer, Relay::Off, KeyChoice::Keep);
    let name = slot_name(App::Timeways, 1);
    let toc = computer.addon_file(&name, &format!("{name}.toc")).unwrap();
    assert!(toc.contains("## Dependencies: Timeways\n"), "{toc}");
}

#[test]
fn new_keys_replace_both_keys_and_both_key_files() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Relay::On, KeyChoice::Keep);
    let old_relay = computer.key(RELAY_KEY_FILE).unwrap();
    let old_timeways = computer.key(TIMEWAYS_KEY_FILE).unwrap();

    let changed = install(&computer, Relay::On, KeyChoice::New);

    let relay_key = computer.key(RELAY_KEY_FILE).unwrap();
    let timeways_key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_ne!(relay_key, old_relay);
    assert_ne!(timeways_key, old_timeways);
    assert_ne!(relay_key, timeways_key);
    assert_eq!(changed.relay_addon, Some(Installed::Updated));
    assert_eq!(changed.timeways_key, Some(Installed::Updated));
    assert_eq!(
        computer.addon_file(ADDON, KEY_FILE),
        Some(key_lua(&relay_key))
    );
    assert_eq!(
        computer.addon_file(TIMEWAYS, KEY_FILE),
        Some(key_lua(&timeways_key))
    );
}

#[cfg(unix)]
#[test]
fn the_keys_are_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Relay::Off, KeyChoice::Keep);
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

    let error = install_files(&computer.folders, Relay::Off, KeyChoice::Keep).unwrap_err();

    assert!(format!("{error:#}").contains("the same"), "{error:#}");
}

#[test]
fn the_bridge_writes_a_missing_timeways_key_file_again_and_no_other_file() {
    let computer = Computer::new(&[TIMEWAYS]);
    install(&computer, Relay::Off, KeyChoice::Keep);
    let dir = computer.folders.addons.join(TIMEWAYS);
    fs::remove_file(dir.join(KEY_FILE)).unwrap();
    fs::remove_file(dir.join("Timeways.toc")).unwrap();

    let repaired = repair_timeways_key(&computer.folders.config, &computer.folders.addons);

    assert_eq!(repaired.unwrap(), Some(Installed::Updated));
    let key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_eq!(computer.addon_file(TIMEWAYS, KEY_FILE), Some(key_lua(&key)));
    let names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names, [KEY_FILE], "the bridge never writes the TOC");
}

#[test]
fn with_no_timeways_folder_or_key_the_bridge_writes_nothing_for_timeways() {
    let computer = Computer::new(&[]);
    install(&computer, Relay::On, KeyChoice::Keep);
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
}

#[cfg(unix)]
#[test]
fn a_linked_timeways_checkout_gets_only_the_key_in_its_real_folder() {
    let computer = Computer::new(&[]);
    let checkout = computer.home().join("timeways-checkout");
    fs::create_dir(&checkout).unwrap();
    std::os::unix::fs::symlink(&checkout, computer.folders.addons.join(TIMEWAYS)).unwrap();

    install(&computer, Relay::Off, KeyChoice::Keep);

    let key = computer.key(TIMEWAYS_KEY_FILE).unwrap();
    assert_eq!(
        fs::read_to_string(checkout.join(KEY_FILE)).unwrap(),
        key_lua(&key)
    );
    let link = fs::symlink_metadata(computer.folders.addons.join(TIMEWAYS)).unwrap();
    assert!(link.file_type().is_symlink());
}

#[test]
fn install_makes_the_slots_of_each_app_that_is_there() {
    let computer = Computer::new(&[TIMEWAYS]);
    let apps = setup::install_all_slots(&computer.folders.addons, Relay::Off).unwrap();
    assert_eq!(apps, [App::Timeways]);
    assert!(!computer.any_slot(App::Relay));
    let apps = setup::install_all_slots(&computer.folders.addons, Relay::On).unwrap();
    assert_eq!(apps, [App::Relay, App::Timeways]);
}

#[test]
fn a_relay_config_gets_a_story_section_once_timeways_is_installed() {
    let computer = Computer::new(&[]);
    computer.config(Relay::On, None);
    let old = fs::read_to_string(computer.folders.config.join("config.toml")).unwrap();
    let wow = computer.wow();
    let parts = ConfigParts {
        wow: &wow,
        relay: None,
        story: Some(&[FoundModel::Claude]),
    };

    let text = setup::config_text(Some(&old), &parts).unwrap();
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
fn the_addon_files_of_the_relay_are_all_written() {
    let computer = Computer::new(&[]);
    install(&computer, Relay::On, KeyChoice::Keep);
    for (name, content) in install::ADDON_FILES {
        let written = fs::read(computer.folders.addons.join(ADDON).join(name)).unwrap();
        assert_eq!(written, content, "{name}");
    }
}
