//! The `gnomish-relay setup` command: the setup of Gnomish Relay, its questions in the
//! terminal, and the lines that it prints. `setup --timeways` is in `setup_timeways.rs`,
//! and `setup.rs` holds the steps that need no terminal.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::check_agent;
use crate::config::{self, Config, RelayConfig, with_tilde};
use crate::config_text::RelayPart;
use crate::dirs::Dirs;
use crate::game_choice;
use crate::hooks_install;
use crate::install;
use crate::model_setup;
use crate::relay_addon::{self, RelayAddon};
use crate::service;
use crate::setup::{self, KeyChoice, Product};
use crate::setup_timeways;
use crate::status::{self, SandboxFound};
use crate::wsl;

/// Reads one answer in a terminal. With no terminal, or an empty answer, the default.
fn ask(question: &str, default: &str) -> Result<String> {
    let answer = read_answer(&format!("{question} [{default}]: "))?.unwrap_or_default();
    Ok(if answer.is_empty() { default } else { &answer }.to_owned())
}

pub(crate) fn stdin_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// The trimmed answer to `prompt`, or `None` with no terminal.
pub(crate) fn read_answer(prompt: &str) -> Result<Option<String>> {
    use std::io::{BufRead, Write};
    if !stdin_is_terminal() {
        return Ok(None);
    }
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(Some(answer.trim().to_owned()))
}

/// The roots of `--roots`, else the folders of code projects that setup finds. Setup
/// asks no folder question: the game adds a folder with a click on the desktop (SPEC.md 9.12).
fn choose_roots(home: &Path, given: Option<&str>) -> Result<Vec<String>> {
    match given {
        Some(list) => roots_in(list, home),
        None => Ok(install::suggest_roots(home)
            .iter()
            .map(|p| with_tilde(p, home))
            .collect()),
    }
}

/// Where agents work, and how the player adds a folder.
fn roots_line(roots: &[PathBuf], home: &Path) -> String {
    if roots.is_empty() {
        return "Pick a project folder in the game to get started.".into();
    }
    let shown: Vec<String> = roots.iter().map(|r| with_tilde(r, home)).collect();
    format!(
        "Agents can work in {}. To add another folder, pick it in the game.",
        shown.join(", ")
    )
}

/// Each root must be a folder that exists.
fn roots_in(answer: &str, home: &Path) -> Result<Vec<String>> {
    let roots: Vec<String> = answer
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect();
    if roots.is_empty() {
        bail!(
            "setup needs the folders that agents can work in. Run gnomish-relay setup --roots ~/code"
        );
    }
    for root in &roots {
        if !config::expand(root, home)?.is_dir() {
            bail!("{root} isn't a folder");
        }
    }
    Ok(roots)
}

fn option<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| *a == name)?;
    args.get(at + 1).copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Autostart {
    On,
    Off,
}

const ONE_APP: &str = "Set up one app at a time: run gnomish-relay setup for Gnomish Relay, \
                       or gnomish-relay setup --timeways for Timeways.";

/// `setup [--wow folder] [--roots a,b] [--relay] [--timeways] [--new-key] [--autostart]
/// [--no-autostart]`. The installers always add `--autostart`, so `--no-autostart` wins.
/// The folder can also come with no `--wow`, as before 0.3.1.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SetupArgs<'a> {
    pub folder: Option<&'a str>,
    pub roots: Option<&'a str>,
    pub product: Product,
    pub keys: KeyChoice,
    pub autostart: Autostart,
}

impl<'a> SetupArgs<'a> {
    fn parse(args: &[&'a str]) -> Result<SetupArgs<'a>> {
        let roots = option(args, "--roots");
        let wow = option(args, "--wow");
        let bare = args
            .iter()
            .copied()
            .find(|a| !a.starts_with("--") && Some(*a) != roots && Some(*a) != wow);
        // `--relay` and `--roots` are for the relay, which a plain setup already is.
        let relay_flag = args.contains(&"--relay") || roots.is_some();
        let product = if args.contains(&"--timeways") {
            Product::Timeways
        } else {
            Product::Relay
        };
        if product == Product::Timeways && relay_flag {
            bail!(ONE_APP);
        }
        Ok(SetupArgs {
            folder: wow.or(bare),
            roots,
            product,
            keys: if args.contains(&"--new-key") {
                KeyChoice::New
            } else {
                KeyChoice::Keep
            },
            autostart: if args.contains(&"--autostart") && !args.contains(&"--no-autostart") {
                Autostart::On
            } else {
                Autostart::Off
            },
        })
    }
}

/// What setup finds before its first step: the config and the game.
pub(crate) struct Found<'a> {
    /// The text of `config.toml` with its config, with no config yet `None`.
    pub existing: Option<(String, Config)>,
    pub wow: Option<&'a Path>,
    pub addons: Option<PathBuf>,
}

/// Each product has its own setup, which never touches the other product (SPEC.md 9.7,
/// decision 15). Every step leaves alone what works, so a second run is safe.
pub fn setup(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let args = SetupArgs::parse(args)?;
    let existing = setup::read_existing(&dirs.config, &dirs.home, args.product)?;
    for line in existing.iter().flat_map(|found| &found.lines) {
        println!("{line}");
    }
    let existing = existing.map(|found| (found.text, found.config));
    let config_wow = existing
        .as_ref()
        .and_then(|(_, config)| config.wow.as_deref());
    let game = game_choice::choose(args.folder, config_wow, &dirs.home);
    let wow = game.folder();
    let addons = wow.map(addons_of).transpose()?;
    let setup_command = match args.product {
        Product::Relay => "gnomish-relay setup",
        Product::Timeways => "gnomish-relay setup --timeways",
    };
    if let Some(line) = game_choice::game_line(&game, setup_command) {
        println!("{line}");
    }
    let found = Found {
        existing,
        wow,
        addons,
    };
    match args.product {
        Product::Relay => setup_relay(dirs, &args, found),
        Product::Timeways => setup_timeways::setup(dirs, &args, found),
    }
}

fn setup_relay(dirs: &Dirs, args: &SetupArgs, found: Found) -> Result<()> {
    let existing = found.existing.as_ref();
    // Every question comes before the first file, so a stop at a question leaves nothing
    // half done.
    let harnesses = relay_questions(existing)?;
    // The key addon and the slots first: they need nothing else, and a later step can fail.
    let changed = product_files(dirs, found.addons.as_deref(), Product::Relay, args.keys)?;
    let config = setup_relay_config(dirs, found.wow, existing, &harnesses, args.roots)?;
    print_relay(dirs, &config);
    autostart(dirs, found.wow, args.autostart, Product::Relay);
    // Setup changes no settings of an agent: they belong to the user (SPEC.md 10.5).
    println!("{}", hooks_install::SETUP_HINT);
    let (Some(changed), Some(addons)) = (changed, found.addons) else {
        println!("{}", game_choice::NO_WOW);
        return Ok(());
    };
    let addon = relay_addon::find(&addons);
    println!("{}", relay_last_line(&changed, args.keys, addon));
    Ok(())
}

/// WoW makes `Interface/AddOns` at its first start. Setup makes it earlier.
fn addons_of(wow: &Path) -> Result<PathBuf> {
    if !wow.is_dir() {
        bail!("{} isn't a folder", wow.display());
    }
    let addons = install::addons_dir(wow);
    std::fs::create_dir_all(&addons)
        .with_context(|| format!("cannot make {}", addons.display()))?;
    Ok(addons)
}

/// The key of `product`, and with a game its key addon and its slots.
pub(crate) fn product_files(
    dirs: &Dirs,
    addons: Option<&Path>,
    product: Product,
    keys: KeyChoice,
) -> Result<Option<setup::Changed>> {
    let Some(addons) = addons else {
        setup::make_keys(&dirs.config, product, keys)?;
        return Ok(None);
    };
    let folders = setup::Folders {
        config: dirs.config.clone(),
        addons: addons.to_owned(),
    };
    setup::install_files(&folders, product, keys).map(Some)
}

/// A failed autostart prints one line, and setup goes on (SPEC.md 11.3). A Timeways
/// player gets no line when it works: the app is a detail of Timeways for them.
pub(crate) fn autostart(dirs: &Dirs, wow: Option<&Path>, autostart: Autostart, product: Product) {
    if autostart == Autostart::Off {
        return;
    }
    let start = match wow {
        Some(_) => service::Start::Now,
        None => service::Start::AtLogin,
    };
    match service::autostart(dirs, start) {
        Ok(_) if product == Product::Timeways => {}
        Ok(log) => {
            println!("Desktop app: on, starts at login");
            if let Some(log) = log {
                println!("{log}");
            }
        }
        Err(e) => println!(
            "Desktop app: can't start at login ({e:#}). To start it now, run gnomish-relay run"
        ),
    }
}

/// The harnesses with no ACP mode that the player adds, asked only for a config with
/// no relay part yet.
fn relay_questions(existing: Option<&(String, Config)>) -> Result<Vec<&'static str>> {
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    if !lacks_relay {
        return Ok(Vec::new());
    }
    // Under WSL, a Windows agent on the PATH runs outside every wall (SPEC.md 11.5).
    choose_harnesses(&wsl::path_var())
}

/// The game folder for the config, when the config has none or another one.
pub(crate) fn new_wow<'a>(
    wow: Option<&'a Path>,
    existing: Option<&(String, Config)>,
) -> Option<&'a Path> {
    let old_wow = existing.and_then(|(_, config)| config.wow.as_deref());
    wow.filter(|wow| old_wow != Some(*wow))
}

/// Writes the config with `parts`, or loads it when it needs no change.
pub(crate) fn write_parts(
    dirs: &Dirs,
    existing: Option<&(String, Config)>,
    parts: &setup::ConfigParts,
) -> Result<Config> {
    let text = existing.map(|(text, _)| text.as_str());
    match setup::config_text(text, parts)? {
        Some(new) => setup::write_config(&dirs.config, &new, &dirs.home),
        None => setup::load_config(&dirs.config, &dirs.home),
    }
}

/// Writes the first config, or adds the relay part or the agents that it lacks. It
/// never adds `[story]`: that is the job of `setup --timeways`.
fn setup_relay_config(
    dirs: &Dirs,
    wow: Option<&Path>,
    existing: Option<&(String, Config)>,
    harnesses: &[&'static str],
    roots_given: Option<&str>,
) -> Result<Config> {
    // Under WSL, a Windows agent on the PATH runs outside every wall (SPEC.md 11.5).
    let path_var = wsl::path_var();
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    let agents = install::find_agents(&path_var);
    let roots = if lacks_relay {
        Some(choose_roots(&dirs.home, roots_given)?)
    } else {
        None
    };
    // A local model is also for the agents: the relay part opens its port.
    let models = if lacks_relay {
        model_setup::find_models(&path_var)
    } else {
        Vec::new()
    };
    let local_ports = model_setup::local_ports(&models);
    let new_agents = setup::new_agents(&agents, existing.map(|(_, config)| config));
    let parts = setup::ConfigParts {
        wow: new_wow(wow, existing),
        relay: roots.as_deref().map(|roots| RelayPart {
            agents: &agents,
            harnesses,
            roots,
            local_ports: &local_ports,
        }),
        new_agents: &new_agents,
        story: None,
    };
    let config = write_parts(dirs, existing, &parts)?;
    let config = if roots.is_none() && has_no_roots(&config) {
        fill_empty_roots(dirs, config, roots_given)?
    } else {
        config
    };
    let added = config.relay.as_ref().map(|relay| &relay.agents);
    for (name, _, _) in &new_agents {
        if added.is_some_and(|agents| agents.contains_key(*name)) {
            println!("Added agent: {name}. To use it, pick it in Settings in the game");
        }
    }
    Ok(config)
}

fn has_no_roots(config: &Config) -> bool {
    config
        .relay
        .as_ref()
        .is_some_and(|relay| relay.policy.folders.roots.is_empty())
}

/// An older setup found no code folder, so setup looks again. A config that can't
/// change keeps its empty roots: the game still adds folders one by one.
fn fill_empty_roots(dirs: &Dirs, config: Config, roots_given: Option<&str>) -> Result<Config> {
    let roots = choose_roots(&dirs.home, roots_given)?;
    if roots.is_empty() {
        return Ok(config);
    }
    let text = std::fs::read_to_string(dirs.config.join(config::FILE))?;
    match setup::with_first_roots(&text, &roots, &dirs.home) {
        Ok(new) => setup::write_config(&dirs.config, &new, &dirs.home),
        Err(e) => {
            println!(
                "Found code folders, but config.toml can't change: {e:#}. Add them to allowed_roots by hand."
            );
            Ok(config)
        }
    }
}

/// A harness with no ACP mode runs its own commands, so setup adds it only on a yes.
fn choose_harnesses(path_var: &std::ffi::OsStr) -> Result<Vec<&'static str>> {
    let mut chosen = Vec::new();
    for name in install::find_harnesses(path_var) {
        let answer = ask(
            &format!(
                "Found {name}. Add it as an agent? It runs its own commands without asking, inside the sandbox. (y/N)"
            ),
            "n",
        )?;
        if answer.eq_ignore_ascii_case("y") {
            chosen.push(name);
        }
    }
    Ok(chosen)
}

fn print_relay(dirs: &Dirs, config: &Config) {
    let Some(relay_config) = &config.relay else {
        return;
    };
    for line in relay_lines(dirs, relay_config) {
        println!("{line}");
    }
    println!("{}", setup::level_line(relay_config));
    println!(
        "{}",
        roots_line(&shown_roots(relay_config), &real_home(dirs))
    );
}

/// The roots are resolved, so the home folder that they start with is resolved too.
fn real_home(dirs: &Dirs) -> PathBuf {
    dirs.home
        .canonicalize()
        .unwrap_or_else(|_| dirs.home.clone())
}

fn shown_roots(config: &RelayConfig) -> Vec<PathBuf> {
    let roots = config.policy.folders.roots.iter();
    roots
        .map(|r| PathBuf::from(String::from_utf8_lossy(r).into_owned()))
        .collect()
}

/// The agent and the sandbox, which setup checks by starting them.
fn relay_lines(dirs: &Dirs, config: &RelayConfig) -> Vec<String> {
    let gate = check_agent::check_gate(dirs, config);
    let path = std::env::var_os("PATH").unwrap_or_default();
    let sandbox = SandboxFound::of(&gate.sandbox.tool, &path);
    vec![
        status::agent_line(config, &gate),
        status::sandbox_line(&sandbox),
    ]
}

/// A missing or old addon comes last, so it is also the last line of the installers
/// (SPEC.md 11.3). It replaces "All set".
fn relay_last_line(changed: &setup::Changed, keys: KeyChoice, addon: RelayAddon) -> &'static str {
    relay_addon::next_step(addon).unwrap_or_else(|| all_set(changed, keys))
}

/// WoW finds a new addon folder only at launch, and a new key only after a `/reload`.
fn all_set(changed: &setup::Changed, keys: KeyChoice) -> &'static str {
    if changed.new_slots || changed.key_addon == install::Installed::New {
        return "All set. Restart WoW, then type /relay";
    }
    if changed.key_addon == install::Installed::Updated || keys == KeyChoice::New {
        return "All set. Type /reload in WoW";
    }
    "All set"
}

/// `gnomish-relay install`: the slots of each product that this computer has.
pub fn install_slots(dirs: &Dirs) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    config.game()?;
    let products = setup::products_of(&config, &dirs.config);
    for game in config.games() {
        setup::install_all_slots(&game.addons, &products)?;
        for product in &products {
            println!(
                "Made {} addon files for {:?} in {}",
                protocol::slot::SLOTS,
                product.app(),
                game.addons.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_takes_the_folder_the_roots_and_the_flags_in_any_order() {
        let args = SetupArgs::parse(&["--roots", "~/a,~/b", "/games/wow", "--autostart"]).unwrap();
        assert_eq!(
            args,
            SetupArgs {
                folder: Some("/games/wow"),
                roots: Some("~/a,~/b"),
                product: Product::Relay,
                keys: KeyChoice::Keep,
                autostart: Autostart::On,
            }
        );
        let plain = SetupArgs::parse(&["--new-key"]).unwrap();
        assert_eq!(plain.folder, None);
        assert_eq!(plain.product, Product::Relay);
        assert_eq!(plain.keys, KeyChoice::New);
        assert_eq!(plain.autostart, Autostart::Off);
    }

    #[test]
    fn the_installer_adds_autostart_and_no_autostart_wins() {
        let args = SetupArgs::parse(&["--autostart", "--timeways", "--no-autostart"]).unwrap();
        assert_eq!(args.autostart, Autostart::Off);
        assert_eq!(args.product, Product::Timeways);
    }

    #[test]
    fn the_relay_flag_is_the_same_as_a_plain_setup() {
        let args = SetupArgs::parse(&["--relay"]).unwrap();
        assert_eq!(args.product, Product::Relay);
    }

    #[test]
    fn timeways_with_a_flag_of_the_relay_is_refused() {
        for flags in [
            ["--timeways", "--relay"].as_slice(),
            ["--roots", "~/a", "--timeways"].as_slice(),
        ] {
            let error = SetupArgs::parse(flags).unwrap_err();
            assert_eq!(error.to_string(), ONE_APP, "{flags:?}");
        }
    }

    #[test]
    fn setup_takes_the_wow_folder_with_or_with_no_flag() {
        let flag = SetupArgs::parse(&["--relay", "--wow", "/games/wow", "--roots", "~/a"]).unwrap();
        let bare = SetupArgs::parse(&["/games/wow", "--timeways"]).unwrap();

        assert_eq!(flag.folder, Some("/games/wow"));
        assert_eq!(flag.roots, Some("~/a"));
        assert_eq!(bare.folder, Some("/games/wow"));
    }

    #[test]
    fn a_folder_below_home_starts_with_a_tilde() {
        let home = Path::new("/home/x");
        assert_eq!(with_tilde(Path::new("/home/x/code"), home), "~/code");
        assert_eq!(with_tilde(home, home), "~");
        assert_eq!(with_tilde(Path::new("/srv/code"), home), "/srv/code");
    }

    #[test]
    fn setup_takes_the_code_folders_that_it_finds_as_the_roots() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("code/app/.git")).unwrap();

        assert_eq!(choose_roots(home.path(), None).unwrap(), ["~/code"]);
    }

    #[test]
    fn setup_with_no_code_folder_takes_no_root() {
        let home = tempfile::tempdir().unwrap();

        assert!(choose_roots(home.path(), None).unwrap().is_empty());
    }

    #[test]
    fn the_roots_line_names_each_root_or_says_to_pick_a_folder_in_the_game() {
        let home = Path::new("/home/x");
        let roots = [PathBuf::from("/home/x/code"), PathBuf::from("/srv/work")];

        assert_eq!(
            roots_line(&roots, home),
            "Agents can work in ~/code, /srv/work. To add another folder, pick it in the game."
        );
        assert_eq!(
            roots_line(&[], home),
            "Pick a project folder in the game to get started."
        );
    }

    #[test]
    fn the_roots_are_the_folders_of_the_answer_and_each_one_exists() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("code")).unwrap();
        std::fs::create_dir_all(home.path().join("work")).unwrap();

        let roots = roots_in(" ~/code, ,~/work ", home.path()).unwrap();

        assert_eq!(roots, ["~/code", "~/work"]);
        assert!(roots_in(" , ", home.path()).is_err());
        let missing = roots_in("~/none", home.path()).unwrap_err();
        assert!(
            missing.to_string().contains("~/none isn't a folder"),
            "{missing}"
        );
    }

    fn changed(new_slots: bool, key_addon: install::Installed) -> setup::Changed {
        setup::Changed {
            key_addon,
            new_slots,
        }
    }

    #[test]
    fn the_last_line_says_to_restart_the_game_for_new_folders_and_to_reload_for_new_keys() {
        use install::Installed::{New, Unchanged, Updated};
        let keep = KeyChoice::Keep;
        assert_eq!(
            all_set(&changed(true, Unchanged), keep),
            "All set. Restart WoW, then type /relay"
        );
        assert_eq!(
            all_set(&changed(false, New), keep),
            "All set. Restart WoW, then type /relay"
        );
        assert_eq!(
            all_set(&changed(false, Updated), keep),
            "All set. Type /reload in WoW"
        );
        assert_eq!(
            all_set(&changed(false, Unchanged), KeyChoice::New),
            "All set. Type /reload in WoW"
        );
        assert_eq!(all_set(&changed(false, Unchanged), keep), "All set");
    }

    #[test]
    fn a_missing_relay_addon_replaces_the_last_line_with_the_curseforge_link() {
        let new = changed(true, install::Installed::New);

        let line = relay_last_line(&new, KeyChoice::Keep, RelayAddon::Missing);

        assert_eq!(
            line,
            "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW."
        );
    }

    #[test]
    fn an_old_relay_addon_is_updated_in_the_curseforge_app_and_a_fit_one_is_all_set() {
        use protocol::version::VersionFit;
        let new = changed(true, install::Installed::New);
        let keep = KeyChoice::Keep;
        let old = RelayAddon::Installed(VersionFit::TooOld);
        let fit = RelayAddon::Installed(VersionFit::Supported);

        assert_eq!(
            relay_last_line(&new, keep, old),
            "Update Gnomish Relay in the CurseForge app, then restart WoW."
        );
        assert_eq!(
            relay_last_line(&new, keep, fit),
            "All set. Restart WoW, then type /relay"
        );
    }
}
