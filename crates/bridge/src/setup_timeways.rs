//! `gnomish-relay setup --timeways`: the setup of Timeways alone (SPEC.md 9.7, decision
//! 15). Its key, key addon, slots, `[story]` with its model, the story program, and the
//! lore pack. It never sets up the relay part.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::dirs::Dirs;
use crate::install;
use crate::lock;
use crate::model::ModelChoice;
use crate::model_setup;
use crate::ollama_install::{self, LOCAL_STORY_MODEL};
use crate::program::find_program;
use crate::service;
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
    let install = install_story_program(dirs);
    autostart(dirs, found.wow, args.autostart, Product::Timeways);
    // The autostart already restarted the app, so only a setup without it restarts here.
    if install == StoryInstall::Done && args.autostart == Autostart::Off {
        restart_if_running(dirs);
    }
    let (Some(changed), Some(addons)) = (changed, found.addons) else {
        println!("{NO_WOW}");
        return Ok(());
    };
    let folder = install::timeways_dir(&addons).is_some();
    println!("{}", last_line(install, &changed, args.keys, folder));
    Ok(())
}

const NO_WOW: &str = "Couldn't find WoW. Start WoW once, then run gnomish-relay setup --timeways.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoryInstall {
    Done,
    Failed,
}

/// The new programs run only after a restart. A stopped app stays stopped: the player
/// turned the autostart off.
fn restart_if_running(dirs: &Dirs) {
    if !matches!(lock::status(&dirs.data), Ok(lock::Bridge::Runs(_))) {
        println!("{NOT_RUNNING}");
        return;
    }
    let restarted = std::env::current_exe()
        .map_err(anyhow::Error::from)
        .and_then(|exe| service::restart(dirs, &exe));
    if let Err(e) = restarted {
        println!(
            "Couldn't restart the desktop app. {} To try again, run gnomish-relay restart",
            sentence_of(&e)
        );
    }
}

const NOT_RUNNING: &str = "The desktop app isn't running. To start it, run gnomish-relay restart";

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
fn install_story_program(dirs: &Dirs) -> StoryInstall {
    println!("{INSTALLING}");
    let bin = std::env::var_os("GNOMISH_BIN").map(PathBuf::from);
    let places = timeways_install::Places::of(dirs, bin);
    match timeways_install::install(dirs, &Sources::from_env(), &places) {
        Ok(report) => {
            for line in installed_lines(&report) {
                println!("{line}");
            }
            StoryInstall::Done
        }
        Err(e) => {
            println!("{}", install_failed_line(&e));
            StoryInstall::Failed
        }
    }
}

const INSTALLING: &str = "Installing Timeways.";

const TRY_AGAIN: &str = "To try again, run gnomish-relay setup --timeways";

/// A failed download names its reason, never the command line of `curl`.
fn install_failed_line(error: &anyhow::Error) -> String {
    if let Some(line) =
        timeways_install::download_failed_line(error, "run gnomish-relay setup --timeways")
    {
        return line;
    }
    format!(
        "Couldn't install Timeways. {} {TRY_AGAIN}",
        sentence_of(error)
    )
}

/// The desktop app builds the lore after setup ends (SPEC.md 11.4).
fn installed_lines(report: &timeways_install::Report) -> Vec<String> {
    let lore = match report.lore {
        Lore::Later => LORE_LATER,
        Lore::Rebuild => LORE_REBUILD,
    };
    vec![
        format!("Timeways {} installed", report.version),
        lore.into(),
    ]
}

const LORE_LATER: &str =
    "The lore downloads in the background. Until it's ready, /lore answers from what you've seen.";
const LORE_REBUILD: &str =
    "New lore downloads in the background. Your current lore works until then.";

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
            "AI model: claude ({})",
            model.as_deref().unwrap_or("default")
        )),
        ModelChoice::Local(local) => Some(format!("AI model: {} on this computer", local.model)),
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
            println!("Couldn't set the AI model. {}", sentence_of(&e));
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
                "Couldn't install the free local model. {} {TRY_AGAIN}",
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
            "{LOCAL_STORY_MODEL} is installed, but it didn't answer a \
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

/// A failed install already printed its reason and the command that tries again.
const NOT_READY: &str = "Timeways isn't ready yet.";

/// Setup for Timeways can come before its addon, which players get from `CurseForge`.
/// That step replaces "all set", and comes last, also after a failure. A failure never
/// ends with "all set".
fn last_line(
    install: StoryInstall,
    changed: &setup::Changed,
    keys: KeyChoice,
    folder: bool,
) -> &'static str {
    if !folder {
        return GET_TIMEWAYS;
    }
    if install == StoryInstall::Failed {
        return NOT_READY;
    }
    if changed.new_slots || changed.key_addon == install::Installed::New {
        return "All set! Restart WoW, then type /timeways test";
    }
    if changed.key_addon == install::Installed::Updated || keys == KeyChoice::New {
        return "All set! Type /reload in WoW, then /timeways test";
    }
    "All set! Type /timeways test in WoW to check it"
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
            Some("AI model: llama3.2:3b on this computer")
        );
    }

    #[test]
    fn an_install_says_the_version_and_that_the_lore_comes_in_the_background() {
        let report = |lore| timeways_install::Report {
            version: "0.1.0".into(),
            changed: vec![],
            lore,
        };

        assert_eq!(
            installed_lines(&report(Lore::Later)),
            ["Timeways 0.1.0 installed", LORE_LATER]
        );
        assert_eq!(installed_lines(&report(Lore::Rebuild))[1], LORE_REBUILD);
    }

    #[test]
    fn a_failed_timeways_install_gives_the_error_and_then_the_next_step() {
        let error = anyhow::anyhow!("the download of timeways-x.tar.gz has a wrong SHA-256 sum");

        assert_eq!(
            install_failed_line(&error),
            "Couldn't install Timeways. The download of timeways-x.tar.gz has a wrong SHA-256 sum. To try again, run gnomish-relay setup --timeways"
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
            "Couldn't download Timeways (the release isn't published yet). To try again later, run gnomish-relay setup --timeways"
        );
    }

    fn changed(new_slots: bool, key_addon: install::Installed) -> setup::Changed {
        setup::Changed {
            key_addon,
            new_slots,
        }
    }

    #[test]
    fn the_last_line_says_all_set_and_how_to_check_it_in_the_game() {
        use install::Installed::{New, Unchanged, Updated};
        let (done, keep) = (StoryInstall::Done, KeyChoice::Keep);

        assert_eq!(
            last_line(done, &changed(true, New), keep, true),
            "All set! Restart WoW, then type /timeways test"
        );
        assert_eq!(
            last_line(done, &changed(false, Updated), keep, true),
            "All set! Type /reload in WoW, then /timeways test"
        );
        assert_eq!(
            last_line(done, &changed(false, Unchanged), KeyChoice::New, true),
            "All set! Type /reload in WoW, then /timeways test"
        );
        assert_eq!(
            last_line(done, &changed(false, Unchanged), keep, true),
            "All set! Type /timeways test in WoW to check it"
        );
    }

    #[test]
    fn a_failed_install_never_ends_with_all_set() {
        let new = changed(true, install::Installed::New);

        assert_eq!(
            last_line(StoryInstall::Failed, &new, KeyChoice::Keep, true),
            "Timeways isn't ready yet."
        );
        assert_eq!(
            last_line(StoryInstall::Failed, &new, KeyChoice::Keep, false),
            "Get the Timeways addon on CurseForge, then restart WoW."
        );
    }

    #[test]
    fn setup_for_timeways_with_no_timeways_folder_ends_with_the_curseforge_step() {
        let new = changed(true, install::Installed::New);

        assert_eq!(
            last_line(StoryInstall::Done, &new, KeyChoice::Keep, false),
            "Get the Timeways addon on CurseForge, then restart WoW."
        );
    }
}
