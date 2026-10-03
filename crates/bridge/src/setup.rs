//! `gnomish-relay setup` for two products (SPEC.md 11.3 and 9.7, decision 15): the keys,
//! the key addons, the slots, and the config. The command line asks the questions and
//! prints the result.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use protocol::apps::App;

use crate::config::{self, Config};
use crate::config_edit;
use crate::config_story;
use crate::config_text::{self, RelayPart};
use crate::fs_safe::{make_private_dir, write_private};
use crate::install::{self, Installed};
use crate::model_setup::FoundModel;
use crate::receive::{KeySet, RELAY_KEY_FILE, TIMEWAYS_KEY_FILE};
use crate::run::now;
use crate::slots::{self, Files};

/// The product that one setup sets up. A setup never touches the other product.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    /// `gnomish-relay setup`: the coding agents in the game.
    Relay,
    /// `gnomish-relay setup --timeways`.
    Timeways,
}

impl Product {
    pub fn app(self) -> App {
        match self {
            Product::Relay => App::Relay,
            Product::Timeways => App::Timeways,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChoice {
    Keep,
    /// `--new-key`: a new key for the product of this setup.
    New,
}

/// The config folder and the `Interface/AddOns` folder of the game.
pub struct Folders {
    pub config: PathBuf,
    pub addons: PathBuf,
}

fn read_key(dir: &Path, file: &str) -> Option<String> {
    let hex = fs::read_to_string(dir.join(file)).ok()?;
    Some(hex.trim().to_owned())
}

/// Reads the key in `file`, or makes one. A new key is never equal to `other`, because
/// the bridge refuses equal keys (SPEC.md 9.7, decision 1).
fn key(dir: &Path, file: &str, choice: KeyChoice, other: Option<&str>) -> Result<String> {
    if choice == KeyChoice::Keep
        && let Some(hex) = read_key(dir, file)
    {
        return Ok(hex);
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
    /// The key addon of the product. `New` for a new key addon folder.
    pub key_addon: Installed,
    /// WoW finds a new slot folder only at launch.
    pub new_slots: bool,
}

/// The key of `product` in hex. Every setup also makes `strip.key` when it is missing,
/// because `KeySet` needs it. With no relay addon, it does nothing.
pub fn make_keys(dir: &Path, product: Product, keys: KeyChoice) -> Result<String> {
    make_private_dir(dir)?;
    let relay_keys = match product {
        Product::Relay => keys,
        Product::Timeways => KeyChoice::Keep,
    };
    let old_timeways = read_key(dir, TIMEWAYS_KEY_FILE);
    let relay = key(dir, RELAY_KEY_FILE, relay_keys, old_timeways.as_deref())?;
    let hex = match product {
        Product::Relay => relay,
        Product::Timeways => key(dir, TIMEWAYS_KEY_FILE, keys, Some(&relay))?,
    };
    // Equal keys stop setup here, as they stop the bridge.
    KeySet::load(dir)?;
    Ok(hex)
}

/// The key, the key addon, and the slots of `product`. They need nothing else, so they
/// come before the config (SPEC.md 11.3).
pub fn install_files(folders: &Folders, product: Product, keys: KeyChoice) -> Result<Changed> {
    let hex = make_keys(&folders.config, product, keys)?;
    let new_slots = install_slots(&folders.addons, product.app())?;
    let key_addon = match product {
        Product::Relay => install::write_relay_keys(&folders.addons, &hex)?,
        Product::Timeways => install::write_timeways_keys(&folders.addons, &hex)?,
    };
    Ok(Changed {
        key_addon,
        new_slots,
    })
}

/// Returns true when the slot folders are new.
fn install_slots(addons: &Path, app: App) -> Result<bool> {
    let new = !slots::is_installed(addons, app);
    slots::install(addons, app, &Files::empty(app, now()))?;
    Ok(new)
}

/// The products that this computer has: the relay with the relay part of the config,
/// and Timeways with its key. An addon folder alone counts for nothing.
pub fn products_of(config: &Config, config_dir: &Path) -> Vec<Product> {
    let mut products = Vec::new();
    if config.relay.is_some() {
        products.push(Product::Relay);
    }
    if config_dir.join(TIMEWAYS_KEY_FILE).is_file() {
        products.push(Product::Timeways);
    }
    products
}

/// `gnomish-relay install`: the slots of each product of `products`.
pub fn install_all_slots(addons: &Path, products: &[Product]) -> Result<()> {
    for product in products {
        let app = product.app();
        slots::install(addons, app, &Files::empty(app, now()))?;
    }
    Ok(())
}

/// The slots of each product of `products` that `addons` lacks. The desktop app runs
/// this at each start, so a game installed after setup gets its slots (SPEC.md 7.9).
pub fn install_missing_slots(addons: &Path, products: &[Product]) -> Result<()> {
    for product in products {
        let app = product.app();
        if !slots::is_installed(addons, app) {
            slots::install(addons, app, &Files::empty(app, now()))?;
        }
    }
    Ok(())
}

/// Writes the Timeways key addon again when it is missing or old, as the bridge does for
/// the relay at each start (SPEC.md 11.3). The bridge never makes a key.
pub fn repair_timeways_key(config_dir: &Path, addons: &Path) -> Result<Option<Installed>> {
    let Ok(hex) = fs::read_to_string(config_dir.join(TIMEWAYS_KEY_FILE)) else {
        return Ok(None);
    };
    install::write_timeways_keys(addons, hex.trim()).map(Some)
}

/// The parts that the config gets. Each one is `Some` only when the config lacks it.
pub struct ConfigParts<'a> {
    /// The game folder, when the config has none or another one.
    pub wow: Option<&'a Path>,
    pub relay: Option<RelayPart<'a>>,
    /// Agents on `PATH` that a config with the relay lacks (`new_agents`).
    pub new_agents: &'a [config::Found<'a>],
    pub story: Option<&'a [FoundModel]>,
}

/// The agents of `found` that the relay part of `config` has no entry for. A first
/// config, or one with no relay, gets them with its relay part.
pub fn new_agents<'a>(
    found: &[config::Found<'a>],
    config: Option<&Config>,
) -> Vec<config::Found<'a>> {
    let Some(relay) = config.and_then(|c| c.relay.as_ref()) else {
        return Vec::new();
    };
    found
        .iter()
        .filter(|(name, _, _)| !relay.agents.contains_key(*name))
        .copied()
        .collect()
}

/// The new text of the config, or `None` when it needs no change. Setup changes no key
/// that exists but the game folder: it writes a first config, or adds a missing part.
pub fn config_text(existing: Option<&str>, parts: &ConfigParts) -> Result<Option<String>> {
    let Some(existing) = existing else {
        return Ok(Some(first_config(parts)));
    };
    let mut text = existing.to_owned();
    if let Some(wow) = parts.wow {
        text = config_edit::with_wow_path(&text, wow)?;
    }
    if let Some(relay) = &parts.relay {
        text = config_text::with_relay(&text, relay);
    }
    if !parts.new_agents.is_empty() {
        text = config_text::with_agents(&text, parts.new_agents);
    }
    if let Some(models) = parts.story {
        text = config_text::with_story(&text, models);
    }
    Ok((text != existing).then_some(text))
}

/// The roots for a relay config that has none yet. Each root goes in through the edit of
/// a desktop Approve, so the base of the folders of the game stays the same (SPEC.md 9.12).
pub fn with_first_roots(text: &str, roots: &[String], home: &Path) -> Result<String> {
    let mut text = text.to_owned();
    for root in roots {
        text = config_edit::with_root(&text, root, home)?;
    }
    Ok(text)
}

fn first_config(parts: &ConfigParts) -> String {
    let Some(relay) = &parts.relay else {
        return config_text::timeways_config(parts.wow, parts.story.unwrap_or(&[]));
    };
    let text = config_text::relay_config(parts.wow, relay);
    match parts.story {
        Some(models) => config_text::with_story(&text, models),
        None => text,
    }
}

/// Checks a new text before it replaces the config, so setup never leaves a config that
/// the bridge refuses. A relay part with an error before keeps it, and the result then
/// has no relay part: `setup --timeways` goes on (SPEC.md 9.7, decision 15).
pub fn write_config(dir: &Path, text: &str, home: &Path) -> Result<Config> {
    const NO_LOAD: &str = "setup made a config that does not load";
    let parts = config::parse_parts(text, home).context(NO_LOAD)?;
    let relay = match parts.relay {
        Ok(relay) => relay,
        Err(_) if relay_part_has_an_error(dir, home) => None,
        Err(e) => return Err(e.context(NO_LOAD)),
    };
    make_private_dir(dir)?;
    write_private(dir, config::FILE, text)?;
    Ok(Config {
        relay,
        ..parts.config
    })
}

/// The config as `write_config` judges it: a relay part with an error loads as none.
/// `read_existing` already stopped a plain setup on such an error.
pub fn load_config(dir: &Path, home: &Path) -> Result<Config> {
    let parts = config::parse_parts(&config::read_text(dir)?, home)?;
    Ok(Config {
        relay: parts.relay.ok().flatten(),
        ..parts.config
    })
}

fn relay_part_has_an_error(dir: &Path, home: &Path) -> bool {
    let Ok(text) = fs::read_to_string(dir.join(config::FILE)) else {
        return false;
    };
    config::parse_parts(&text, home).is_ok_and(|parts| parts.relay.is_err())
}

/// The config that a setup finds, and the line that it prints about it.
pub struct Existing {
    pub text: String,
    pub config: Config,
    /// A repair of `default_cwd`, or an error in the relay part for `setup --timeways`.
    pub lines: Vec<String>,
}

/// `None` with no config yet. Both setups repair `default_cwd` (SPEC.md 12). Only a plain
/// setup stops on an error in the relay part, because the relay part is its own.
pub fn read_existing(dir: &Path, home: &Path, product: Product) -> Result<Option<Existing>> {
    let text = match config::read_text(dir) {
        Ok(text) => text,
        Err(e) if e.is::<config::SetupUnfinished>() => return Ok(None),
        Err(e) => return Err(e),
    };
    let (text, mut lines) = repair_default_folder(dir, text, home)?;
    let not_valid = || format!("{} is not valid", dir.join(config::FILE).display());
    let parts = config::parse_parts(&text, home).with_context(not_valid)?;
    let relay = match (parts.relay, product) {
        (Ok(relay), _) => relay,
        (Err(e), Product::Relay) => return Err(e.context(not_valid())),
        (Err(e), Product::Timeways) => {
            lines.push(relay_error_line(&e));
            None
        }
    };
    let config = Config {
        relay,
        ..parts.config
    };
    Ok(Some(Existing {
        text,
        config,
        lines,
    }))
}

fn relay_error_line(error: &anyhow::Error) -> String {
    format!(
        "Gnomish Relay: config.toml has an error, and the desktop app won't start until it's \
         fixed: {error:#}"
    )
}

/// A `default_cwd` that breaks its rule becomes `~`, which the rule always allows.
fn repair_default_folder(dir: &Path, text: String, home: &Path) -> Result<(String, Vec<String>)> {
    let Ok(Err(error)) = config::parse_parts(&text, home).map(|parts| parts.relay) else {
        return Ok((text, Vec::new()));
    };
    let Some(bad) = error.downcast_ref::<config::BadDefaultFolder>() else {
        return Ok((text, Vec::new()));
    };
    let repaired = config_edit::with_home_default_cwd(&text)?;
    write_private(dir, config::FILE, &repaired)?;
    let line = format!("Fixed config.toml: {bad}, so it's now ~ (your home folder).");
    Ok((repaired, vec![line]))
}

/// Puts `model` into `[story]` of the config (SPEC.md 11.6).
pub fn write_story_model(dir: &Path, model: &FoundModel, home: &Path) -> Result<Config> {
    let file = dir.join(config::FILE);
    let text =
        fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
    write_config(dir, &config_story::with_story_model(&text, model), home)
}

/// The level that the config gives the default agent, and how to change it. The
/// config is the ceiling of every chat (S6), so the player needs to see it.
/// No "change it" part: the game picks only `ask` or `auto-edit`, and a player never edits
/// a config file to change a setting.
pub fn level_line(relay: &config::RelayConfig) -> String {
    let name = &relay.policy.default_agent;
    let level = relay.policy.agents.get(name).copied().unwrap_or_default();
    format!("Permissions: {}. {}", level.word(), level.meaning())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_config_with_no_relay_is_the_timeways_config() {
        let parts = ConfigParts {
            wow: Some(Path::new("/wow")),
            relay: None,
            new_agents: &[],
            story: Some(&[]),
        };
        let text = config_text(None, &parts).unwrap().unwrap();
        assert!(text.starts_with("[wow]\n"), "{text}");
        assert!(text.contains("\n[story]\n"), "{text}");
        assert!(!text.contains("allowed_roots"), "{text}");
    }

    #[test]
    fn an_existing_config_that_lacks_nothing_stays_as_it_is() {
        let parts = ConfigParts {
            wow: None,
            relay: None,
            new_agents: &[],
            story: None,
        };
        assert_eq!(config_text(Some("anything"), &parts).unwrap(), None);
    }

    #[test]
    fn an_existing_config_gets_the_new_game_folder_and_keeps_the_rest() {
        let parts = ConfigParts {
            wow: Some(Path::new("/games/wow")),
            relay: None,
            new_agents: &[],
            story: None,
        };
        let old = "# mine\n[wow]\npath = \"/old/wow\"\n[story]\nmodel = \"claude\"\n";

        let text = config_text(Some(old), &parts).unwrap().unwrap();

        assert_eq!(
            text,
            "# mine\n[wow]\npath = \"/games/wow\"\n[story]\nmodel = \"claude\"\n"
        );
    }

    fn level_line_of(permission: &str) -> String {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("Code")).unwrap();
        let text = format!(
            "allowed_roots = [\"~/Code\"]\ndefault_agent = \"claude\"\n[wow]\npath = \"~/wow\"\n\
             [agents.claude]\nkind = \"claude\"\ncommand = [\"claude\"]\npermission = \"{permission}\"\n"
        );
        let config = config::parse(&text, home.path()).unwrap();
        level_line(config.require_relay().unwrap())
    }

    #[test]
    fn setup_says_what_the_permissions_of_the_default_agent_do() {
        assert_eq!(
            level_line_of("auto-edit"),
            "Permissions: auto-edit. Agents edit files and run commands in the sandbox on their \
             own, and ask you before anything risky."
        );
        assert_eq!(
            level_line_of("ask"),
            "Permissions: ask. Agents ask you before each edit and each command."
        );
    }
}
