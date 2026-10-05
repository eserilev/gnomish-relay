//! The game half of dev mode (SPEC.md 16.1): the addon folder of each game becomes a link
//! into this checkout, and goes back at the end. The record goes to disk before the first
//! change, so an end after a crash at any point puts each game back.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fs_safe::write_atomic;

pub const RECORD_FILE: &str = "dev-mode.json";
const ADDON: &str = "GnomishRelay";
/// In `Interface`, next to `AddOns`: WoW never loads it, and the move is one rename.
const BACKUP: &str = "GnomishRelay.dev-backup";

/// What `AddOns/GnomishRelay` was before dev mode.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Before {
    Folder,
    Link { target: PathBuf },
    Nothing,
}

/// Whether a bridge ran before dev mode, so the end starts the login service again.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Release {
    Ran,
    Stopped,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub addons: PathBuf,
    pub before: Before,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub release: Release,
    pub games: Vec<Game>,
}

/// One change of the start. The tests stop after each one, as a crash would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    MoveToBackup(PathBuf),
    RemoveLink(PathBuf),
    LinkCheckout(PathBuf),
}

fn addon_folder(addons: &Path) -> PathBuf {
    addons.join(ADDON)
}

fn backup_folder(addons: &Path) -> Result<PathBuf> {
    let interface = addons
        .parent()
        .with_context(|| format!("{} has no parent folder", addons.display()))?;
    Ok(interface.join(BACKUP))
}

fn note(addons: &Path) -> Result<Before> {
    let addon = addon_folder(addons);
    let meta = match fs::symlink_metadata(&addon) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Before::Nothing),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", addon.display())),
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(&addon)?;
        return Ok(Before::Link { target });
    }
    if !meta.is_dir() {
        bail!("{} is a file, not a folder", addon.display());
    }
    Ok(Before::Folder)
}

/// Notes each game as it is now. Refuses a backup that is already there: it can hold the
/// only copy of an addon.
pub fn plan(addons: &[PathBuf], release: Release) -> Result<Record> {
    let mut games = Vec::new();
    for folder in addons {
        let backup = backup_folder(folder)?;
        if fs::symlink_metadata(&backup).is_ok() {
            bail!(
                "{} is already there. Move it back to {} or delete it, then try again",
                backup.display(),
                addon_folder(folder).display()
            );
        }
        games.push(Game {
            addons: folder.clone(),
            before: note(folder)?,
        });
    }
    Ok(Record { release, games })
}

pub fn write_record(data: &Path, record: &Record) -> Result<()> {
    let json = serde_json::to_vec_pretty(record)?;
    write_atomic(data, RECORD_FILE, &json)
}

pub fn read_record(data: &Path) -> Result<Option<Record>> {
    let path = data.join(RECORD_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let record = serde_json::from_str(&text)
        .with_context(|| format!("{} has a wrong shape", path.display()))?;
    Ok(Some(record))
}

pub fn start_steps(record: &Record) -> Vec<Step> {
    let mut steps = Vec::new();
    for game in &record.games {
        match game.before {
            Before::Folder => steps.push(Step::MoveToBackup(game.addons.clone())),
            Before::Link { .. } => steps.push(Step::RemoveLink(game.addons.clone())),
            Before::Nothing => {}
        }
        steps.push(Step::LinkCheckout(game.addons.clone()));
    }
    steps
}

/// `checkout` is `addon/GnomishRelay` of this checkout.
pub fn apply(step: &Step, checkout: &Path) -> Result<()> {
    match step {
        Step::MoveToBackup(addons) => {
            let backup = backup_folder(addons)?;
            fs::rename(addon_folder(addons), &backup)
                .with_context(|| format!("cannot move the addon to {}", backup.display()))
        }
        Step::RemoveLink(addons) => remove_link(&addon_folder(addons)),
        Step::LinkCheckout(addons) => link_folder(checkout, &addon_folder(addons)),
    }
}

/// Puts back each game of the record, then deletes it. Returns `None` with no record.
pub fn end(data: &Path) -> Result<Option<Record>> {
    let Some(record) = read_record(data)? else {
        return Ok(None);
    };
    for game in &record.games {
        put_back(game)?;
    }
    fs::remove_file(data.join(RECORD_FILE))?;
    Ok(Some(record))
}

/// Checks the disk before each change, so it works again after a crash at any point.
pub fn put_back(game: &Game) -> Result<()> {
    let addon = addon_folder(&game.addons);
    match &game.before {
        Before::Folder => put_back_folder(&game.addons),
        Before::Link { target } => {
            if link_target(&addon).as_ref() == Some(target) {
                return Ok(());
            }
            remove_any_link(&addon)?;
            refuse_a_real_folder(&addon)?;
            link_folder(target, &addon)
        }
        Before::Nothing => remove_any_link(&addon),
    }
}

fn put_back_folder(addons: &Path) -> Result<()> {
    let addon = addon_folder(addons);
    let backup = backup_folder(addons)?;
    if fs::symlink_metadata(&backup).is_err() {
        // The rename already ran, or the start stopped before it.
        if !is_real_folder(&addon) {
            remove_any_link(&addon)?;
            bail!(
                "{} and its backup are both missing. Install the addon again",
                addon.display()
            );
        }
        return Ok(());
    }
    remove_any_link(&addon)?;
    refuse_a_real_folder(&addon)?;
    fs::rename(&backup, &addon).with_context(|| format!("cannot move {} back", backup.display()))
}

/// A real folder holds files. Dev mode never deletes one.
fn refuse_a_real_folder(addon: &Path) -> Result<()> {
    if fs::symlink_metadata(addon).is_ok() {
        bail!(
            "{} is a real folder now, so dev mode leaves it. Check it, delete the one you don't need, then run cargo run -- dev --end",
            addon.display()
        );
    }
    Ok(())
}

fn is_real_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn link_target(path: &Path) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    fs::read_link(path).ok()
}

fn remove_any_link(path: &Path) -> Result<()> {
    if link_target(path).is_none() {
        return Ok(());
    }
    remove_link(path)
}

fn remove_link(path: &Path) -> Result<()> {
    // Windows makes a folder link as a folder, so it also removes it as one.
    let removed = if cfg!(windows) {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    removed.with_context(|| format!("cannot remove the link {}", path.display()))
}

fn link_folder(target: &Path, at: &Path) -> Result<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, at);
    made.with_context(|| format!("cannot link {} to {}", at.display(), target.display()))
}

/// Links each file of `addon/transport` into `addon/GnomishRelay`, as the install copies
/// them. Git ignores these links.
pub fn link_transport(addon: &Path) -> Result<()> {
    let transport = addon
        .parent()
        .context("the addon folder has no parent")?
        .join("transport");
    for entry in fs::read_dir(&transport)? {
        let name = entry?.file_name();
        let at = addon.join(&name);
        if fs::symlink_metadata(&at).is_ok() {
            continue;
        }
        link_file(&Path::new("..").join("transport").join(&name), &at)?;
    }
    Ok(())
}

fn link_file(target: &Path, at: &Path) -> Result<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, at);
    made.with_context(|| format!("cannot link {}", at.display()))
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    /// A game folder with `Interface/AddOns`, and a checkout with `addon/GnomishRelay`.
    struct Machine {
        root: tempfile::TempDir,
    }

    impl Machine {
        fn new() -> Machine {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("checkout/addon/GnomishRelay")).unwrap();
            fs::create_dir_all(root.path().join("checkout/addon/transport")).unwrap();
            fs::create_dir_all(root.path().join("data")).unwrap();
            Machine { root }
        }

        fn data(&self) -> PathBuf {
            self.root.path().join("data")
        }

        fn checkout(&self) -> PathBuf {
            self.root.path().join("checkout/addon/GnomishRelay")
        }

        /// The `AddOns` folder of game `n`, in the state `before`.
        fn game(&self, n: usize, before: &Before) -> PathBuf {
            let addons = self.root.path().join(format!("game{n}/Interface/AddOns"));
            fs::create_dir_all(&addons).unwrap();
            let addon = addons.join(ADDON);
            match before {
                Before::Folder => {
                    fs::create_dir(&addon).unwrap();
                    fs::write(addon.join("GnomishRelay.toc"), format!("game {n}")).unwrap();
                }
                Before::Link { target } => {
                    fs::create_dir_all(target).unwrap();
                    std::os::unix::fs::symlink(target, &addon).unwrap();
                }
                Before::Nothing => {}
            }
            addons
        }

        fn other_link(&self, n: usize) -> Before {
            Before::Link {
                target: self.root.path().join(format!("elsewhere{n}")),
            }
        }
    }

    /// The state of one game, as a test compares it.
    fn state(addons: &Path) -> (Before, Option<String>, bool) {
        let addon = addon_folder(addons);
        let toc = fs::read_to_string(addon.join("GnomishRelay.toc")).ok();
        let backup = fs::symlink_metadata(backup_folder(addons).unwrap()).is_ok();
        (note(addons).unwrap(), toc, backup)
    }

    fn start(machine: &Machine, record: &Record, steps: usize) {
        write_record(&machine.data(), record).unwrap();
        for step in start_steps(record).iter().take(steps) {
            apply(step, &machine.checkout()).unwrap();
        }
    }

    #[test]
    fn a_folder_goes_to_the_backup_and_comes_back_with_its_files() {
        let machine = Machine::new();
        let addons = machine.game(1, &Before::Folder);
        let before = state(&addons);
        let record = plan(std::slice::from_ref(&addons), Release::Ran).unwrap();

        start(&machine, &record, usize::MAX);
        let during = note(&addons).unwrap();
        let ended = end(&machine.data()).unwrap();

        assert_eq!(
            during,
            Before::Link {
                target: machine.checkout()
            }
        );
        assert_eq!(ended, Some(record));
        assert_eq!(state(&addons), before);
        assert!(!machine.data().join(RECORD_FILE).exists());
    }

    #[test]
    fn a_link_of_the_developer_comes_back_to_its_old_target() {
        let machine = Machine::new();
        let addons = machine.game(1, &machine.other_link(1));
        let before = state(&addons);
        let record = plan(std::slice::from_ref(&addons), Release::Stopped).unwrap();

        start(&machine, &record, usize::MAX);
        end(&machine.data()).unwrap();

        assert_eq!(state(&addons), before);
    }

    #[test]
    fn a_game_without_the_addon_ends_without_it() {
        let machine = Machine::new();
        let addons = machine.game(1, &Before::Nothing);
        let record = plan(std::slice::from_ref(&addons), Release::Stopped).unwrap();

        start(&machine, &record, usize::MAX);
        end(&machine.data()).unwrap();

        assert_eq!(state(&addons), (Before::Nothing, None, false));
    }

    /// The start state of kind `kind`: a folder, a link, or nothing.
    fn kind_of(machine: &Machine, kind: usize) -> Before {
        match kind {
            0 => Before::Folder,
            1 => machine.other_link(1),
            _ => Before::Nothing,
        }
    }

    #[test]
    fn a_crash_after_any_step_of_the_start_ends_as_it_began() {
        for kind in 0..3 {
            for crash_after in 0..=2 {
                let machine = Machine::new();
                let before = kind_of(&machine, kind);
                let addons = machine.game(1, &before);
                let at_start = state(&addons);
                let record = plan(std::slice::from_ref(&addons), Release::Ran).unwrap();

                start(&machine, &record, crash_after);
                end(&machine.data()).unwrap();

                assert_eq!(
                    state(&addons),
                    at_start,
                    "{before:?}, crash after {crash_after}"
                );
            }
        }
    }

    #[test]
    fn a_second_end_finds_no_record_and_changes_nothing() {
        let machine = Machine::new();
        let addons = machine.game(1, &Before::Folder);
        let record = plan(std::slice::from_ref(&addons), Release::Ran).unwrap();
        start(&machine, &record, usize::MAX);
        end(&machine.data()).unwrap();
        let after_first = state(&addons);

        let second = end(&machine.data()).unwrap();

        assert_eq!(second, None);
        assert_eq!(state(&addons), after_first);
    }

    #[test]
    fn a_new_folder_next_to_the_backup_stops_the_end_and_both_stay() {
        let machine = Machine::new();
        let addons = machine.game(1, &Before::Folder);
        let record = plan(std::slice::from_ref(&addons), Release::Ran).unwrap();
        start(&machine, &record, usize::MAX);
        // The CurseForge app installs the addon again during dev mode.
        remove_link(&addon_folder(&addons)).unwrap();
        fs::create_dir(addon_folder(&addons)).unwrap();

        let error = end(&machine.data()).unwrap_err();

        assert!(format!("{error:#}").contains("real folder"), "{error:#}");
        assert!(is_real_folder(&addon_folder(&addons)));
        assert!(is_real_folder(&backup_folder(&addons).unwrap()));
        assert!(
            machine.data().join(RECORD_FILE).exists(),
            "the record stays"
        );
    }

    #[test]
    fn a_backup_from_before_stops_the_start_before_any_change() {
        let machine = Machine::new();
        let addons = machine.game(1, &Before::Folder);
        fs::create_dir(backup_folder(&addons).unwrap()).unwrap();

        let error = plan(std::slice::from_ref(&addons), Release::Ran).unwrap_err();

        assert!(format!("{error:#}").contains("already there"), "{error:#}");
        assert!(is_real_folder(&addon_folder(&addons)));
    }

    #[test]
    fn a_record_that_is_not_json_is_an_error_and_stays() {
        let machine = Machine::new();
        fs::write(machine.data().join(RECORD_FILE), "{").unwrap();

        assert!(end(&machine.data()).is_err());
        assert!(machine.data().join(RECORD_FILE).exists());
    }

    #[test]
    fn the_transport_files_get_links_and_a_real_file_stays() {
        let machine = Machine::new();
        let transport = machine.checkout().parent().unwrap().join("transport");
        fs::write(transport.join("Strip.lua"), "strip").unwrap();
        fs::write(transport.join("Codec.lua"), "codec").unwrap();
        fs::write(machine.checkout().join("Codec.lua"), "mine").unwrap();

        link_transport(&machine.checkout()).unwrap();
        link_transport(&machine.checkout()).unwrap();

        let strip = machine.checkout().join("Strip.lua");
        assert_eq!(fs::read_to_string(&strip).unwrap(), "strip");
        assert_eq!(
            fs::read_link(&strip).unwrap(),
            Path::new("../transport/Strip.lua")
        );
        assert_eq!(
            fs::read_to_string(machine.checkout().join("Codec.lua")).unwrap(),
            "mine"
        );
    }

    /// Deterministic numbers for the seeded test.
    struct Seeded(u64);

    impl Seeded {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % n as u64).unwrap()
        }
    }

    #[test]
    fn random_games_and_crashes_always_end_as_they_began() {
        let mut rng = Seeded(0x9E37_79B9_7F4A_7C15);
        for round in 0..200 {
            let machine = Machine::new();
            let count = 1 + rng.below(3);
            let mut games = Vec::new();
            for n in 0..count {
                let before = match rng.below(3) {
                    1 => machine.other_link(n),
                    kind => kind_of(&machine, kind),
                };
                games.push(machine.game(n, &before));
            }
            let at_start: Vec<_> = games.iter().map(|g| state(g)).collect();
            let record = plan(&games, Release::Ran).unwrap();
            let steps = start_steps(&record).len();

            start(&machine, &record, rng.below(steps + 1));
            // A crash during the end too: some games come back, and the record stays.
            for game in record.games.iter().take(rng.below(count + 1)) {
                put_back(game).unwrap();
            }
            end(&machine.data()).unwrap();
            end(&machine.data()).unwrap();

            let at_end: Vec<_> = games.iter().map(|g| state(g)).collect();
            assert_eq!(at_end, at_start, "round {round}");
        }
    }
}
