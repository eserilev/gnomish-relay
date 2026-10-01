//! `gnomish-relay setup --timeways`: the setup of Timeways alone (SPEC.md 9.7, decision
//! 15). Its key, key addon, slots, `[story]` with its model, the story program, and the
//! lore pack. It never sets up the relay part.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::dirs::Dirs;
use crate::install;
use crate::model::ModelChoice;
use crate::model_setup;
use crate::ollama_install::{self, LOCAL_STORY_MODEL};
use crate::program::find_program;
use crate::setup::{self, KeyChoice, Product};
use crate::setup_command::{
    Autostart, Found, SetupArgs, autostart, new_wow, product_files, read_answer, stdin_is_terminal,
    write_parts,
};
use crate::timeways_install::{self, Lore, Sources};
use crate::wsl;

pub(crate) fn setup(dirs: &Dirs, args: &SetupArgs, found: Found) -> Result<()> {
    let existing = found.existing.as_ref();
    // The question comes before the first file, so a stop there leaves nothing half done.
    let local_model = local_model_question(existing);
    let changed = product_files(dirs, found.addons.as_deref(), Product::Timeways, args.keys)?;
    let config = setup_story_config(dirs, found.wow, existing)?;
    let config = setup_story_model(dirs, config, local_model);
    if let Some(line) = story_line(&config) {
        println!("{line}");
    }
    install_story_program(dirs, args.autostart);
    autostart(dirs, found.wow, args.autostart);
    let (Some(changed), Some(addons)) = (changed, found.addons) else {
        println!("{NO_WOW}");
        return Ok(());
    };
    let folder = install::timeways_dir(&addons).is_some();
    println!("{}", last_line(&changed, args.keys, folder));
    Ok(())
}

const NO_WOW: &str =
    "Timeways: WoW not found. Start WoW once, then run gnomish-relay setup --timeways.";

/// Writes the first config, or adds `[story]` to a config that has none. It keeps every
/// line of the relay part.
fn setup_story_config(
    dirs: &Dirs,
    wow: Option<&std::path::Path>,
    existing: Option<&(String, Config)>,
) -> Result<Config> {
    let lacks_story = existing.is_none_or(|(_, c)| c.story.is_none());
    let models = if lacks_story {
        model_setup::find_models(&wsl::path_var())
    } else {
        Vec::new()
    };
    let parts = setup::ConfigParts {
        wow: new_wow(wow, existing),
        relay: None,
        new_agents: &[],
        story: lacks_story.then_some(models.as_slice()),
    };
    write_parts(dirs, existing, &parts)
}

/// A failed step prints one line, and setup goes on (SPEC.md 11.4).
fn install_story_program(dirs: &Dirs, autostart: Autostart) {
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
        Ok(report) => installed_lines(&report, &places, autostart),
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
        sentence_of(error)
    )
}

fn installed_lines(
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
        lines.push("To start the Timeways story program, run gnomish-relay restart".into());
    }
    lines
}

/// Whether setup installs a free local model for Timeways (SPEC.md 11.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalModel {
    Install,
    Skip,
}

fn local_model_question(existing: Option<&(String, Config)>) -> LocalModel {
    let has_model = existing.is_some_and(|(_, config)| story_has_model(config));
    if has_model || !model_setup::find_models(&wsl::path_var()).is_empty() {
        return LocalModel::Skip;
    }
    ask_local_model()
}

/// With no model, the offer of a free local model already said what to do.
fn story_line(config: &Config) -> Option<String> {
    let model = config.story.as_ref().map(|story| &story.model.choice);
    match model? {
        ModelChoice::Claude { model, .. } => Some(format!(
            "Timeways story model: claude ({})",
            model.as_deref().unwrap_or("default")
        )),
        ModelChoice::Local(local) => Some(format!("Timeways story model: local {}", local.model)),
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
            println!("Timeways story model: couldn't set it. {}", sentence_of(&e));
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
                "Timeways story model: couldn't install the free local model. {} {TRY_AGAIN}",
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
            "Timeways story model: {LOCAL_STORY_MODEL} is installed, but it didn't answer a \
             test. {} Check that Ollama is running, then run gnomish-relay restart",
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
/// That step replaces "all set", and comes last.
fn last_line(changed: &setup::Changed, keys: KeyChoice, folder: bool) -> &'static str {
    if !folder {
        return GET_TIMEWAYS;
    }
    if changed.new_slots || changed.key_addon == install::Installed::New {
        return "Timeways: all set. Restart WoW to load the addon";
    }
    if changed.key_addon == install::Installed::Updated || keys == KeyChoice::New {
        return "Timeways: all set. Type /reload in WoW";
    }
    "Timeways: all set"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

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
    fn the_story_line_names_timeways_and_the_model_and_says_nothing_with_no_model() {
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
            Some("Timeways story model: local llama3.2:3b")
        );
    }

    fn places() -> timeways_install::Places {
        timeways_install::Places {
            bin: PathBuf::from("/b"),
            pack: PathBuf::from("/p"),
            work: PathBuf::from("/w"),
        }
    }

    #[test]
    fn the_timeways_lines_name_the_version_the_lore_and_the_next_step() {
        let built = timeways_install::Report {
            version: "0.1.0".into(),
            changed: vec![],
            lore: Lore::Built(vec!["read 9 pages, skipped 1".into()]),
        };
        let kept = timeways_install::Report {
            lore: Lore::Kept("Couldn't build the Timeways lore. Your old lore stays.".into()),
            ..built
        };

        let lines = installed_lines(&kept, &places(), Autostart::On);
        assert_eq!(
            lines,
            [
                "Timeways story program: 0.1.0 in /b",
                "Timeways lore: Couldn't build the Timeways lore. Your old lore stays. To try again, run gnomish-relay setup --timeways",
            ]
        );
        let off = installed_lines(&kept, &places(), Autostart::Off);
        assert_eq!(
            off.last().unwrap(),
            "To start the Timeways story program, run gnomish-relay restart"
        );
    }

    #[test]
    fn a_lore_error_with_no_period_stays_apart_from_the_next_step() {
        let kept = timeways_install::Report {
            version: "0.1.0".into(),
            changed: vec![],
            lore: Lore::Kept("couldn't download the Wowpedia lore (no internet connection)".into()),
        };

        let lines = installed_lines(&kept, &places(), Autostart::On);

        assert_eq!(
            lines[1],
            "Timeways lore: Couldn't download the Wowpedia lore (no internet connection). To try again, run gnomish-relay setup --timeways"
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

    fn changed(new_slots: bool, key_addon: install::Installed) -> setup::Changed {
        setup::Changed {
            key_addon,
            new_slots,
        }
    }

    #[test]
    fn the_last_line_of_timeways_says_timeways_and_what_the_game_needs() {
        use install::Installed::{New, Unchanged, Updated};
        let keep = KeyChoice::Keep;

        assert_eq!(
            last_line(&changed(true, New), keep, true),
            "Timeways: all set. Restart WoW to load the addon"
        );
        assert_eq!(
            last_line(&changed(false, Updated), keep, true),
            "Timeways: all set. Type /reload in WoW"
        );
        assert_eq!(
            last_line(&changed(false, Unchanged), KeyChoice::New, true),
            "Timeways: all set. Type /reload in WoW"
        );
        assert_eq!(
            last_line(&changed(false, Unchanged), keep, true),
            "Timeways: all set"
        );
    }

    #[test]
    fn setup_for_timeways_with_no_timeways_folder_ends_with_the_curseforge_step() {
        let new = changed(true, install::Installed::New);

        assert_eq!(
            last_line(&new, KeyChoice::Keep, false),
            "Get the Timeways addon on CurseForge, then restart WoW."
        );
    }
}
