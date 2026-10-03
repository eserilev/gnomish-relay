//! The WoW clients that Gnomish Relay supports (SPEC.md 7.9).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WowClient {
    Forever,
    Anniversary,
}

impl WowClient {
    /// In search order: on a tie, the first one wins.
    pub const ALL: [WowClient; 2] = [WowClient::Forever, WowClient::Anniversary];

    /// The folder of the client inside `World of Warcraft`.
    pub fn folder(self) -> &'static str {
        match self {
            WowClient::Forever => "_classic_beta_",
            WowClient::Anniversary => "_anniversary_",
        }
    }

    /// The `## Interface` number of the client: major * 10000 + minor * 100 + patch.
    pub fn interface(self) -> u32 {
        match self {
            WowClient::Forever => 16001,
            WowClient::Anniversary => 20506,
        }
    }

    /// The name of the client in fixture files, such as `forever-1.60.1.70124.json`.
    pub fn name(self) -> &'static str {
        match self {
            WowClient::Forever => "forever",
            WowClient::Anniversary => "anniversary",
        }
    }

    /// The name that players see, in `status` and in the README.
    pub fn title(self) -> &'static str {
        match self {
            WowClient::Forever => "WoW: Forever",
            WowClient::Anniversary => "TBC Anniversary",
        }
    }

    pub fn of_folder_name(name: &str) -> Option<WowClient> {
        WowClient::ALL.into_iter().find(|c| c.folder() == name)
    }

    /// The client of a build, from the major version of its interface number. Each
    /// supported client has its own major version.
    pub fn of_interface(interface: u32) -> Option<WowClient> {
        WowClient::ALL
            .into_iter()
            .find(|c| c.interface() / 10_000 == interface / 10_000)
    }
}

/// The game folders that the desktop app serves: `configured` first, then each other
/// client folder next to it. Battle.net puts every client in one `World of Warcraft`
/// folder, so a player with two clients needs no switch.
pub fn served_games(configured: &Path) -> Vec<PathBuf> {
    let mut games = vec![configured.to_owned()];
    let name = configured.file_name().and_then(OsStr::to_str);
    if name.and_then(WowClient::of_folder_name).is_none() {
        return games;
    }
    let Some(install) = configured.parent() else {
        return games;
    };
    for client in WowClient::ALL {
        let game = install.join(client.folder());
        if game != configured && game.is_dir() {
            games.push(game);
        }
    }
    games
}

/// The `## Interface` of every addon that the desktop app writes. It lists every client,
/// so one file loads in each.
pub const TOC_INTERFACE: &str = "16001, 20506";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toc_interface_lists_every_client_in_order() {
        let numbers: Vec<String> = WowClient::ALL
            .iter()
            .map(|c| c.interface().to_string())
            .collect();
        assert_eq!(TOC_INTERFACE, numbers.join(", "));
    }

    #[test]
    fn a_folder_name_gives_its_client() {
        assert_eq!(
            WowClient::of_folder_name("_anniversary_"),
            Some(WowClient::Anniversary)
        );
        assert_eq!(
            WowClient::of_folder_name("_classic_beta_"),
            Some(WowClient::Forever)
        );
        assert_eq!(WowClient::of_folder_name("_retail_"), None);
    }

    #[test]
    fn a_patch_of_a_client_still_gives_that_client() {
        assert_eq!(WowClient::of_interface(20507), Some(WowClient::Anniversary));
        assert_eq!(WowClient::of_interface(16101), Some(WowClient::Forever));
        assert_eq!(WowClient::of_interface(120_001), None);
    }

    #[test]
    fn the_configured_game_comes_first_and_each_other_client_next_to_it_follows() {
        let wow = tempfile::tempdir().unwrap();
        let forever = wow.path().join("_classic_beta_");
        let anniversary = wow.path().join("_anniversary_");
        std::fs::create_dir_all(&forever).unwrap();
        std::fs::create_dir_all(&anniversary).unwrap();
        std::fs::create_dir_all(wow.path().join("_retail_")).unwrap();

        assert_eq!(
            served_games(&anniversary),
            [anniversary.clone(), forever.clone()]
        );
        assert_eq!(served_games(&forever), [forever, anniversary]);
    }

    #[test]
    fn a_client_that_is_not_installed_is_not_served() {
        let wow = tempfile::tempdir().unwrap();
        let forever = wow.path().join("_classic_beta_");
        std::fs::create_dir_all(&forever).unwrap();

        assert_eq!(served_games(&forever), [forever]);
    }

    #[test]
    fn a_game_folder_with_another_name_is_served_alone() {
        let wow = tempfile::tempdir().unwrap();
        let custom = wow.path().join("my wow");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::create_dir_all(wow.path().join("_anniversary_")).unwrap();

        assert_eq!(served_games(&custom), [custom]);
    }

    #[test]
    fn each_client_has_its_own_folder_name_and_major_version() {
        let [forever, anniversary] = WowClient::ALL;
        assert_ne!(forever.folder(), anniversary.folder());
        assert_ne!(forever.name(), anniversary.name());
        assert_ne!(
            forever.interface() / 10_000,
            anniversary.interface() / 10_000
        );
    }
}
