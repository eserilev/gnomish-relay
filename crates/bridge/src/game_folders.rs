//! The folders of one WoW game that the desktop app reads and writes (SPEC.md 7.9).

use std::path::{Path, PathBuf};

use crate::install::addons_dir;
use crate::wow_client::WowClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameFolders {
    /// The name of the game folder, such as `_anniversary_`. Two games can have an
    /// account folder with the same name, so the name of an account starts with it.
    pub name: String,
    pub addons: PathBuf,
    pub screenshots: PathBuf,
    /// `WTF/Account`, which holds the saved variables of each account.
    pub accounts: PathBuf,
}

impl GameFolders {
    pub fn of(game: &Path) -> GameFolders {
        let name = game.file_name().unwrap_or_default().to_string_lossy();
        GameFolders {
            name: name.into_owned(),
            addons: addons_dir(game),
            screenshots: game.join("Screenshots"),
            accounts: game.join("WTF").join("Account"),
        }
    }

    /// The name of the client, or the folder name for a folder with another name.
    pub fn title(&self) -> &str {
        match WowClient::of_folder_name(&self.name) {
            Some(client) => client.title(),
            None => &self.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_has_the_title_of_its_client_or_its_folder_name() {
        assert_eq!(
            GameFolders::of(Path::new("/w/_anniversary_")).title(),
            "TBC Anniversary"
        );
        assert_eq!(
            GameFolders::of(Path::new("/w/_classic_beta_")).title(),
            "WoW: Forever"
        );
        assert_eq!(GameFolders::of(Path::new("/w/my wow")).title(), "my wow");
    }

    #[test]
    fn the_folders_of_a_game_are_inside_it_and_named_after_it() {
        let game = Path::new("/wow/_anniversary_");

        let folders = GameFolders::of(game);

        assert_eq!(folders.name, "_anniversary_");
        assert_eq!(folders.addons, game.join("Interface").join("AddOns"));
        assert_eq!(folders.screenshots, game.join("Screenshots"));
        assert_eq!(folders.accounts, game.join("WTF").join("Account"));
    }
}
