//! The names of the files of each app in the game folder (SPEC.md 9.7, decision 5). The
//! slot folders and the saved variables of one app never share a name with the other.

use std::path::{Path, PathBuf};

use protocol::apps::App;

use crate::install::KEY_FILE;

/// The folder name of the addon. WoW names the saved variables file after it.
pub fn addon_name(app: App) -> &'static str {
    match app {
        App::Relay => "GnomishRelay",
        App::Timeways => "Timeways",
    }
}

/// The file in `WTF/Account/<account>/SavedVariables` that WoW writes at a `/reload`.
pub fn saved_variables_file(app: App) -> String {
    format!("{}.lua", addon_name(app))
}

/// The files of the game that no agent and no command of a game run reads (SPEC.md 6.6.3):
/// the strip keys, the saved chats of every account, and the prompts in the pixels of
/// the screenshots. Only `Key.lua` of each addon: a developer checkout links the addon
/// folder into a repository that an agent edits.
pub fn private_game_paths(addons: &Path, accounts: &Path, screenshots: &Path) -> Vec<PathBuf> {
    vec![
        addons.join(addon_name(App::Relay)).join(KEY_FILE),
        addons.join(addon_name(App::Timeways)).join(KEY_FILE),
        accounts.to_owned(),
        screenshots.to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_private_game_paths_are_the_keys_the_accounts_and_the_screenshots() {
        let game = Path::new("/games/wow");

        let paths = private_game_paths(
            &game.join("Interface/AddOns"),
            &game.join("WTF/Account"),
            &game.join("Screenshots"),
        );

        assert_eq!(
            paths,
            [
                game.join("Interface/AddOns/GnomishRelay/Key.lua"),
                game.join("Interface/AddOns/Timeways/Key.lua"),
                game.join("WTF/Account"),
                game.join("Screenshots"),
            ]
        );
    }

    #[test]
    fn each_app_has_its_own_saved_variables_file() {
        assert_eq!(saved_variables_file(App::Relay), "GnomishRelay.lua");
        assert_eq!(saved_variables_file(App::Timeways), "Timeways.lua");
    }
}
