//! Setup and update for Timeways (SPEC.md 11.4): the programs of its release, and
//! `program` and `lore_pack` of `[story]`. The desktop app builds the lore pack itself
//! (`lore_job`).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use semver::Version;

use crate::auto_update::{pinned_releases, save_installed_timeways};
use crate::config::{self, Config};
use crate::config_story::{config_path, with_story_paths};
use crate::dirs::Dirs;
use crate::download_failure::{self, DownloadFailed, Reason};
use crate::fs_safe::make_private_dir;
use crate::lore_job::REBUILD_FILE;
use crate::lore_pack::PACK_FILE;
use crate::service;
use crate::setup::write_config;
use crate::timeways_release::{self, RELEASES, STORY, URL_VAR};

/// Where the release comes from.
pub struct Sources {
    pub release: String,
}

impl Sources {
    /// `TIMEWAYS_URL` changes it, for a mirror or a test.
    pub fn from_env() -> Sources {
        Sources::with_release(RELEASES)
    }

    /// The release of `version` in place of the latest one (SPEC.md 11.3, auto-update).
    pub fn pinned(version: &Version) -> Sources {
        Sources::with_release(&pinned_releases("timeways", version))
    }

    fn with_release(release: &str) -> Sources {
        Sources {
            release: std::env::var(URL_VAR).unwrap_or_else(|_| release.into()),
        }
    }
}

/// Where the programs, the pack, and the download go.
pub struct Places {
    pub bin: PathBuf,
    pub pack: PathBuf,
    /// In the data folder, which the sandbox hides. It goes away after each install.
    pub work: PathBuf,
}

impl Places {
    /// `bin` is `GNOMISH_BIN` when it is set. The data folder of Windows holds the `bin`
    /// of `install.ps1`, and the bridge refuses a story program in a hidden folder.
    pub fn of(dirs: &Dirs, bin: Option<PathBuf>) -> Places {
        let parent = dirs.data.parent().unwrap_or(&dirs.home);
        let timeways = parent.join("timeways");
        let default_bin = if cfg!(windows) {
            timeways.join("bin")
        } else {
            dirs.home.join(".local").join("bin")
        };
        Places {
            bin: bin.unwrap_or(default_bin),
            pack: timeways.join(PACK_FILE),
            work: dirs.data.join("timeways-download"),
        }
    }

    fn program(&self, name: &str) -> PathBuf {
        self.bin
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }
}

/// What happens to the lore pack. The desktop app builds it in the background.
#[derive(Debug, PartialEq, Eq)]
pub enum Lore {
    /// No pack yet. Until it is built, `/lore` answers from the seen text only.
    Later,
    /// The old pack serves until the new one is built.
    Rebuild,
}

#[derive(Debug)]
pub struct Report {
    pub version: String,
    /// The programs that are new or changed.
    pub changed: Vec<String>,
    pub lore: Lore,
}

fn fresh_work_folder(work: &Path) -> Result<()> {
    let _ = fs::remove_dir_all(work);
    make_private_dir(work)
}

/// Installs the programs, sets the config, and asks the desktop app for a lore pack. It
/// downloads no dump: the build takes minutes, and a closed terminal would lose it.
pub fn install(dirs: &Dirs, sources: &Sources, places: &Places) -> Result<Report> {
    fresh_work_folder(&places.work)?;
    let result = install_release(sources, places);
    let _ = fs::remove_dir_all(&places.work);
    if let Err(error) = &result {
        log_details(dirs, error);
    }
    let (version, changed) = result?;
    save_installed_timeways(&dirs.data, &version)?;
    let lore = ask_for_lore(dirs, places)?;
    set_config(dirs, places)?;
    Ok(Report {
        version,
        changed,
        lore,
    })
}

fn install_release(sources: &Sources, places: &Places) -> Result<(String, Vec<String>)> {
    let download = timeways_release::fetch(&sources.release, &places.work)?;
    let changed = timeways_release::install(&download, &places.bin)?;
    Ok((download.version, changed))
}

/// An old pack serves until the new one is in place (SPEC.md 11.4).
fn ask_for_lore(dirs: &Dirs, places: &Places) -> Result<Lore> {
    if !places.pack.is_file() {
        return Ok(Lore::Later);
    }
    fs::write(dirs.data.join(REBUILD_FILE), "")
        .with_context(|| format!("cannot write {}", dirs.data.join(REBUILD_FILE).display()))?;
    Ok(Lore::Rebuild)
}

/// The details of a failed download go to the log, never to the terminal.
fn log_details(dirs: &Dirs, error: &anyhow::Error) {
    if let Some(details) = download_failure::details_of(error) {
        let _ = service::append_to_log(dirs, &format!("timeways: {details}"));
    }
}

/// The line after a failed download of the story program, or `None` for another error.
/// `again` is the command that tries again, for example "run gnomish-relay update".
pub fn download_failed_line(error: &anyhow::Error, again: &str) -> Option<String> {
    let reason = error.downcast_ref::<DownloadFailed>()?.reason;
    let what = "Couldn't download Timeways";
    Some(match reason {
        Reason::Missing => {
            format!("{what} (the release isn't published yet). To try again later, {again}")
        }
        Reason::Offline => format!("{what}. Check your internet connection, then {again}"),
        Reason::Other => format!("{what}. To try again, {again}"),
    })
}

/// Sets `program` and `lore_pack` in `[story]`. The loader checks the text first.
pub fn set_config(dirs: &Dirs, places: &Places) -> Result<()> {
    let file = dirs.config.join(config::FILE);
    let text =
        fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
    let program = config_path(&places.program(STORY), &dirs.home);
    let pack = config_path(&places.pack, &dirs.home);
    let new = with_story_paths(&text, &program, &pack);
    if new != text {
        write_config(&dirs.config, &new, &dirs.home)?;
    }
    Ok(())
}

/// The program of `[story]`, when setup installed it from a Timeways release.
pub fn installed_story_program(config: &Config) -> Option<PathBuf> {
    let program = config.story.as_ref()?.program.as_ref()?.program.clone();
    let name = program.file_stem()?.to_string_lossy().into_owned();
    (name == STORY && program.is_file()).then_some(program)
}

/// `gnomish-relay update`: the programs of the latest release, into the folder of the
/// installed story program. Returns the programs that changed.
pub fn update(dirs: &Dirs, sources: &Sources, story_program: &Path) -> Result<Vec<String>> {
    let bin = story_program
        .parent()
        .context("the story program has no folder")?;
    let work = dirs.data.join("timeways-download");
    fresh_work_folder(&work)?;
    let result = timeways_release::fetch(&sources.release, &work).and_then(|download| {
        let changed = timeways_release::install(&download, bin)?;
        save_installed_timeways(&dirs.data, &download.version)?;
        Ok(changed)
    });
    let _ = fs::remove_dir_all(&work);
    if let Err(error) = &result {
        log_details(dirs, error);
    }
    result
}

/// An error of the chain starts in lower case and has no period, so the next sentence
/// of a line needs both.
pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut line: String = first.to_uppercase().chain(chars).collect();
    if !line.ends_with(['.', '!', '?']) {
        line.push('.');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(reason: Reason) -> anyhow::Error {
        anyhow::Error::new(DownloadFailed {
            reason,
            details: String::new(),
        })
    }

    #[test]
    fn a_story_program_that_is_not_published_says_to_try_again_later() {
        let again = "run gnomish-relay setup --timeways";

        assert_eq!(
            download_failed_line(&failed(Reason::Missing), again).unwrap(),
            "Couldn't download Timeways (the release isn't published yet). \
             To try again later, run gnomish-relay setup --timeways"
        );
        assert_eq!(
            download_failed_line(&failed(Reason::Offline), again).unwrap(),
            "Couldn't download Timeways. Check your internet connection, \
             then run gnomish-relay setup --timeways"
        );
        assert_eq!(
            download_failed_line(&failed(Reason::Other), again).unwrap(),
            "Couldn't download Timeways. To try again, run gnomish-relay \
             setup --timeways"
        );
        assert_eq!(download_failed_line(&anyhow::anyhow!("x"), again), None);
    }

    #[test]
    fn an_error_becomes_a_sentence_with_one_period() {
        assert_eq!(sentence("the download failed"), "The download failed.");
        assert_eq!(sentence("Your old lore stays."), "Your old lore stays.");
        assert_eq!(sentence(""), "");
    }
}
