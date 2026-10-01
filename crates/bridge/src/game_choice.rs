//! Which WoW folder setup uses, with no question (SPEC.md 11.3): the one given, the one
//! of the config, or the one that the player played last.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Result, bail};

use crate::dirs::Dirs;
use crate::install;

pub const NO_WOW: &str = "WoW not found. Start WoW once, then run gnomish-relay setup.";

/// `WTF/Account/<account>/<realm>/<character>/SavedVariables/<file>` is the deepest file.
const WTF_DEPTH: u8 = 6;

#[derive(Debug, PartialEq, Eq)]
pub enum Game {
    /// `--wow`, or the folder of the config.
    Known(PathBuf),
    /// The only folder that the search found.
    Only(PathBuf),
    /// The last played of several folders.
    Newest(PathBuf),
    Missing,
}

impl Game {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Game::Known(game) | Game::Only(game) | Game::Newest(game) => Some(game),
            Game::Missing => None,
        }
    }
}

/// A given folder wins, then the folder of the config while it exists, then the search.
pub fn choose(given: Option<&str>, config_wow: Option<&Path>, home: &Path) -> Game {
    if let Some(folder) = given {
        return Game::Known(install::game_folder(folder));
    }
    if let Some(wow) = config_wow.filter(|wow| wow.is_dir()) {
        return Game::Known(wow.to_owned());
    }
    from_search(&install::find_games(home))
}

pub fn from_search(games: &[PathBuf]) -> Game {
    match games {
        [] => Game::Missing,
        [only] => Game::Only(only.clone()),
        _ => newest(games).map_or(Game::Missing, |game| Game::Newest(game.clone())),
    }
}

/// The line that names the folder. With several, it also says how to take another one,
/// with `setup`, the setup command of the product.
pub fn game_line(game: &Game, setup: &str) -> Option<String> {
    match game {
        Game::Known(game) | Game::Only(game) => Some(format!("WoW: {}", game.display())),
        Game::Newest(game) => Some(format!(
            "Using WoW at {}. To use another one, run {setup} --wow <folder>.",
            game.display()
        )),
        Game::Missing => None,
    }
}

/// For the commands that need the game but no config, such as `selftest collect`.
pub fn require(dirs: &Dirs, given: Option<&str>) -> Result<PathBuf> {
    match choose(given, None, &dirs.home).folder() {
        Some(game) => Ok(game.to_owned()),
        None => bail!(NO_WOW),
    }
}

/// On a tie, the first folder of the search wins.
fn newest(games: &[PathBuf]) -> Option<&PathBuf> {
    let mut best: Option<(&PathBuf, Option<SystemTime>)> = None;
    for game in games {
        let played = last_played(game);
        if best.is_none_or(|(_, time)| played > time) {
            best = Some((game, played));
        }
    }
    best.map(|(game, _)| game)
}

/// WoW writes `Config.wtf` and the saved variables at each logout.
pub fn last_played(game: &Path) -> Option<SystemTime> {
    newest_file(&game.join("WTF"), WTF_DEPTH)
}

/// Links are not followed, so a link to a big folder costs nothing.
fn newest_file(dir: &Path, depth: u8) -> Option<SystemTime> {
    let mut newest = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let time = if kind.is_dir() && depth > 1 {
            newest_file(&entry.path(), depth - 1)
        } else if kind.is_file() {
            entry.metadata().ok().and_then(|meta| meta.modified().ok())
        } else {
            None
        };
        newest = newest.max(time);
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn saved_file(game: &Path, file: &str, age: u64) {
        let path = game.join("WTF").join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x").unwrap();
        let time = SystemTime::now() - Duration::from_secs(age);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    #[test]
    fn of_two_installs_setup_takes_the_one_with_the_newest_saved_file() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        saved_file(&old, "Config.wtf", 60);
        saved_file(&new, "Config.wtf", 3600);
        saved_file(
            &new,
            "Account/A/Realm/Char/SavedVariables/GnomishRelay.lua",
            10,
        );

        let game = from_search(&[old, new.clone()]);

        assert_eq!(game, Game::Newest(new));
    }

    #[test]
    fn an_install_with_no_wtf_folder_loses_to_one_that_was_played() {
        let root = tempfile::tempdir().unwrap();
        let never = root.path().join("never");
        let played = root.path().join("played");
        fs::create_dir_all(&never).unwrap();
        saved_file(&played, "Config.wtf", 86_400);

        assert_eq!(from_search(&[never, played.clone()]), Game::Newest(played));
    }

    #[test]
    fn with_no_saved_file_anywhere_the_first_install_wins() {
        let first = PathBuf::from("/none/a");

        assert_eq!(
            from_search(&[first.clone(), PathBuf::from("/none/b")]),
            Game::Newest(first)
        );
    }

    #[test]
    fn one_install_or_none_needs_no_choice() {
        let only = PathBuf::from("/games/wow");

        assert_eq!(from_search(std::slice::from_ref(&only)), Game::Only(only));
        assert_eq!(from_search(&[]), Game::Missing);
    }

    #[test]
    fn a_given_folder_wins_over_the_config_and_the_config_over_the_search() {
        let home = tempfile::tempdir().unwrap();
        let config_wow = home.path().join("wow");
        fs::create_dir_all(&config_wow).unwrap();
        let given = home.path().join("other").to_string_lossy().into_owned();

        assert_eq!(
            choose(Some(&given), Some(&config_wow), home.path()),
            Game::Known(install::game_folder(&given))
        );
        assert_eq!(
            choose(None, Some(&config_wow), home.path()),
            Game::Known(config_wow)
        );
    }

    #[test]
    fn a_config_folder_that_is_gone_starts_a_search() {
        let home = tempfile::tempdir().unwrap();

        let game = choose(None, Some(&home.path().join("gone")), home.path());

        assert_eq!(game, Game::Missing);
    }

    #[test]
    fn the_line_of_a_chosen_install_says_how_to_use_another_one() {
        let game = PathBuf::from("/games/wow");

        assert_eq!(
            game_line(&Game::Newest(game.clone()), "gnomish-relay setup").unwrap(),
            "Using WoW at /games/wow. To use another one, run gnomish-relay setup --wow <folder>."
        );
        assert_eq!(
            game_line(
                &Game::Newest(game.clone()),
                "gnomish-relay setup --timeways"
            )
            .unwrap(),
            "Using WoW at /games/wow. To use another one, run gnomish-relay setup --timeways --wow <folder>."
        );
        assert_eq!(
            game_line(&Game::Only(game), "gnomish-relay setup").unwrap(),
            "WoW: /games/wow"
        );
        assert_eq!(game_line(&Game::Missing, "gnomish-relay setup"), None);
    }
}
