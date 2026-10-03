//! Builds the Timeways lore pack in the background (SPEC.md 11.4): the dump download,
//! `timeways-pack from-dump`, and the rename over the pack. Setup only asks for a build,
//! so a closed terminal or a reboot loses nothing: the next start tries again.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::download_failure::{self, DownloadFailed, Reason};
use crate::fs_safe::{make_private_dir, write_atomic_unsynced};
use crate::lore_pack::{self, DUMP_URL, DUMP_URL_VAR};
use crate::run::log;
use crate::timeways_release::PACK;

/// In the data folder: setup asks for a new pack while an old one exists.
pub const REBUILD_FILE: &str = "lore-rebuild";
/// In the data folder: the state of the last build, for `status`.
pub const STATE_FILE: &str = "lore-state";
const WORK_FOLDER: &str = "timeways-lore";
const TRY_AGAIN_AFTER: Duration = Duration::from_hours(1);

/// What one build needs.
#[derive(Clone, Debug)]
pub struct LoreParts {
    pub pack_program: PathBuf,
    pub pack: PathBuf,
    pub dump_url: String,
    /// The data folder, which the sandbox hides. The dump goes into a folder in it.
    pub data: PathBuf,
}

impl LoreParts {
    /// `timeways-pack` sits next to the story program, as setup installs them.
    /// `TIMEWAYS_DUMP_URL` changes the dump, for a mirror or a test.
    pub fn of(story_program: &Path, pack: &Path, data: &Path) -> LoreParts {
        let name = format!("{PACK}{}", std::env::consts::EXE_SUFFIX);
        LoreParts {
            pack_program: story_program.with_file_name(name),
            pack: pack.to_owned(),
            dump_url: std::env::var(DUMP_URL_VAR).unwrap_or_else(|_| DUMP_URL.into()),
            data: data.to_owned(),
        }
    }

    /// No pack yet, or setup asked for a new one.
    pub fn build_needed(&self) -> bool {
        !self.pack.is_file() || self.data.join(REBUILD_FILE).is_file()
    }
}

/// The state that `status` shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoreState {
    Downloading {
        megabytes: u64,
    },
    Building,
    Ready,
    /// The reason, as a player reads it.
    Failed(String),
}

impl LoreState {
    pub fn status_line(&self) -> String {
        match self {
            LoreState::Downloading { megabytes } => {
                format!("Timeways lore: downloading ({megabytes} MB)")
            }
            LoreState::Building => "Timeways lore: building".into(),
            LoreState::Ready => "Timeways lore: ready".into(),
            LoreState::Failed(reason) => {
                format!("Timeways lore: {reason}. The desktop app tries again within an hour.")
            }
        }
    }

    fn to_text(&self) -> String {
        match self {
            LoreState::Downloading { megabytes } => format!("downloading {megabytes}"),
            LoreState::Building => "building".into(),
            LoreState::Ready => "ready".into(),
            LoreState::Failed(reason) => format!("failed {reason}"),
        }
    }

    fn from_text(text: &str) -> Option<LoreState> {
        let text = text.trim_end();
        if let Some(megabytes) = text.strip_prefix("downloading ") {
            return megabytes
                .parse()
                .ok()
                .map(|megabytes| LoreState::Downloading { megabytes });
        }
        if let Some(reason) = text.strip_prefix("failed ") {
            return Some(LoreState::Failed(reason.to_owned()));
        }
        match text {
            "building" => Some(LoreState::Building),
            "ready" => Some(LoreState::Ready),
            _ => None,
        }
    }
}

/// The state of the last build, or `None` before the first one.
pub fn read_state(data: &Path) -> Option<LoreState> {
    LoreState::from_text(&fs::read_to_string(data.join(STATE_FILE)).ok()?)
}

fn write_state(data: &Path, state: &LoreState) {
    if let Err(e) = write_atomic_unsynced(data, STATE_FILE, state.to_text().as_bytes()) {
        log(&format!("timeways: cannot save the lore state: {e:#}"));
    }
}

/// One build: the dump, the pack, and the rename. The dump is about 133 MB, so its
/// folder goes either way. A failed build keeps the old pack (SPEC.md 11.4).
pub fn build(parts: &LoreParts) -> Result<Vec<String>> {
    let work = parts.data.join(WORK_FOLDER);
    let _ = fs::remove_dir_all(&work);
    make_private_dir(&work)?;
    let mut shown = None;
    let result = lore_pack::download_dump(&parts.dump_url, &work, |bytes| {
        let megabytes = bytes / 1_000_000;
        if shown != Some(megabytes) {
            write_state(&parts.data, &LoreState::Downloading { megabytes });
            shown = Some(megabytes);
        }
    })
    .and_then(|dump| {
        write_state(&parts.data, &LoreState::Building);
        lore_pack::build(&parts.pack_program, &dump, &parts.pack)
    });
    let _ = fs::remove_dir_all(&work);
    match &result {
        Ok(_) => {
            let _ = fs::remove_file(parts.data.join(REBUILD_FILE));
            write_state(&parts.data, &LoreState::Ready);
        }
        Err(error) => {
            if let Some(details) = download_failure::details_of(error) {
                log(&format!("timeways: {details}"));
            }
            log(&format!("timeways: the lore build failed: {error:#}"));
            write_state(&parts.data, &LoreState::Failed(failure_reason(error)));
        }
    }
    result
}

/// The reason of a failed build, as a player reads it. The details go to the log.
pub fn failure_reason(error: &anyhow::Error) -> String {
    let reason = error.downcast_ref::<DownloadFailed>().map(|f| f.reason);
    match reason {
        Some(Reason::Missing) => {
            "couldn't download the Wowpedia lore (Wowpedia doesn't have it right now)".into()
        }
        Some(Reason::Offline) => {
            "couldn't download the Wowpedia lore (no internet connection)".into()
        }
        Some(Reason::Other) => "couldn't download the Wowpedia lore".into(),
        None => "couldn't build the lore".into(),
    }
}

/// What a step of the job gives the run loop.
#[derive(Debug, PartialEq, Eq)]
pub enum Finished {
    /// The new pack is in place. The desktop app restarts, so the story program gets it.
    Built,
    Failed,
}

/// At most one build at a time, off the main loop, and at most one try an hour.
pub struct LoreJob {
    parts: LoreParts,
    running: Option<JoinHandle<Result<Vec<String>>>>,
    last_try: Option<Instant>,
}

impl LoreJob {
    pub fn new(parts: LoreParts) -> LoreJob {
        LoreJob {
            parts,
            running: None,
            last_try: None,
        }
    }

    /// Starts a build when one is needed, and reports a build that ended.
    pub fn tick(&mut self, now: Instant) -> Option<Finished> {
        if let Some(handle) = self.running.take_if(|handle| handle.is_finished()) {
            return Some(match handle.join() {
                Ok(Ok(_)) => Finished::Built,
                _ => Finished::Failed,
            });
        }
        let waiting = self.running.is_some()
            || self
                .last_try
                .is_some_and(|last| now.duration_since(last) < TRY_AGAIN_AFTER);
        if waiting || !self.parts.build_needed() {
            return None;
        }
        self.last_try = Some(now);
        log("timeways: building the lore in the background");
        let parts = self.parts.clone();
        self.running = Some(std::thread::spawn(move || build(&parts)));
        None
    }
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
    fn a_failed_build_names_the_reason_and_no_command() {
        assert_eq!(
            failure_reason(&failed(Reason::Offline)),
            "couldn't download the Wowpedia lore (no internet connection)"
        );
        assert_eq!(
            failure_reason(&failed(Reason::Missing)),
            "couldn't download the Wowpedia lore (Wowpedia doesn't have it right now)"
        );
        assert_eq!(
            failure_reason(&anyhow::anyhow!("the pack program crashed")),
            "couldn't build the lore"
        );
    }

    #[test]
    fn each_state_reads_back_as_it_was_written() {
        let states = [
            LoreState::Downloading { megabytes: 45 },
            LoreState::Building,
            LoreState::Ready,
            LoreState::Failed("couldn't download the Wowpedia lore".into()),
        ];
        for state in states {
            assert_eq!(LoreState::from_text(&state.to_text()), Some(state));
        }
        assert_eq!(LoreState::from_text("downloading lots"), None);
        assert_eq!(LoreState::from_text("garbage"), None);
    }

    #[test]
    fn each_state_has_a_status_line() {
        assert_eq!(
            LoreState::Downloading { megabytes: 45 }.status_line(),
            "Timeways lore: downloading (45 MB)"
        );
        assert_eq!(LoreState::Ready.status_line(), "Timeways lore: ready");
        assert_eq!(
            LoreState::Failed(
                "couldn't download the Wowpedia lore (no internet connection)".into()
            )
            .status_line(),
            "Timeways lore: couldn't download the Wowpedia lore (no internet connection). The desktop app tries again within an hour."
        );
    }

    #[test]
    fn a_build_is_needed_with_no_pack_or_with_a_rebuild_request() {
        let root = tempfile::tempdir().unwrap();
        let parts = LoreParts {
            pack_program: root.path().join("timeways-pack"),
            pack: root.path().join("lore.sqlite"),
            dump_url: String::new(),
            data: root.path().to_owned(),
        };
        assert!(parts.build_needed());

        fs::write(&parts.pack, "lore").unwrap();
        assert!(!parts.build_needed());

        fs::write(root.path().join(REBUILD_FILE), "").unwrap();
        assert!(parts.build_needed());
    }

    #[test]
    fn the_pack_program_is_next_to_the_story_program() {
        let parts = LoreParts::of(
            Path::new("/home/p/.local/bin/timeways-story"),
            Path::new("/l.sqlite"),
            Path::new("/d"),
        );

        let name = format!("timeways-pack{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(
            parts.pack_program,
            Path::new("/home/p/.local/bin").join(name)
        );
    }
}
