//! Setup and update for Timeways (SPEC.md 11.4): the programs of its release, the lore
//! pack, and `program` and `lore_pack` of `[story]`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Config};
use crate::config_story::{config_path, with_story_paths};
use crate::dirs::Dirs;
use crate::fs_safe::make_private_dir;
use crate::lore_pack::{self, DUMP_URL, DUMP_URL_VAR, PACK_FILE};
use crate::setup::write_config;
use crate::timeways_release::{self, PACK, RELEASES, STORY, URL_VAR};

/// Where the release and the dump come from.
pub struct Sources {
    pub release: String,
    pub dump: String,
}

impl Sources {
    /// `TIMEWAYS_URL` and `TIMEWAYS_DUMP_URL` change them, for a mirror or a test.
    pub fn from_env() -> Sources {
        let var =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Sources {
            release: var(URL_VAR, RELEASES),
            dump: var(DUMP_URL_VAR, DUMP_URL),
        }
    }
}

/// Where the programs, the pack, and the downloads go.
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

/// What happened to the lore pack.
#[derive(Debug, PartialEq, Eq)]
pub enum Lore {
    /// The last two lines of `timeways-pack`.
    Built(Vec<String>),
    /// The build failed, and the old pack stays.
    Kept(String),
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

/// Installs the programs, builds a new lore pack, and sets the config. With no pack at
/// all after the build, the config keeps no program, and this fails.
pub fn install(
    dirs: &Dirs,
    sources: &Sources,
    places: &Places,
    progress: impl FnMut(u64),
) -> Result<Report> {
    fresh_work_folder(&places.work)?;
    let result = install_in_work_folder(sources, places, progress);
    // The dump is 133 MB, and nothing needs it after the build.
    let _ = fs::remove_dir_all(&places.work);
    let (version, changed, lore) = result?;
    // A built pack is always there, so only a failed build with no old pack stops here.
    if let (Lore::Kept(error), false) = (&lore, places.pack.is_file()) {
        bail!("{error}");
    }
    set_config(dirs, places)?;
    Ok(Report {
        version,
        changed,
        lore,
    })
}

fn install_in_work_folder(
    sources: &Sources,
    places: &Places,
    progress: impl FnMut(u64),
) -> Result<(String, Vec<String>, Lore)> {
    let download = timeways_release::fetch(&sources.release, &places.work)?;
    let changed = timeways_release::install(&download, &places.bin)?;
    let lore = match build_lore(sources, places, progress) {
        Ok(summary) => Lore::Built(summary),
        Err(error) => Lore::Kept(format!("{error:#}")),
    };
    Ok((download.version, changed, lore))
}

fn build_lore(
    sources: &Sources,
    places: &Places,
    progress: impl FnMut(u64),
) -> Result<Vec<String>> {
    let dump = lore_pack::download_dump(&sources.dump, &places.work, progress)?;
    lore_pack::build(&places.program(PACK), &dump, &places.pack)
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
    let result = timeways_release::fetch(&sources.release, &work)
        .and_then(|download| timeways_release::install(&download, bin));
    let _ = fs::remove_dir_all(&work);
    result
}
