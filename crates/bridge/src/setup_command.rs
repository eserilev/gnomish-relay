//! The `gnomish-relay setup` command: the questions in the terminal, and the lines that
//! it prints. `setup.rs` holds the steps that need no terminal.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::check_agent;
use crate::config::{self, Config, RelayConfig};
use crate::config_text::RelayPart;
use crate::dirs::Dirs;
use crate::hooks_install;
use crate::install;
use crate::model::ModelChoice;
use crate::model_setup;
use crate::service;
use crate::setup::{self, KeyChoice};
use crate::status::{self, SandboxFound};

/// The game folder: the one given, the one found, or the answer to a question.
pub fn pick_game(dirs: &Dirs, given: Option<&str>) -> Result<PathBuf> {
    if let Some(folder) = given {
        return Ok(install::game_folder(folder));
    }
    let games = install::find_games(&dirs.home);
    if let [game] = games.as_slice() {
        return Ok(game.clone());
    }
    for (n, game) in games.iter().enumerate() {
        println!("{}. {}", n + 1, game.display());
    }
    let answer = ask("WoW folder", if games.is_empty() { "" } else { "1" })?;
    if answer.is_empty() {
        bail!("give the WoW folder: gnomish-relay setup <folder>");
    }
    Ok(chosen_game(&answer, &games))
}

/// A number picks a listed game. Anything else is a folder.
fn chosen_game(answer: &str, games: &[PathBuf]) -> PathBuf {
    let listed = answer
        .parse::<usize>()
        .ok()
        .and_then(|n| games.get(n.checked_sub(1)?));
    listed
        .cloned()
        .unwrap_or_else(|| install::game_folder(answer))
}

/// Reads one answer in a terminal. With no terminal, or an empty answer, the default.
fn ask(question: &str, default: &str) -> Result<String> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return Ok(default.to_owned());
    }
    print!("{question} [{default}]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    Ok(if answer.is_empty() { default } else { answer }.to_owned())
}

/// `~/code` reads better in the config than the full path.
fn with_tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The folders of code projects that setup finds, or the home folder.
fn choose_roots(home: &Path, given: Option<&str>) -> Result<Vec<String>> {
    let found: Vec<String> = install::suggest_roots(home)
        .iter()
        .map(|p| with_tilde(p, home))
        .collect();
    let answer = match given {
        Some(list) => list.to_owned(),
        // No default of the home folder: it holds ~/.ssh and the browser profiles.
        None => ask(
            "Folders the agents can work in, divided by commas",
            &found.join(", "),
        )?,
    };
    roots_in(&answer, home)
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
        bail!("give the folders that the agents can work in: gnomish-relay setup --roots ~/code");
    }
    for root in &roots {
        if !config::expand(root, home)?.is_dir() {
            bail!("{root} is not a folder");
        }
    }
    Ok(roots)
}

fn option<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| *a == name)?;
    args.get(at + 1).copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Autostart {
    On,
    Off,
}

/// `setup [folder] [--roots a,b] [--relay] [--new-key] [--autostart]`.
#[derive(Debug, PartialEq, Eq)]
struct SetupArgs<'a> {
    folder: Option<&'a str>,
    roots: Option<&'a str>,
    /// `--relay`, or `--roots`, which only the relay uses.
    relay_asked: bool,
    keys: KeyChoice,
    autostart: Autostart,
}

impl<'a> SetupArgs<'a> {
    fn parse(args: &[&'a str]) -> SetupArgs<'a> {
        let roots = option(args, "--roots");
        let folder = args
            .iter()
            .copied()
            .find(|a| !a.starts_with("--") && Some(*a) != roots);
        SetupArgs {
            folder,
            roots,
            relay_asked: args.contains(&"--relay") || roots.is_some(),
            keys: if args.contains(&"--new-key") {
                KeyChoice::New
            } else {
                KeyChoice::Keep
            },
            autostart: if args.contains(&"--autostart") {
                Autostart::On
            } else {
                Autostart::Off
            },
        }
    }
}

/// Every step leaves alone what works, so a second run is safe (SPEC.md 11.3).
pub fn setup(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let args = SetupArgs::parse(args);
    let wow = pick_game(dirs, args.folder)?;
    if !wow.is_dir() {
        bail!("{} is not a folder", wow.display());
    }
    // WoW makes Interface/AddOns at its first start. Setup makes it earlier.
    let addons = install::addons_dir(&wow);
    std::fs::create_dir_all(&addons)
        .with_context(|| format!("cannot make {}", addons.display()))?;
    println!("WoW: {}", wow.display());
    let existing = match std::fs::read_to_string(dirs.config.join(config::FILE)) {
        Ok(text) => Some((text, config::load(&dirs.config, &dirs.home)?)),
        Err(_) => None,
    };
    let timeways = install::timeways_dir(&addons).is_some();
    let found = setup::Found {
        relay_asked: args.relay_asked,
        config_has_relay: existing.as_ref().map(|(_, c)| c.relay.is_some()),
        relay_folder: addons.join(install::ADDON).exists(),
        timeways_folder: timeways,
    };
    let relay = match setup::relay_choice(&found) {
        setup::RelayChoice::Decided(relay) => relay,
        setup::RelayChoice::Ask => ask_relay()?,
    };
    let folders = setup::Folders {
        config: dirs.config.clone(),
        addons,
    };
    // The addon and the slots first: they need nothing else, and a later step can fail.
    let changed = setup::install_files(&folders, relay, args.keys)?;
    let config = setup_config(dirs, &wow, existing.as_ref(), relay, timeways, args.roots)?;
    print_setup(dirs, &config, relay, timeways);
    if args.autostart == Autostart::On {
        match service::autostart(dirs) {
            Ok(()) => println!("Bridge: on, starts at login"),
            Err(e) => println!("Bridge: not started at login ({e:#}). Run: gnomish-relay run"),
        }
    }
    println!("{}", last_line(&changed, relay, args.keys));
    // Setup changes no settings of an agent: they belong to the user (SPEC.md 10.5).
    if relay == setup::Relay::On {
        println!("{}", hooks_install::SETUP_HINT);
    }
    Ok(())
}

/// A player who came for Timeways says no, so no is the answer with no terminal.
fn ask_relay() -> Result<setup::Relay> {
    let answer = ask(
        "Also set up Gnomish Relay, coding agents in the game? (y/N)",
        "n",
    )?;
    Ok(if answer.eq_ignore_ascii_case("y") {
        setup::Relay::On
    } else {
        setup::Relay::Off
    })
}

/// Writes the first config, or adds the part that it lacks: the relay with `--relay`,
/// and `[story]` when the Timeways addon is there.
fn setup_config(
    dirs: &Dirs,
    wow: &Path,
    existing: Option<&(String, Config)>,
    relay: setup::Relay,
    timeways: bool,
    roots_given: Option<&str>,
) -> Result<Config> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    let lacks_story = existing.is_none_or(|(_, c)| c.story.is_none());
    let agents = install::find_agents(&path_var);
    let roots = if relay == setup::Relay::On && lacks_relay {
        choose_roots(&dirs.home, roots_given)?
    } else {
        Vec::new()
    };
    let harnesses = if roots.is_empty() {
        Vec::new()
    } else {
        choose_harnesses(&path_var)?
    };
    let wants_story = timeways && lacks_story;
    // A local model is also for the agents: the relay part opens its port.
    let models = if wants_story || !roots.is_empty() {
        model_setup::find_models(&path_var)
    } else {
        Vec::new()
    };
    let local_ports = model_setup::local_ports(&models);
    let new_agents = setup::new_agents(&agents, existing.map(|(_, config)| config));
    let parts = setup::ConfigParts {
        wow,
        relay: (!roots.is_empty()).then_some(RelayPart {
            agents: &agents,
            harnesses: &harnesses,
            roots: &roots,
            local_ports: &local_ports,
        }),
        new_agents: &new_agents,
        story: wants_story.then_some(models.as_slice()),
    };
    let text = existing.map(|(text, _)| text.as_str());
    let config = match setup::config_text(text, &parts) {
        Some(new) => setup::write_config(&dirs.config, &new, &dirs.home)?,
        None => config::load(&dirs.config, &dirs.home)?,
    };
    let added = config.relay.as_ref().map(|relay| &relay.agents);
    for (name, _, _) in &new_agents {
        if added.is_some_and(|agents| agents.contains_key(*name)) {
            println!("Added agent: {name}. Pick it for a new chat in the game, in Settings");
        }
    }
    Ok(config)
}

/// A harness with no ACP mode runs its own commands, so setup adds it only on a yes.
fn choose_harnesses(path_var: &std::ffi::OsStr) -> Result<Vec<&'static str>> {
    let mut chosen = Vec::new();
    for name in install::find_harnesses(path_var) {
        let answer = ask(
            &format!(
                "Found {name}. Add it as an agent? It runs its own commands with no question, inside the sandbox. (y/N)"
            ),
            "n",
        )?;
        if answer.eq_ignore_ascii_case("y") {
            chosen.push(name);
        }
    }
    Ok(chosen)
}

fn print_setup(dirs: &Dirs, config: &Config, relay: setup::Relay, timeways: bool) {
    match &config.relay {
        Some(relay_config) => {
            for line in relay_lines(dirs, relay_config) {
                println!("{line}");
            }
            let config_file = dirs.config.join(config::FILE);
            println!("{}", setup::level_line(relay_config, &config_file));
        }
        None if relay == setup::Relay::Off => {
            println!("Gnomish Relay: off. To add coding agents: gnomish-relay setup --relay");
        }
        None => {}
    }
    if timeways {
        println!("{}", story_line(config));
    }
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

fn story_line(config: &Config) -> String {
    let model = config.story.as_ref().map(|story| &story.model.choice);
    match model {
        Some(ModelChoice::Claude { model, .. }) => format!(
            "Story model: claude ({})",
            model.as_deref().unwrap_or("default")
        ),
        Some(ModelChoice::Local(local)) => format!("Story model: local {}", local.model),
        _ => {
            "Story model: none. Set model in [story] of the config, then run: gnomish-relay restart"
                .into()
        }
    }
}

/// WoW finds a new addon folder only at launch, and a new key only after a `/reload`.
fn last_line(changed: &setup::Changed, relay: setup::Relay, keys: KeyChoice) -> &'static str {
    let new_relay = changed.relay_addon == Some(install::Installed::New);
    if changed.new_slots || new_relay {
        return match relay {
            setup::Relay::On => "Restart WoW, then type /relay",
            setup::Relay::Off => "Restart WoW, then log in",
        };
    }
    let updated = [changed.relay_addon.as_ref(), changed.timeways_key.as_ref()]
        .contains(&Some(&install::Installed::Updated));
    if updated || keys == KeyChoice::New {
        return "Type /reload in WoW";
    }
    "Ready"
}

/// `gnomish-relay install`: the slots of each app of the config.
pub fn install_slots(dirs: &Dirs) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let dir = install::addons_dir(&config.wow);
    let relay = match config.relay {
        Some(_) => setup::Relay::On,
        None => setup::Relay::Off,
    };
    for app in setup::install_all_slots(&dir, relay)? {
        println!(
            "made {} slots of {app:?} in {}",
            protocol::slot::SLOTS,
            dir.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_takes_the_folder_the_roots_and_the_flags_in_any_order() {
        let args = SetupArgs::parse(&["--roots", "~/a,~/b", "/games/wow", "--autostart"]);
        assert_eq!(
            args,
            SetupArgs {
                folder: Some("/games/wow"),
                roots: Some("~/a,~/b"),
                relay_asked: true,
                keys: KeyChoice::Keep,
                autostart: Autostart::On,
            }
        );
        let plain = SetupArgs::parse(&["--new-key"]);
        assert_eq!(plain.folder, None);
        assert!(!plain.relay_asked);
        assert_eq!(plain.keys, KeyChoice::New);
        assert_eq!(plain.autostart, Autostart::Off);
    }

    #[test]
    fn a_number_picks_a_listed_game_and_other_text_names_a_folder() {
        let root = tempfile::tempdir().unwrap();
        let games = [root.path().join("a"), root.path().join("b")];
        assert_eq!(chosen_game("2", &games), games[1]);
        let typed = root.path().join("c").to_string_lossy().into_owned();
        assert_eq!(chosen_game(&typed, &games), install::game_folder(&typed));
        assert_eq!(chosen_game("0", &games), install::game_folder("0"));
    }

    #[test]
    fn a_folder_below_home_starts_with_a_tilde() {
        let home = Path::new("/home/x");
        assert_eq!(with_tilde(Path::new("/home/x/code"), home), "~/code");
        assert_eq!(with_tilde(home, home), "~");
        assert_eq!(with_tilde(Path::new("/srv/code"), home), "/srv/code");
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
            missing.to_string().contains("~/none is not a folder"),
            "{missing}"
        );
    }

    fn changed(new_slots: bool, relay_addon: Option<install::Installed>) -> setup::Changed {
        setup::Changed {
            new_slots,
            relay_addon,
            timeways_key: None,
        }
    }

    #[test]
    fn the_last_line_says_to_restart_the_game_for_new_folders_and_to_reload_for_new_keys() {
        let on = setup::Relay::On;
        let keep = KeyChoice::Keep;
        assert_eq!(
            last_line(&changed(true, None), on, keep),
            "Restart WoW, then type /relay"
        );
        assert_eq!(
            last_line(&changed(true, None), setup::Relay::Off, keep),
            "Restart WoW, then log in"
        );
        let updated = changed(false, Some(install::Installed::Updated));
        assert_eq!(last_line(&updated, on, keep), "Type /reload in WoW");
        assert_eq!(
            last_line(&changed(false, None), on, KeyChoice::New),
            "Type /reload in WoW"
        );
        assert_eq!(last_line(&changed(false, None), on, keep), "Ready");
    }
}
