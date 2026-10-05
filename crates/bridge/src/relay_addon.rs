//! The relay addon on the disk. Players get it only from `CurseForge`, so the desktop app
//! reads it and never writes it (SPEC.md 11.3).

use std::fs;
use std::path::Path;

use protocol::apps::App;
use protocol::version::{VersionFit, version_fit};

use crate::install::relay_dir;
use crate::story::UPDATE_BRIDGE;

pub const GET_ADDON: &str = "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW.";
pub const UPDATE_ADDON: &str = "Update Gnomish Relay in the CurseForge app, then restart WoW.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayAddon {
    Missing,
    Installed(VersionFit),
}

pub fn find(addons: &Path) -> RelayAddon {
    let Some(dir) = relay_dir(addons) else {
        return RelayAddon::Missing;
    };
    // An addon with no version predates the range of every desktop app.
    let fit = app_version(&dir).map_or(VersionFit::TooOld, |v| version_fit(App::Relay, v));
    RelayAddon::Installed(fit)
}

/// `version` of `ns.App` in `App.lua`: the number that the addon sends in `ver=` (SPEC.md 7.7).
fn app_version(dir: &Path) -> Option<u32> {
    let app = fs::read_to_string(dir.join("App.lua")).ok()?;
    let value = app
        .lines()
        .find_map(|line| line.trim().strip_prefix("version ="))?;
    value.trim().trim_end_matches(',').parse().ok()
}

/// What the player does next, or `None` when the addon fits this desktop app.
pub fn next_step(addon: RelayAddon) -> Option<&'static str> {
    match addon {
        RelayAddon::Missing => Some(GET_ADDON),
        RelayAddon::Installed(VersionFit::TooOld) => Some(UPDATE_ADDON),
        RelayAddon::Installed(VersionFit::TooNew) => Some(UPDATE_BRIDGE),
        RelayAddon::Installed(VersionFit::Supported) => None,
    }
}

pub fn status_line(addon: RelayAddon) -> String {
    match addon {
        RelayAddon::Missing => format!("Addon: missing. {GET_ADDON}"),
        RelayAddon::Installed(VersionFit::TooOld) => format!("Addon: too old. {UPDATE_ADDON}"),
        RelayAddon::Installed(VersionFit::TooNew) => {
            format!("Addon: newer than the desktop app. {UPDATE_BRIDGE}")
        }
        RelayAddon::Installed(VersionFit::Supported) => "Addon: OK".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use protocol::version::{newest, oldest};

    use crate::install::ADDON;

    fn addon_with_app_lua(app_lua: &str) -> tempfile::TempDir {
        let addons = tempfile::tempdir().unwrap();
        let dir = addons.path().join(ADDON);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("App.lua"), app_lua).unwrap();
        addons
    }

    fn app_lua_of_version(n: u32) -> String {
        format!(
            "local _, ns = ...\n\nns.App = {{\n\ttitle = \"Gnomish Relay\",\n\tversion = {n},\n}}\n"
        )
    }

    #[test]
    fn with_no_addon_folder_the_player_gets_the_curseforge_link() {
        let addons = tempfile::tempdir().unwrap();

        let found = find(addons.path());

        assert_eq!(found, RelayAddon::Missing);
        assert_eq!(
            next_step(found),
            Some(
                "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW."
            )
        );
    }

    #[test]
    fn the_addon_of_this_repo_fits_this_desktop_app() {
        let addons = addon_with_app_lua(include_str!("../../../addon/GnomishRelay/App.lua"));

        let found = find(addons.path());

        assert_eq!(found, RelayAddon::Installed(VersionFit::Supported));
        assert_eq!(next_step(found), None);
        assert_eq!(status_line(found), "Addon: OK");
    }

    #[test]
    fn an_older_addon_is_updated_in_the_curseforge_app() {
        let addons = addon_with_app_lua(&app_lua_of_version(oldest(App::Relay) - 1));

        let found = find(addons.path());

        assert_eq!(found, RelayAddon::Installed(VersionFit::TooOld));
        assert_eq!(
            next_step(found),
            Some("Update Gnomish Relay in the CurseForge app, then restart WoW.")
        );
    }

    #[test]
    fn a_newer_addon_asks_for_a_newer_desktop_app() {
        let addons = addon_with_app_lua(&app_lua_of_version(newest(App::Relay) + 1));

        let found = find(addons.path());

        assert_eq!(found, RelayAddon::Installed(VersionFit::TooNew));
        assert_eq!(
            status_line(found),
            "Addon: newer than the desktop app. Update the desktop app: run gnomish-relay update."
        );
    }

    #[test]
    fn an_addon_with_no_version_counts_as_too_old() {
        let no_version = addon_with_app_lua("local _, ns = ...\nns.App = {}\n");
        let no_app_lua = tempfile::tempdir().unwrap();
        fs::create_dir(no_app_lua.path().join(ADDON)).unwrap();

        assert_eq!(
            find(no_version.path()),
            RelayAddon::Installed(VersionFit::TooOld)
        );
        assert_eq!(
            find(no_app_lua.path()),
            RelayAddon::Installed(VersionFit::TooOld)
        );
    }

    #[test]
    fn the_addon_folder_is_found_in_any_case() {
        let addons = tempfile::tempdir().unwrap();
        let dir = addons.path().join("gnomishrelay");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("App.lua"), app_lua_of_version(oldest(App::Relay))).unwrap();

        assert_eq!(
            find(addons.path()),
            RelayAddon::Installed(VersionFit::Supported)
        );
    }

    /// Dev mode links the repo into the game (SPEC.md 16.1).
    #[cfg(unix)]
    #[test]
    fn a_linked_repository_counts_as_an_installed_addon() {
        let addons = tempfile::tempdir().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addon/GnomishRelay");
        std::os::unix::fs::symlink(&repo, addons.path().join(ADDON)).unwrap();

        assert_eq!(
            find(addons.path()),
            RelayAddon::Installed(VersionFit::Supported)
        );
    }

    #[test]
    fn the_status_line_names_the_problem_before_the_next_step() {
        assert_eq!(
            status_line(RelayAddon::Missing),
            format!("Addon: missing. {GET_ADDON}")
        );
        assert_eq!(
            status_line(RelayAddon::Installed(VersionFit::TooOld)),
            "Addon: too old. Update Gnomish Relay in the CurseForge app, then restart WoW."
        );
    }
}
