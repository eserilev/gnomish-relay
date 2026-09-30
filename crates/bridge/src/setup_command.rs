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
use crate::ollama_install::{self, LOCAL_STORY_MODEL};
use crate::program::find_program;
use crate::relay_addon::{self, RelayAddon};
use crate::service;
use crate::setup::{self, KeyChoice};
use crate::status::{self, SandboxFound};
use crate::timeways_install::{self, Lore, Sources};
use crate::wsl;

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
        bail!("setup needs your WoW folder. Run gnomish-relay setup <folder>");
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
    let answer = read_answer(&format!("{question} [{default}]: "))?.unwrap_or_default();
    Ok(if answer.is_empty() { default } else { &answer }.to_owned())
}

fn stdin_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// The trimmed answer to `prompt`, or `None` with no terminal.
fn read_answer(prompt: &str) -> Result<Option<String>> {
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
            "Folders the agents can work in (separate them with commas)",
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
enum Autostart {
    On,
    Off,
}

/// Whether setup installs the Timeways programs and builds the lore pack (SPEC.md 11.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimewaysInstall {
    /// `--timeways`: always, also again.
    Asked,
    /// Only with a Timeways folder and no story program in the config.
    IfMissing,
}

/// `setup [folder] [--roots a,b] [--relay] [--timeways] [--new-key] [--autostart]
/// [--no-autostart]`. The installers always add `--autostart`, so `--no-autostart` wins.
#[derive(Debug, PartialEq, Eq)]
struct SetupArgs<'a> {
    folder: Option<&'a str>,
    roots: Option<&'a str>,
    /// `--relay`, or `--roots`, which only the relay uses.
    relay_asked: bool,
    timeways: TimewaysInstall,
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
            timeways: if args.contains(&"--timeways") {
                TimewaysInstall::Asked
            } else {
                TimewaysInstall::IfMissing
            },
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
        }
    }
}

/// Every step leaves alone what works, so a second run is safe (SPEC.md 11.3).
pub fn setup(dirs: &Dirs, args: &[&str]) -> Result<()> {
    let args = SetupArgs::parse(args);
    let wow = pick_game(dirs, args.folder)?;
    if !wow.is_dir() {
        bail!("{} isn't a folder", wow.display());
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
    // The key addons and the slots first: they need nothing else, and a later step can fail.
    let changed = setup::install_files(&folders, relay, args.keys)?;
    let addon = (relay == setup::Relay::On).then(|| relay_addon::find(&folders.addons));
    let config = setup_config(dirs, &wow, existing.as_ref(), relay, timeways, args.roots)?;
    print_setup(dirs, &config, relay);
    let config = if timeways || args.timeways == TimewaysInstall::Asked {
        let config = setup_story_model(dirs, config);
        if let Some(line) = story_line(&config) {
            println!("{line}");
        }
        config
    } else {
        config
    };
    if wants_timeways_install(args.timeways, timeways, &config) {
        setup_timeways(dirs, args.autostart);
    }
    if args.autostart == Autostart::On {
        match service::autostart(dirs) {
            Ok(()) => println!("Desktop app: on, starts at login"),
            Err(e) => println!(
                "Desktop app: can't start at login ({e:#}). To start it now, run gnomish-relay run"
            ),
        }
    }
    // Setup changes no settings of an agent: they belong to the user (SPEC.md 10.5).
    if relay == setup::Relay::On {
        println!("{}", hooks_install::SETUP_HINT);
    }
    println!("{}", final_line(&changed, relay, args.keys, addon));
    Ok(())
}

fn wants_timeways_install(asked: TimewaysInstall, folder: bool, config: &Config) -> bool {
    let has_program = config.story.as_ref().is_some_and(|s| s.program.is_some());
    asked == TimewaysInstall::Asked || (folder && !has_program)
}

/// A failed step prints one line, and setup goes on (SPEC.md 11.4).
fn setup_timeways(dirs: &Dirs, autostart: Autostart) {
    use std::io::Write;
    println!(
        "Timeways: installing the story program and building its lore (a download of about 133 MB)"
    );
    let bin = std::env::var_os("GNOMISH_BIN").map(PathBuf::from);
    let places = timeways_install::Places::of(dirs, bin);
    let mut shown = None;
    let result = timeways_install::install(dirs, &Sources::from_env(), &places, |bytes| {
        let megabytes = bytes / 1_000_000;
        if shown != Some(megabytes) {
            print!("\rDownloading the Wowpedia lore: {megabytes} MB");
            let _ = std::io::stdout().flush();
            shown = Some(megabytes);
        }
    });
    if shown.is_some() {
        println!();
    }
    let lines = match result {
        Ok(report) => timeways_lines(&report, &places, autostart),
        Err(e) => vec![install_failed_line(&e)],
    };
    for line in lines {
        println!("{line}");
    }
}

const TRY_AGAIN: &str = "To try again, run gnomish-relay setup --timeways";

fn install_failed_line(error: &anyhow::Error) -> String {
    format!(
        "Timeways: couldn't install the story program. {} {TRY_AGAIN}",
        timeways_install::sentence(&format!("{error:#}"))
    )
}

fn timeways_lines(
    report: &timeways_install::Report,
    places: &timeways_install::Places,
    autostart: Autostart,
) -> Vec<String> {
    let mut lines = vec![format!(
        "Timeways story program: {} in {}",
        report.version,
        places.bin.display()
    )];
    match &report.lore {
        Lore::Built(summary) => {
            lines.extend(summary.iter().map(|line| format!("Timeways lore: {line}")));
        }
        Lore::Kept(error) => lines.push(format!(
            "Timeways lore: {} {TRY_AGAIN}",
            timeways_install::sentence(error)
        )),
    }
    if autostart == Autostart::Off {
        lines.push("To start the story program, run gnomish-relay restart".into());
    }
    lines
}

/// A player who came for Timeways says no, so no is the answer with no terminal.
fn ask_relay() -> Result<setup::Relay> {
    let answer = ask(
        "Also set up Gnomish Relay, to chat with coding agents in WoW? (y/N)",
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
    // Under WSL, a Windows agent on the PATH runs outside every wall (SPEC.md 11.5).
    let path_var = wsl::path_var();
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
            println!("Added agent: {name}. To use it, pick it in Settings in the game");
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

fn print_setup(dirs: &Dirs, config: &Config, relay: setup::Relay) {
    match &config.relay {
        Some(relay_config) => {
            for line in relay_lines(dirs, relay_config) {
                println!("{line}");
            }
            let config_file = dirs.config.join(config::FILE);
            println!("{}", setup::level_line(relay_config, &config_file));
        }
        None if relay == setup::Relay::Off => {
            println!("Coding agents: off. To turn them on, run gnomish-relay setup --relay");
        }
        None => {}
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

/// With no model, the offer of a free local model already said what to do.
fn story_line(config: &Config) -> Option<String> {
    let model = config.story.as_ref().map(|story| &story.model.choice);
    match model? {
        ModelChoice::Claude { model, .. } => Some(format!(
            "Story model: claude ({})",
            model.as_deref().unwrap_or("default")
        )),
        ModelChoice::Local(local) => Some(format!("Story model: local {}", local.model)),
        ModelChoice::None => None,
    }
}

fn story_has_model(config: &Config) -> bool {
    story_line(config).is_some()
}

const NO_MODEL: &str =
    "No AI model found. Timeways works without one, but it writes no story text.";
const OFFER: &str =
    "Install a free local model? It runs on this computer and needs about 2 GB. [Y/n]: ";
const LATER: &str = "To install it later, run gnomish-relay setup --timeways";
const LATER_IN_A_TERMINAL: &str =
    "To install a free local model, run gnomish-relay setup --timeways in a terminal";

/// An empty answer takes the default, which is yes.
fn said_yes(answer: &str) -> bool {
    answer.is_empty() || answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// A: the first model that the player has. C: with none, the offer of a free local
/// model (SPEC.md 11.6). A failed step prints one line, and setup goes on.
fn setup_story_model(dirs: &Dirs, config: Config) -> Config {
    if story_has_model(&config) {
        return config;
    }
    let models = model_setup::find_models(&wsl::path_var());
    let Some(found) = models.first() else {
        return offer_local_model(dirs, config);
    };
    match setup::write_story_model(&dirs.config, found, &dirs.home) {
        Ok(new) => new,
        Err(e) => {
            println!("Story model: couldn't set it. {}", sentence_of(&e));
            config
        }
    }
}

fn sentence_of(error: &anyhow::Error) -> String {
    timeways_install::sentence(&format!("{error:#}"))
}

/// Never a download of 2 GB with no yes, so no terminal means no.
fn offer_local_model(dirs: &Dirs, config: Config) -> Config {
    let os = ollama_install::Os::this();
    println!("{NO_MODEL}");
    if !stdin_is_terminal() {
        println!("{LATER_IN_A_TERMINAL}");
        return config;
    }
    println!(
        "Setup can install Ollama with its official installer: {}",
        ollama_install::installer(os).shown
    );
    let answer = match read_answer(OFFER) {
        Ok(answer) => answer.unwrap_or_default(),
        Err(_) => "n".into(),
    };
    if !said_yes(&answer) {
        println!("{LATER}");
        return config;
    }
    match install_local_model(dirs, os) {
        Ok(new) => new,
        Err(e) => {
            println!(
                "Story model: couldn't install the free local model. {} {TRY_AGAIN}",
                sentence_of(&e)
            );
            config
        }
    }
}

/// Only a failed test of the model leaves it in the config: it's installed.
fn install_local_model(dirs: &Dirs, os: ollama_install::Os) -> Result<Config> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let curl = find_program("curl", &path, cfg!(windows)).context("Couldn't find curl")?;
    let ollama = ollama_install::Ollama {
        curl,
        url: model_setup::OLLAMA.into(),
    };
    if ollama_install::server_answers(&ollama) {
        println!("Ollama is already running.");
    } else {
        install_ollama(&ollama, os)?;
    }
    pull_with_progress(&ollama)?;
    let model = model_setup::FoundModel::Local {
        url: ollama.url.clone(),
        model: LOCAL_STORY_MODEL.into(),
    };
    let config = setup::write_story_model(&dirs.config, &model, &dirs.home)?;
    println!("Testing the model…");
    if let Err(e) = ollama_install::check_answer(&config) {
        println!(
            "Story model: {LOCAL_STORY_MODEL} is installed, but it didn't answer a test. {} \
             Check that Ollama is running, then run gnomish-relay restart",
            sentence_of(&e)
        );
    }
    Ok(config)
}

/// The installer goes into a private temp folder that goes away after the install.
fn install_ollama(ollama: &ollama_install::Ollama, os: ollama_install::Os) -> Result<()> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("gnomish-relay-ollama-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let folder = builder.tempdir().context("Couldn't make a temp folder")?;
    println!("Downloading the Ollama installer…");
    let installer = ollama_install::download_installer(&ollama.curl, os, folder.path())?;
    match os {
        ollama_install::Os::LinuxOrMac => {
            println!("Installing Ollama. It can ask for your password.");
        }
        ollama_install::Os::Windows => println!("Installing Ollama…"),
    }
    ollama_install::install_and_start(ollama, installer, ollama_install::START_WAIT)
}

fn pull_with_progress(ollama: &ollama_install::Ollama) -> Result<()> {
    use std::io::Write;
    println!("Downloading {LOCAL_STORY_MODEL}…");
    let mut shown = None;
    let result = ollama_install::pull(ollama, LOCAL_STORY_MODEL, &mut |done, all| {
        if shown != Some(done) {
            print!("\rDownloading {LOCAL_STORY_MODEL}: {done} MB of {all} MB");
            let _ = std::io::stdout().flush();
            shown = Some(done);
        }
    });
    if shown.is_some() {
        println!();
    }
    result
}

/// A missing or unfit relay addon comes last, so it is also the last line of the
/// installers (SPEC.md 11.3).
fn final_line(
    changed: &setup::Changed,
    relay: setup::Relay,
    keys: KeyChoice,
    addon: Option<RelayAddon>,
) -> &'static str {
    addon
        .and_then(relay_addon::next_step)
        .unwrap_or_else(|| last_line(changed, relay, keys))
}

/// WoW finds a new addon folder only at launch, and a new key only after a `/reload`.
fn last_line(changed: &setup::Changed, relay: setup::Relay, keys: KeyChoice) -> &'static str {
    let parts = [changed.relay_key.as_ref(), changed.timeways_key.as_ref()];
    if changed.new_slots || parts.contains(&Some(&install::Installed::New)) {
        return match relay {
            setup::Relay::On => "All set. Restart WoW, then type /relay",
            setup::Relay::Off => "All set. Restart WoW to load the addon",
        };
    }
    if parts.contains(&Some(&install::Installed::Updated)) || keys == KeyChoice::New {
        return "All set. Type /reload in WoW";
    }
    "All set"
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
            "Made {} addon files for {app:?} in {}",
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
    fn an_empty_answer_or_yes_installs_the_local_model_and_any_other_answer_does_not() {
        for yes in ["", "y", "Y", "yes", "YES"] {
            assert!(said_yes(yes), "{yes:?}");
        }
        for no in ["n", "no", "N", "maybe", "yess"] {
            assert!(!said_yes(no), "{no:?}");
        }
    }

    #[test]
    fn the_offer_names_the_size_and_the_default_and_each_no_names_the_command_for_later() {
        assert!(OFFER.contains("about 2 GB"));
        assert!(OFFER.ends_with("[Y/n]: "));
        assert!(LATER.ends_with("run gnomish-relay setup --timeways"));
        assert!(LATER_IN_A_TERMINAL.contains("run gnomish-relay setup --timeways in a terminal"));
    }

    #[test]
    fn the_story_line_names_the_model_and_says_nothing_with_no_model() {
        let home = tempfile::tempdir().unwrap();
        let wow = home.path().join("wow");
        let parse = |text: &str| config::parse(text, home.path()).unwrap();
        let none = parse(&crate::config_text::timeways_config(&wow, &[]));
        let local = parse(&crate::config_text::timeways_config(
            &wow,
            &[model_setup::FoundModel::Local {
                url: model_setup::OLLAMA.into(),
                model: LOCAL_STORY_MODEL.into(),
            }],
        ));

        assert_eq!(story_line(&none), None);
        assert!(!story_has_model(&none));
        assert_eq!(
            story_line(&local).as_deref(),
            Some("Story model: local llama3.2:3b")
        );
    }

    #[test]
    fn setup_takes_the_folder_the_roots_and_the_flags_in_any_order() {
        let args = SetupArgs::parse(&["--roots", "~/a,~/b", "/games/wow", "--autostart"]);
        assert_eq!(
            args,
            SetupArgs {
                folder: Some("/games/wow"),
                roots: Some("~/a,~/b"),
                relay_asked: true,
                timeways: TimewaysInstall::IfMissing,
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
    fn the_installer_adds_autostart_and_no_autostart_wins() {
        let args = SetupArgs::parse(&["--autostart", "--timeways", "--no-autostart"]);
        assert_eq!(args.autostart, Autostart::Off);
        assert_eq!(args.timeways, TimewaysInstall::Asked);
        assert!(!args.relay_asked);
    }

    fn story_config(program: bool) -> Config {
        let home = tempfile::tempdir().unwrap();
        let mut text = "[wow]\npath = \"~/wow\"\n[story]\n".to_owned();
        if program {
            text.push_str("program = \"~/s\"\nlore_pack = \"~/l\"\n");
        }
        config::parse(&text, home.path()).unwrap()
    }

    #[test]
    fn setup_installs_timeways_when_asked_or_when_its_program_is_missing() {
        let with = story_config(true);
        let without = story_config(false);
        assert!(wants_timeways_install(TimewaysInstall::Asked, false, &with));
        assert!(wants_timeways_install(
            TimewaysInstall::IfMissing,
            true,
            &without
        ));
        assert!(!wants_timeways_install(
            TimewaysInstall::IfMissing,
            true,
            &with
        ));
        assert!(!wants_timeways_install(
            TimewaysInstall::IfMissing,
            false,
            &without
        ));
    }

    #[test]
    fn the_timeways_lines_name_the_version_the_lore_and_the_next_step() {
        let places = timeways_install::Places {
            bin: PathBuf::from("/b"),
            pack: PathBuf::from("/p"),
            work: PathBuf::from("/w"),
        };
        let built = timeways_install::Report {
            version: "0.1.0".into(),
            changed: vec![],
            lore: Lore::Built(vec!["read 9 pages, skipped 1".into()]),
        };
        let kept = timeways_install::Report {
            lore: Lore::Kept("Couldn't build the Timeways lore. Your old lore stays.".into()),
            ..built
        };

        let lines = timeways_lines(&kept, &places, Autostart::On);
        assert_eq!(
            lines,
            [
                "Timeways story program: 0.1.0 in /b",
                "Timeways lore: Couldn't build the Timeways lore. Your old lore stays. To try again, run gnomish-relay setup --timeways",
            ]
        );
        let off = timeways_lines(&kept, &places, Autostart::Off);
        assert_eq!(
            off.last().unwrap(),
            "To start the story program, run gnomish-relay restart"
        );
    }

    #[test]
    fn a_lore_error_with_no_period_stays_apart_from_the_next_step() {
        let places = timeways_install::Places {
            bin: PathBuf::from("/b"),
            pack: PathBuf::from("/p"),
            work: PathBuf::from("/w"),
        };
        let kept = timeways_install::Report {
            version: "0.1.0".into(),
            changed: vec![],
            lore: Lore::Kept("the download of the Wowpedia lore failed".into()),
        };

        let lines = timeways_lines(&kept, &places, Autostart::On);

        assert_eq!(
            lines[1],
            "Timeways lore: The download of the Wowpedia lore failed. To try again, run gnomish-relay setup --timeways"
        );
    }

    #[test]
    fn a_failed_timeways_install_gives_the_error_and_then_the_next_step() {
        let error = anyhow::anyhow!("the download of timeways-x.tar.gz has a wrong SHA-256 sum");

        assert_eq!(
            install_failed_line(&error),
            "Timeways: couldn't install the story program. The download of timeways-x.tar.gz has a wrong SHA-256 sum. To try again, run gnomish-relay setup --timeways"
        );
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
            missing.to_string().contains("~/none isn't a folder"),
            "{missing}"
        );
    }

    fn changed(new_slots: bool, relay_key: Option<install::Installed>) -> setup::Changed {
        setup::Changed {
            new_slots,
            relay_key,
            timeways_key: None,
        }
    }

    #[test]
    fn the_last_line_says_to_restart_the_game_for_new_folders_and_to_reload_for_new_keys() {
        let on = setup::Relay::On;
        let keep = KeyChoice::Keep;
        assert_eq!(
            last_line(&changed(true, None), on, keep),
            "All set. Restart WoW, then type /relay"
        );
        assert_eq!(
            last_line(&changed(true, None), setup::Relay::Off, keep),
            "All set. Restart WoW to load the addon"
        );
        let updated = changed(false, Some(install::Installed::Updated));
        assert_eq!(
            last_line(&updated, on, keep),
            "All set. Type /reload in WoW"
        );
        assert_eq!(
            last_line(&changed(false, None), on, KeyChoice::New),
            "All set. Type /reload in WoW"
        );
        assert_eq!(last_line(&changed(false, None), on, keep), "All set");
        let new_timeways_key = setup::Changed {
            timeways_key: Some(install::Installed::New),
            ..changed(false, None)
        };
        assert_eq!(
            last_line(&new_timeways_key, setup::Relay::Off, keep),
            "All set. Restart WoW to load the addon"
        );
    }

    #[test]
    fn a_missing_relay_addon_replaces_the_last_line_with_the_curseforge_link() {
        let new = changed(true, Some(install::Installed::New));

        let line = final_line(
            &new,
            setup::Relay::On,
            KeyChoice::Keep,
            Some(RelayAddon::Missing),
        );

        assert_eq!(
            line,
            "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW."
        );
    }

    #[test]
    fn an_old_relay_addon_is_updated_in_the_curseforge_app_and_a_fit_one_is_all_set() {
        use protocol::version::VersionFit;
        let new = changed(true, Some(install::Installed::New));
        let (on, keep) = (setup::Relay::On, KeyChoice::Keep);
        let old = Some(RelayAddon::Installed(VersionFit::TooOld));
        let fit = Some(RelayAddon::Installed(VersionFit::Supported));

        assert_eq!(
            final_line(&new, on, keep, old),
            "Update Gnomish Relay in the CurseForge app, then restart WoW."
        );
        assert_eq!(
            final_line(&new, on, keep, fit),
            "All set. Restart WoW, then type /relay"
        );
        assert_eq!(
            final_line(&new, setup::Relay::Off, keep, None),
            "All set. Restart WoW to load the addon"
        );
    }
}
