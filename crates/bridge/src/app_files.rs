//! The names of the files of each app in the game folder (SPEC.md 9.7, decision 5). The
//! slot folders and the saved variables of one app never share a name with the other.

use protocol::apps::App;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_app_has_its_own_saved_variables_file() {
        assert_eq!(saved_variables_file(App::Relay), "GnomishRelay.lua");
        assert_eq!(saved_variables_file(App::Timeways), "Timeways.lua");
    }
}
