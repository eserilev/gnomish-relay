//! The `gnomish-relay setup` command: the questions in the terminal, and the lines that
//! it prints. `setup.rs` holds the steps that need no terminal.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::check_agent;
use crate::config::{self, Config, RelayConfig, with_tilde};
use crate::config_text::RelayPart;
use crate::dirs::Dirs;
use crate::game_choice;
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

/// `setup [--wow folder] [--roots a,b] [--relay] [--timeways] [--new-key] [--autostart]
/// [--no-autostart]`. The installers always add `--autostart`, so `--no-autostart` wins.
/// The folder can also come with no `--wow`, as before 0.3.1.
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
        let wow = option(args, "--wow");
        let bare = args
            .iter()
            .copied()
            .find(|a| !a.starts_with("--") && Some(*a) != roots && Some(*a) != wow);
        SetupArgs {
            folder: wow.or(bare),
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
    let existing = match std::fs::read_to_string(dirs.config.join(config::FILE)) {
        Ok(text) => Some((text, config::load(&dirs.config, &dirs.home)?)),
        Err(_) => None,
    };
    let config_wow = existing
        .as_ref()
        .and_then(|(_, config)| config.wow.as_deref());
    let game = game_choice::choose(args.folder, config_wow, &dirs.home);
    let wow = game.folder();
    let addons = wow.map(addons_of).transpose()?;
    if let Some(line) = game_choice::game_line(&game) {
        println!("{line}");
    }
    let timeways_folder = addons
        .as_deref()
        .is_some_and(|addons| install::timeways_dir(addons).is_some());
    let timeways = setup::timeways_choice(timeways_folder, args.timeways == TimewaysInstall::Asked);
    let found = setup::Found {
        relay_asked: args.relay_asked,
        config_has_relay: existing.as_ref().map(|(_, c)| c.relay.is_some()),
        relay_folder: addons
            .as_deref()
            .is_some_and(|addons| addons.join(install::ADDON).exists()),
        timeways,
    };
    // Every question comes before the first file, so a stop at a question leaves nothing
    // half done.
    let answers = ask_all(&found, existing.as_ref())?;
    let relay = answers.relay;
    // The key addons and the slots first: they need nothing else, and a later step can fail.
    let game_files = setup_files(dirs, addons, relay, timeways, args.keys)?;
    let config = setup_config(dirs, wow, existing.as_ref(), &answers, timeways, args.roots)?;
    print_setup(dirs, &config, relay);
    let config = if timeways == setup::Timeways::On {
        let config = setup_story_model(dirs, config, answers.local_model);
        if let Some(line) = story_line(&config) {
            println!("{line}");
        }
        config
    } else {
        config
    };
    if wants_timeways_install(args.timeways, timeways_folder, &config) {
        setup_timeways(dirs, args.autostart);
    }
    if args.autostart == Autostart::On {
        let start = match wow {
            Some(_) => service::Start::Now,
            None => service::Start::AtLogin,
        };
        match service::autostart(dirs, start) {
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
    let Some((changed, addon)) = game_files else {
        println!("{}", game_choice::NO_WOW);
        return Ok(());
    };
    let get_timeways = timeways_step(timeways, timeways_folder);
    for line in final_lines(&changed, relay, args.keys, addon, get_timeways) {
        println!("{line}");
    }
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

/// The keys, and with a game its files and the relay addon that setup found there.
fn setup_files(
    dirs: &Dirs,
    addons: Option<PathBuf>,
    relay: setup::Relay,
    timeways: setup::Timeways,
    keys: KeyChoice,
) -> Result<Option<(setup::Changed, Option<RelayAddon>)>> {
    let Some(addons) = addons else {
        setup::make_keys(&dirs.config, timeways, keys)?;
        return Ok(None);
    };
    let folders = setup::Folders {
        config: dirs.config.clone(),
        addons,
    };
    let changed = setup::install_files(&folders, relay, timeways, keys)?;
    let addon = (relay == setup::Relay::On).then(|| relay_addon::find(&folders.addons));
    Ok(Some((changed, addon)))
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

/// A failed download names its reason, never the command line of `curl`.
fn install_failed_line(error: &anyhow::Error) -> String {
    if let Some(line) =
        timeways_install::download_failed_line(error, "run gnomish-relay setup --timeways")
    {
        return line;
    }
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

/// The answers to the yes or no questions of setup. Setup asks no path question.
struct Answers {
    relay: setup::Relay,
    /// The harnesses with no ACP mode that the player added.
    harnesses: Vec<&'static str>,
    local_model: LocalModel,
}

/// Whether setup installs a free local model for Timeways (SPEC.md 11.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalModel {
    Install,
    Skip,
}

fn ask_all(found: &setup::Found, existing: Option<&(String, Config)>) -> Result<Answers> {
    let relay = match setup::relay_choice(found) {
        setup::RelayChoice::Decided(relay) => relay,
        setup::RelayChoice::Ask => ask_relay()?,
    };
    // Under WSL, a Windows agent on the PATH runs outside every wall (SPEC.md 11.5).
    let path_var = wsl::path_var();
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    let harnesses = if relay == setup::Relay::On && lacks_relay {
        choose_harnesses(&path_var)?
    } else {
        Vec::new()
    };
    let has_model = existing.is_some_and(|(_, config)| story_has_model(config));
    let no_model = found.timeways == setup::Timeways::On
        && !has_model
        && model_setup::find_models(&path_var).is_empty();
    let local_model = if no_model {
        ask_local_model()
    } else {
        LocalModel::Skip
    };
    Ok(Answers {
        relay,
        harnesses,
        local_model,
    })
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
    wow: Option<&Path>,
    existing: Option<&(String, Config)>,
    answers: &Answers,
    timeways: setup::Timeways,
    roots_given: Option<&str>,
) -> Result<Config> {
    // Under WSL, a Windows agent on the PATH runs outside every wall (SPEC.md 11.5).
    let path_var = wsl::path_var();
    let lacks_relay = existing.is_none_or(|(_, c)| c.relay.is_none());
    let lacks_story = existing.is_none_or(|(_, c)| c.story.is_none());
    let agents = install::find_agents(&path_var);
    let roots = if answers.relay == setup::Relay::On && lacks_relay {
        Some(choose_roots(&dirs.home, roots_given)?)
    } else {
        None
    };
    let wants_story = timeways == setup::Timeways::On && lacks_story;
    // A local model is also for the agents: the relay part opens its port.
    let models = if wants_story || roots.is_some() {
        model_setup::find_models(&path_var)
    } else {
        Vec::new()
    };
    let local_ports = model_setup::local_ports(&models);
    let new_agents = setup::new_agents(&agents, existing.map(|(_, config)| config));
    let old_wow = existing.and_then(|(_, config)| config.wow.as_deref());
    let parts = setup::ConfigParts {
        wow: wow.filter(|wow| old_wow != Some(*wow)),
        relay: roots.as_deref().map(|roots| RelayPart {
            agents: &agents,
            harnesses: &answers.harnesses,
            roots,
            local_ports: &local_ports,
        }),
        new_agents: &new_agents,
        story: wants_story.then_some(models.as_slice()),
    };
    let text = existing.map(|(text, _)| text.as_str());
    let config = match setup::config_text(text, &parts)? {
        Some(new) => setup::write_config(&dirs.config, &new, &dirs.home)?,
        None => config::load(&dirs.config, &dirs.home)?,
    };
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

fn print_setup(dirs: &Dirs, config: &Config, relay: setup::Relay) {
    match &config.relay {
        Some(relay_config) => {
            for line in relay_lines(dirs, relay_config) {
                println!("{line}");
            }
            println!("{}", setup::level_line(relay_config));
            println!(
                "{}",
                roots_line(&shown_roots(relay_config), &real_home(dirs))
            );
        }
        None if relay == setup::Relay::Off => {
            println!("Coding agents: off. To turn them on, run gnomish-relay setup --relay");
        }
        None => {}
    }
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

/// A: the first model that the player has. C: with none, the free local model on a yes
/// (SPEC.md 11.6). A failed step prints one line, and setup goes on.
fn setup_story_model(dirs: &Dirs, config: Config, local_model: LocalModel) -> Config {
    if story_has_model(&config) {
        return config;
    }
    let models = model_setup::find_models(&wsl::path_var());
    let Some(found) = models.first() else {
        return match local_model {
            LocalModel::Install => install_free_model(dirs, config),
            LocalModel::Skip => config,
        };
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
fn ask_local_model() -> LocalModel {
    println!("{NO_MODEL}");
    if !stdin_is_terminal() {
        println!("{LATER_IN_A_TERMINAL}");
        return LocalModel::Skip;
    }
    println!(
        "Setup can install Ollama with its official installer: {}",
        ollama_install::installer(ollama_install::Os::this()).shown
    );
    let answer = match read_answer(OFFER) {
        Ok(answer) => answer.unwrap_or_default(),
        Err(_) => "n".into(),
    };
    if !said_yes(&answer) {
        println!("{LATER}");
        return LocalModel::Skip;
    }
    LocalModel::Install
}

fn install_free_model(dirs: &Dirs, config: Config) -> Config {
    match install_local_model(dirs, ollama_install::Os::this()) {
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

// TODO: add the link when the Timeways project on CurseForge has one.
const GET_TIMEWAYS: &str = "Get the Timeways addon on CurseForge, then restart WoW.";

/// Setup for Timeways can come before its addon, which players get from `CurseForge`.
fn timeways_step(timeways: setup::Timeways, folder: bool) -> Option<&'static str> {
    (timeways == setup::Timeways::On && !folder).then_some(GET_TIMEWAYS)
}

/// A missing addon comes last, so it is also the last line of the installers (SPEC.md
/// 11.3). The next step of each addon replaces "All set".
fn final_lines(
    changed: &setup::Changed,
    relay: setup::Relay,
    keys: KeyChoice,
    addon: Option<RelayAddon>,
    timeways_step: Option<&'static str>,
) -> Vec<&'static str> {
    let steps: Vec<&'static str> = [addon.and_then(relay_addon::next_step), timeways_step]
        .into_iter()
        .flatten()
        .collect();
    if steps.is_empty() {
        return vec![last_line(changed, relay, keys)];
    }
    steps
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
    let dir = install::addons_dir(config.game()?);
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
        let none = parse(&crate::config_text::timeways_config(Some(&wow), &[]));
        let local = parse(&crate::config_text::timeways_config(
            Some(&wow),
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
    fn a_failed_download_of_the_story_program_says_why_and_never_shows_curl() {
        let error = anyhow::Error::new(crate::download_failure::DownloadFailed {
            reason: crate::download_failure::Reason::Missing,
            details: "curl -fsSL https://x failed".into(),
        });

        let line = install_failed_line(&error);

        assert_eq!(
            line,
            "Timeways: couldn't download the story program (the release isn't published yet). To try again later, run gnomish-relay setup --timeways"
        );
    }

    #[test]
    fn setup_takes_the_wow_folder_with_or_with_no_flag() {
        let flag = SetupArgs::parse(&["--relay", "--wow", "/games/wow", "--roots", "~/a"]);
        let bare = SetupArgs::parse(&["/games/wow", "--relay"]);

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

        let lines = final_lines(
            &new,
            setup::Relay::On,
            KeyChoice::Keep,
            Some(RelayAddon::Missing),
            None,
        );

        assert_eq!(
            lines,
            [
                "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW."
            ]
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
            final_lines(&new, on, keep, old, None),
            ["Update Gnomish Relay in the CurseForge app, then restart WoW."]
        );
        assert_eq!(
            final_lines(&new, on, keep, fit, None),
            ["All set. Restart WoW, then type /relay"]
        );
        assert_eq!(
            final_lines(&new, setup::Relay::Off, keep, None, None),
            ["All set. Restart WoW to load the addon"]
        );
    }

    #[test]
    fn setup_for_timeways_with_no_timeways_folder_ends_with_the_curseforge_step() {
        let new = changed(false, Some(install::Installed::New));
        let (off, keep) = (setup::Relay::Off, KeyChoice::Keep);

        assert_eq!(
            timeways_step(setup::Timeways::On, false),
            Some(GET_TIMEWAYS)
        );
        assert_eq!(timeways_step(setup::Timeways::On, true), None);
        assert_eq!(timeways_step(setup::Timeways::Off, false), None);
        assert_eq!(
            final_lines(&new, off, keep, None, Some(GET_TIMEWAYS)),
            ["Get the Timeways addon on CurseForge, then restart WoW."]
        );
        let both = final_lines(
            &new,
            setup::Relay::On,
            keep,
            Some(RelayAddon::Missing),
            Some(GET_TIMEWAYS),
        );
        assert_eq!(both.len(), 2);
        assert_eq!(both[1], GET_TIMEWAYS, "the Timeways step is the last line");
    }
}
