//! The work of the desktop app next to the messages: auto-update (SPEC.md 11.3), a game
//! that gets the addon later (7.9), and the Timeways lore build (11.4). Each can end in
//! a restart of the desktop app, which waits until no run is in progress.

use std::time::Instant;

use crate::auto_update::{Activity, AutoUpdater};
use crate::game_watch::GameWatch;
use crate::lore_job::{Finished, LoreJob};

#[derive(Default)]
pub struct Background {
    /// `None` with `auto_update = false` (SPEC.md 11.3).
    pub auto_update: Option<AutoUpdater>,
    /// `None` in tests: a restart needs the real program (SPEC.md 7.9).
    pub game_watch: Option<GameWatch>,
    /// Only with a story program in the config (SPEC.md 11.4).
    pub lore: Option<LoreJob>,
    /// Why the desktop app restarts, once no run is in progress.
    restart_due: Option<String>,
}

impl Background {
    pub fn new(
        auto_update: Option<AutoUpdater>,
        game_watch: Option<GameWatch>,
        lore: Option<LoreJob>,
    ) -> Background {
        Background {
            auto_update,
            game_watch,
            lore,
            restart_due: None,
        }
    }

    /// One step. Returns the reason for a restart when one is due and no run is in
    /// progress. The caller restarts the desktop app.
    pub fn step(&mut self, activity: Activity, now: Instant) -> Option<String> {
        if let Some(updater) = &mut self.auto_update {
            updater.tick(activity, now);
        }
        let new_game = self
            .game_watch
            .as_mut()
            .and_then(|watch| watch.new_game(activity, now));
        if let Some(game) = new_game {
            self.restart_due = Some(format!(
                "{}: the addon is there now, so the desktop app restarts to serve it",
                game.display()
            ));
        }
        if let Some(Finished::Built) = self.lore.as_mut().and_then(|job| job.tick(now)) {
            self.restart_due = Some(
                "timeways: the lore is ready, so the desktop app restarts to give it to the story program"
                    .into(),
            );
        }
        if activity == Activity::Busy {
            return None;
        }
        self.restart_due.take()
    }
}
