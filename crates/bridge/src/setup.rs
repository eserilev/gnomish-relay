//! `gnomish-relay setup` for two apps (SPEC.md 11.3 and 9.7, decision 15): the keys,
//! the addon files, the slots, and the config. The command line asks the questions and
//! prints the result.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use protocol::apps::App;

use crate::config::{self, Config};
use crate::config_text::{self, RelayPart};
use crate::fs_safe::write_private;
use crate::install::{self, Installed};
use crate::model_setup::FoundModel;
use crate::receive::{KeySet, RELAY_KEY_FILE, TIMEWAYS_KEY_FILE};
use crate::run::now;
use crate::slots::{self, Files};

/// Whether this computer runs the relay: the coding agents in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relay {
    On,
    Off,
}

/// Whether setup can decide the relay alone, or asks the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayChoice {
    Decided(Relay),
    Ask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChoice {
    Keep,
    /// `--new-key`: a new key for each app that this computer has.
    New,
}

/// What setup knows before it asks: the flags, the config, and the addon folders.
pub struct Found {
    /// `--relay` or `--roots`.
    pub relay_asked: bool,
    /// `None` with no config yet.
    pub config_has_relay: Option<bool>,
    pub relay_folder: bool,
    pub timeways_folder: bool,
}

/// Only a player with Timeways, no relay folder, and no config gets a question.
pub fn relay_choice(found: &Found) -> RelayChoice {
    if found.relay_asked {
        return RelayChoice::Decided(Relay::On);
    }
    if let Some(has_relay) = found.config_has_relay {
        return RelayChoice::Decided(if has_relay { Relay::On } else { Relay::Off });
    }
    if found.relay_folder || !found.timeways_folder {
        return RelayChoice::Decided(Relay::On);
    }
    RelayChoice::Ask
}

/// The config folder and the `Interface/AddOns` folder of the game.
pub struct Folders {
    pub config: PathBuf,
    pub addons: PathBuf,
}

/// Reads the key in `file`, or makes one. A new key is never equal to `other`, because
/// the bridge refuses equal keys (SPEC.md 9.7, decision 1).
fn key(dir: &Path, file: &str, choice: KeyChoice, other: Option<&str>) -> Result<String> {
    if choice == KeyChoice::Keep
        && let Ok(hex) = fs::read_to_string(dir.join(file))
    {
        return Ok(hex.trim().to_owned());
    }
    let mut hex = install::new_key()?;
    while Some(hex.as_str()) == other {
        hex = install::new_key()?;
    }
    write_private(dir, file, &hex)?;
    Ok(hex)
}

/// What the file steps changed.
#[derive(Debug, PartialEq, Eq)]
pub struct Changed {
    /// `None` with the relay off.
    pub relay_addon: Option<Installed>,
    /// `None` with no Timeways folder.
    pub timeways_key: Option<Installed>,
    /// WoW finds a new slot folder only at launch.
    pub new_slots: bool,
}

/// The keys, the addon files, and the slots of each app that this computer has. They
/// need nothing else, so they come before the config (SPEC.md 11.3).
pub fn install_files(folders: &Folders, relay: Relay, keys: KeyChoice) -> Result<Changed> {
    let dir = &folders.config;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    // `KeySet` needs the relay key, so every player gets it. With no addon, it does nothing.
    let relay_key = key(dir, RELAY_KEY_FILE, keys, None)?;
    let mut new_slots = false;
    let relay_addon = match relay {
        Relay::On => {
            new_slots |= install_slots(&folders.addons, App::Relay)?;
            Some(install::install_addon(&folders.addons, &relay_key)?)
        }
        Relay::Off => None,
    };
    let timeways_key = match install::timeways_dir(&folders.addons) {
        Some(timeways) => {
            let hex = key(dir, TIMEWAYS_KEY_FILE, keys, Some(&relay_key))?;
            new_slots |= install_slots(&folders.addons, App::Timeways)?;
            Some(install::write_key_file(&timeways, &hex)?)
        }
        None => None,
    };
    // Equal keys stop setup here, as they stop the bridge.
    KeySet::load(dir)?;
    Ok(Changed {
        relay_addon,
        timeways_key,
        new_slots,
    })
}

/// Returns true when the slot folders are new.
fn install_slots(addons: &Path, app: App) -> Result<bool> {
    let new = !slots::is_installed(addons, app);
    slots::install(addons, app, &Files::empty(app, now()))?;
    Ok(new)
}

/// `gnomish-relay install`: the slots of the relay when it is on, and of Timeways when
/// its addon is there.
pub fn install_all_slots(addons: &Path, relay: Relay) -> Result<Vec<App>> {
    let mut apps = Vec::new();
    if relay == Relay::On {
        apps.push(App::Relay);
    }
    if install::timeways_dir(addons).is_some() {
        apps.push(App::Timeways);
    }
    for app in &apps {
        slots::install(addons, *app, &Files::empty(*app, now()))?;
    }
    Ok(apps)
}

/// Writes `Key.lua` into the Timeways folder again when it is missing or old, as the
/// bridge does for the relay at each start (SPEC.md 11.3). The bridge never makes a key.
pub fn repair_timeways_key(config_dir: &Path, addons: &Path) -> Result<Option<Installed>> {
    let Some(timeways) = install::timeways_dir(addons) else {
        return Ok(None);
    };
    let Ok(hex) = fs::read_to_string(config_dir.join(TIMEWAYS_KEY_FILE)) else {
        return Ok(None);
    };
    install::write_key_file(&timeways, hex.trim()).map(Some)
}

/// The parts that the config gets. Each one is `Some` only when the config lacks it.
pub struct ConfigParts<'a> {
    pub wow: &'a Path,
    pub relay: Option<RelayPart<'a>>,
    pub story: Option<&'a [FoundModel]>,
}

/// The new text of the config, or `None` when it needs no change. Setup never changes a
/// key that exists: it writes a first config, or adds a missing part.
pub fn config_text(existing: Option<&str>, parts: &ConfigParts) -> Option<String> {
    let Some(existing) = existing else {
        let text = match (&parts.relay, parts.story) {
            (Some(relay), story) => {
                let text = config_text::relay_config(parts.wow, relay);
                match story {
                    Some(models) => config_text::with_story(&text, models),
                    None => text,
                }
            }
            (None, story) => config_text::timeways_config(parts.wow, story.unwrap_or(&[])),
        };
        return Some(text);
    };
    if parts.relay.is_none() && parts.story.is_none() {
        return None;
    }
    let mut text = existing.to_owned();
    if let Some(relay) = &parts.relay {
        text = config_text::with_relay(&text, relay);
    }
    if let Some(models) = parts.story {
        text = config_text::with_story(&text, models);
    }
    Some(text)
}

/// Checks a new text before it replaces the config, so setup never leaves a config that
/// the bridge refuses.
pub fn write_config(dir: &Path, text: &str, home: &Path) -> Result<Config> {
    let config = config::parse(text, home).context("setup made a config that does not load")?;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    write_private(dir, config::FILE, text)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(relay_asked: bool, config: Option<bool>, relay: bool, timeways: bool) -> Found {
        Found {
            relay_asked,
            config_has_relay: config,
            relay_folder: relay,
            timeways_folder: timeways,
        }
    }

    const ON: RelayChoice = RelayChoice::Decided(Relay::On);
    const OFF: RelayChoice = RelayChoice::Decided(Relay::Off);

    #[test]
    fn relay_or_roots_turns_the_relay_on_even_for_a_timeways_config() {
        assert_eq!(relay_choice(&found(true, Some(false), false, true)), ON);
    }

    #[test]
    fn an_existing_config_decides_the_relay() {
        assert_eq!(relay_choice(&found(false, Some(true), false, true)), ON);
        assert_eq!(relay_choice(&found(false, Some(false), true, true)), OFF);
    }

    #[test]
    fn a_relay_folder_turns_the_relay_on() {
        assert_eq!(relay_choice(&found(false, None, true, true)), ON);
    }

    #[test]
    fn with_no_timeways_folder_the_relay_is_on_as_before() {
        assert_eq!(relay_choice(&found(false, None, false, false)), ON);
    }

    #[test]
    fn only_a_new_player_with_timeways_alone_gets_the_question() {
        assert_eq!(
            relay_choice(&found(false, None, false, true)),
            RelayChoice::Ask
        );
    }

    #[test]
    fn a_first_config_with_no_relay_is_the_timeways_config() {
        let parts = ConfigParts {
            wow: Path::new("/wow"),
            relay: None,
            story: Some(&[]),
        };
        let text = config_text(None, &parts).unwrap();
        assert!(text.starts_with("[wow]\n"), "{text}");
        assert!(text.contains("\n[story]\n"), "{text}");
        assert!(!text.contains("allowed_roots"), "{text}");
    }

    #[test]
    fn an_existing_config_that_lacks_nothing_stays_as_it_is() {
        let parts = ConfigParts {
            wow: Path::new("/wow"),
            relay: None,
            story: None,
        };
        assert_eq!(config_text(Some("anything"), &parts), None);
    }
}
