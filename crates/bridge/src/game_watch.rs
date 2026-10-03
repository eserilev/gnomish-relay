//! Restarts the desktop app when another WoW client gets the addon (SPEC.md 7.9). The
//! bridge picks its games at its start, and the sandbox hides the private files of
//! only those games, so a new game needs a restart, not a change in place.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::auto_update::{Activity, CHECK_EVERY};
use crate::wow_client::served_games;

pub struct GameWatch {
    /// `[wow] path` of the config.
    configured: PathBuf,
    /// The games that this bridge serves since its start.
    served: Vec<PathBuf>,
    last_check: Option<Instant>,
}

impl GameWatch {
    pub fn new(configured: &Path, served: Vec<PathBuf>) -> GameWatch {
        GameWatch {
            configured: configured.to_owned(),
            served,
            last_check: None,
        }
    }

    /// A game to serve that this bridge does not serve, once a minute while nothing runs.
    /// The caller restarts the desktop app.
    pub fn new_game(&mut self, activity: Activity, now: Instant) -> Option<PathBuf> {
        if self
            .last_check
            .is_some_and(|last| now.duration_since(last) < CHECK_EVERY)
        {
            return None;
        }
        self.last_check = Some(now);
        if activity == Activity::Busy {
            return None;
        }
        let new = served_games(&self.configured)
            .into_iter()
            .find(|game| !self.served.contains(game))?;
        // One restart is enough: the new bridge serves the game from its start.
        self.served.push(new.clone());
        Some(new)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn add_addon(game: &Path) {
        fs::create_dir_all(game.join("Interface/AddOns/GnomishRelay")).unwrap();
    }

    #[test]
    fn a_client_that_gets_the_addon_later_is_a_new_game_once() {
        let wow = tempfile::tempdir().unwrap();
        let forever = wow.path().join("_classic_beta_");
        let anniversary = wow.path().join("_anniversary_");
        add_addon(&forever);
        let mut watch = GameWatch::new(&forever, served_games(&forever));
        let start = Instant::now();
        assert_eq!(watch.new_game(Activity::Idle, start), None);

        add_addon(&anniversary);

        let later = start + CHECK_EVERY;
        assert_eq!(watch.new_game(Activity::Idle, later), Some(anniversary));
        assert_eq!(watch.new_game(Activity::Idle, later + CHECK_EVERY), None);
    }

    #[test]
    fn a_new_game_waits_while_a_run_is_in_progress() {
        let wow = tempfile::tempdir().unwrap();
        let forever = wow.path().join("_classic_beta_");
        add_addon(&forever);
        let mut watch = GameWatch::new(&forever, served_games(&forever));
        add_addon(&wow.path().join("_anniversary_"));
        let start = Instant::now();

        assert_eq!(watch.new_game(Activity::Busy, start), None);
        assert!(
            watch
                .new_game(Activity::Idle, start + CHECK_EVERY)
                .is_some()
        );
    }

    #[test]
    fn the_check_runs_at_most_once_a_minute() {
        let wow = tempfile::tempdir().unwrap();
        let forever = wow.path().join("_classic_beta_");
        add_addon(&forever);
        let mut watch = GameWatch::new(&forever, served_games(&forever));
        let start = Instant::now();
        assert_eq!(watch.new_game(Activity::Idle, start), None);

        add_addon(&wow.path().join("_anniversary_"));

        assert_eq!(watch.new_game(Activity::Idle, start), None);
    }
}
