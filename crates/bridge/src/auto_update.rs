//! Auto-update (SPEC.md 11.3): the `CurseForge` app puts a newer addon on disk, and the
//! desktop app installs the release of the same version.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use semver::Version;

use crate::config::{AutoUpdate, Config};
use crate::dirs::Dirs;
use crate::install::{ADDON, TIMEWAYS};
use crate::run::log;
use crate::timeways_install::installed_story_program;

/// The version of the Timeways release that setup or `update` installed last.
pub const TIMEWAYS_VERSION_FILE: &str = "timeways-version";
/// The lines of `update --auto`, which has no terminal.
pub const LOG_FILE: &str = "update.log";
const CHECK_EVERY: Duration = Duration::from_mins(1);
const TRY_AGAIN_AFTER: Duration = Duration::from_hours(1);

/// The `## Version:` of a TOC. The packager of Timeways writes the tag, with its `v`.
/// A developer checkout has `@project-version@`, which is no version.
pub fn toc_version(toc: &str) -> Option<Version> {
    let line = toc
        .lines()
        .find_map(|line| line.trim().strip_prefix("## Version:"))?;
    parse_version(line)
}

fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    Version::parse(text.strip_prefix('v').unwrap_or(text)).ok()
}

/// Only up: an older addon never changes the desktop app. With no installed version,
/// any known addon version is newer.
pub fn newer(installed: Option<&Version>, on_disk: Option<Version>) -> Option<Version> {
    let on_disk = on_disk?;
    match installed {
        Some(installed) if on_disk <= *installed => None,
        _ => Some(on_disk),
    }
}

/// The release folder of one version, in place of `.../releases/latest/download`.
pub fn pinned_releases(repo: &str, version: &Version) -> String {
    format!("https://github.com/eserilev/{repo}/releases/download/v{version}")
}

/// The parts of the desktop app that follow their addon.
#[derive(Clone, Debug)]
pub struct Parts {
    /// The `AddOns` folder of each served game (SPEC.md 7.9).
    pub addons: Vec<PathBuf>,
    pub data: PathBuf,
    pub relay: bool,
    /// Only for a story program that setup installed (SPEC.md 11.4).
    pub timeways: bool,
}

impl Parts {
    /// `None` with `auto_update = false` or with no game folder.
    pub fn of(dirs: &Dirs, config: &Config) -> Option<Parts> {
        if config.auto_update == AutoUpdate::Off {
            return None;
        }
        Some(Parts {
            addons: config.games().into_iter().map(|g| g.addons).collect(),
            data: dirs.data.clone(),
            relay: config.relay.is_some(),
            timeways: installed_story_program(config).is_some(),
        })
    }

    pub fn wanted(&self) -> Wanted {
        let running = parse_version(env!("CARGO_PKG_VERSION"));
        Wanted {
            relay: self
                .relay
                .then(|| newer(running.as_ref(), self.addon_version(ADDON)))
                .flatten(),
            timeways: self
                .timeways
                .then(|| {
                    newer(
                        self.timeways_installed().as_ref(),
                        self.addon_version(TIMEWAYS),
                    )
                })
                .flatten(),
        }
    }

    /// The newest version in any served game: the `CurseForge` app updates each game on
    /// its own.
    fn addon_version(&self, addon: &str) -> Option<Version> {
        self.addons
            .iter()
            .filter_map(|dir| version_in(dir, addon))
            .max()
    }

    fn timeways_installed(&self) -> Option<Version> {
        installed_timeways(&self.data)
    }
}

fn version_in(addons: &Path, addon: &str) -> Option<Version> {
    let toc = addons.join(addon).join(format!("{addon}.toc"));
    toc_version(&fs::read_to_string(toc).ok()?)
}

pub fn installed_timeways(data: &Path) -> Option<Version> {
    parse_version(&fs::read_to_string(data.join(TIMEWAYS_VERSION_FILE)).ok()?)
}

pub fn save_installed_timeways(data: &Path, version: &str) -> std::io::Result<()> {
    fs::write(data.join(TIMEWAYS_VERSION_FILE), format!("{version}\n"))
}

/// The release that each part needs. `None` when its addon is not newer.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub relay: Option<Version>,
    pub timeways: Option<Version>,
}

impl Wanted {
    /// One name for the whole update, so a failed one waits before the next try.
    fn name(&self) -> Option<String> {
        match (&self.relay, &self.timeways) {
            (None, None) => None,
            (relay, timeways) => Some(format!("relay {relay:?}, timeways {timeways:?}")),
        }
    }
}

/// A try of the same versions waits an hour, so a release whose files are not
/// attached yet never loops.
#[derive(Default)]
pub struct Tries {
    last: BTreeMap<String, Instant>,
}

impl Tries {
    pub fn may_try(&self, name: &str, now: Instant) -> bool {
        self.last
            .get(name)
            .is_none_or(|last| now.duration_since(*last) >= TRY_AGAIN_AFTER)
    }

    pub fn tried(&mut self, name: &str, now: Instant) {
        self.last.insert(name.to_owned(), now);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    /// A run is in progress, maybe with a desktop request open.
    Busy,
}

/// Checks the addons once a minute, and starts `update --auto` while nothing runs.
pub struct AutoUpdater {
    parts: Parts,
    /// The desktop app, which `update --auto` replaces.
    program: PathBuf,
    last_check: Option<Instant>,
    tries: Tries,
}

impl AutoUpdater {
    pub fn new(parts: Parts, program: PathBuf) -> AutoUpdater {
        AutoUpdater {
            parts,
            program,
            last_check: None,
            tries: Tries::default(),
        }
    }

    pub fn tick(&mut self, activity: Activity, now: Instant) {
        if self
            .last_check
            .is_some_and(|last| now.duration_since(last) < CHECK_EVERY)
        {
            return;
        }
        self.last_check = Some(now);
        if activity == Activity::Busy {
            return;
        }
        let wanted = self.parts.wanted();
        let Some(name) = wanted.name() else {
            return;
        };
        if !self.tries.may_try(&name, now) {
            return;
        }
        self.tries.tried(&name, now);
        log(&format!(
            "auto-update: an addon is newer ({name}), so the desktop app updates"
        ));
        if let Err(e) = start_update(&self.program, &self.parts.data) {
            log(&format!("auto-update: cannot start the update: {e:#}"));
        }
    }
}

/// `update --auto` restarts the bridge last, so it runs as a process of its own.
fn start_update(program: &Path, data: &Path) -> anyhow::Result<()> {
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join(LOG_FILE))?;
    let mut command = Command::new(program);
    command
        .args(["update", "--auto"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    own_process_group(&mut command);
    command.spawn()?;
    Ok(())
}

/// A restart without a service stops the process group of the old bridge.
#[cfg(unix)]
fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn a_toc_version_drops_its_v_and_a_placeholder_is_no_version() {
        assert_eq!(
            toc_version("## Interface: 16001\n## Version: 0.4.2\n"),
            Some(version("0.4.2"))
        );
        assert_eq!(
            toc_version("## Version: v0.1.0-rc.2\r\n"),
            Some(version("0.1.0-rc.2"))
        );
        assert_eq!(toc_version("## Version: @project-version@\n"), None);
        assert_eq!(toc_version("## Title: Timeways\n"), None);
    }

    #[test]
    fn only_a_newer_addon_wants_an_update() {
        let installed = version("0.4.2");

        assert_eq!(
            newer(Some(&installed), Some(version("0.4.3"))),
            Some(version("0.4.3"))
        );
        assert_eq!(newer(Some(&installed), Some(version("0.4.2"))), None);
        assert_eq!(newer(Some(&installed), Some(version("0.4.1"))), None);
        assert_eq!(newer(Some(&installed), None), None);
    }

    #[test]
    fn a_pre_release_is_older_than_its_release() {
        let release = version("0.1.0");

        assert_eq!(newer(Some(&release), Some(version("0.1.0-rc.2"))), None);
        assert_eq!(
            newer(Some(&version("0.1.0-rc.2")), Some(release.clone())),
            Some(release)
        );
    }

    #[test]
    fn with_no_installed_version_a_known_addon_version_wants_an_update() {
        assert_eq!(newer(None, Some(version("0.1.0"))), Some(version("0.1.0")));
    }

    #[test]
    fn a_pinned_release_folder_names_the_tag_of_the_version() {
        assert_eq!(
            pinned_releases("timeways", &version("0.1.0-rc.2")),
            "https://github.com/eserilev/timeways/releases/download/v0.1.0-rc.2"
        );
    }

    #[test]
    fn the_same_versions_wait_an_hour_and_other_versions_do_not() {
        let start = Instant::now();
        let mut tries = Tries::default();

        tries.tried("a", start);

        assert!(!tries.may_try("a", start + Duration::from_mins(59)));
        assert!(tries.may_try("a", start + TRY_AGAIN_AFTER));
        assert!(tries.may_try("b", start));
    }

    struct Disk {
        _tmp: tempfile::TempDir,
        parts: Parts,
    }

    fn disk(relay_toc: Option<&str>, timeways_toc: Option<&str>) -> Disk {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("AddOns");
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).unwrap();
        for (addon, toc) in [(ADDON, relay_toc), (TIMEWAYS, timeways_toc)] {
            let Some(toc) = toc else { continue };
            fs::create_dir_all(addons.join(addon)).unwrap();
            fs::write(addons.join(addon).join(format!("{addon}.toc")), toc).unwrap();
        }
        let parts = Parts {
            addons: vec![addons],
            data,
            relay: true,
            timeways: true,
        };
        Disk { _tmp: tmp, parts }
    }

    #[test]
    fn a_newer_relay_addon_wants_its_release_and_the_same_version_wants_none() {
        let running = env!("CARGO_PKG_VERSION");
        let same = disk(Some(&format!("## Version: {running}\n")), None);
        let newer_one = disk(Some("## Version: 999.0.0\n"), None);

        assert_eq!(same.parts.wanted(), Wanted::default());
        assert_eq!(newer_one.parts.wanted().relay, Some(version("999.0.0")));
    }

    #[test]
    fn the_newest_relay_addon_of_any_served_game_wants_its_release() {
        let mut forever = disk(Some("## Version: 0.0.1\n"), None);
        let anniversary = disk(Some("## Version: 999.0.0\n"), None);
        forever
            .parts
            .addons
            .extend(anniversary.parts.addons.clone());

        assert_eq!(forever.parts.wanted().relay, Some(version("999.0.0")));
    }

    #[test]
    fn a_timeways_addon_newer_than_the_installed_release_wants_its_release() {
        let d = disk(None, Some("## Version: v0.2.0\n"));
        save_installed_timeways(&d.parts.data, "0.1.0").unwrap();

        assert_eq!(d.parts.wanted().timeways, Some(version("0.2.0")));

        save_installed_timeways(&d.parts.data, "0.2.0").unwrap();
        assert_eq!(d.parts.wanted(), Wanted::default());
    }

    #[test]
    fn a_part_that_is_off_wants_nothing() {
        let mut d = disk(Some("## Version: 999.0.0\n"), Some("## Version: 9.0.0\n"));
        d.parts.relay = false;
        d.parts.timeways = false;

        assert_eq!(d.parts.wanted(), Wanted::default());
    }

    #[cfg(unix)]
    fn updater_with_marker(d: &Disk) -> (AutoUpdater, PathBuf) {
        let program = d.parts.data.join("fake-gnomish-relay");
        let marker = d.parts.data.join("started");
        let script = format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display());
        crate::fake_program::write(&program, &script).unwrap();
        (AutoUpdater::new(d.parts.clone(), program), marker)
    }

    #[cfg(unix)]
    fn wait_for(file: &Path) -> Option<String> {
        for _ in 0..200 {
            if let Ok(text) = fs::read_to_string(file) {
                return Some(text);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }

    #[cfg(unix)]
    #[test]
    fn a_newer_addon_while_nothing_runs_starts_update_auto() {
        let d = disk(Some("## Version: 999.0.0\n"), None);
        let (mut updater, marker) = updater_with_marker(&d);

        updater.tick(Activity::Idle, Instant::now());

        assert_eq!(wait_for(&marker).as_deref(), Some("update --auto\n"));
    }

    #[cfg(unix)]
    #[test]
    fn a_run_in_progress_keeps_the_update_until_the_next_check() {
        let d = disk(Some("## Version: 999.0.0\n"), None);
        let (mut updater, marker) = updater_with_marker(&d);
        let start = Instant::now();

        updater.tick(Activity::Busy, start);
        updater.tick(Activity::Idle, start + Duration::from_secs(1));
        assert!(!marker.exists());

        updater.tick(Activity::Idle, start + CHECK_EVERY);
        assert!(wait_for(&marker).is_some());
    }
}
