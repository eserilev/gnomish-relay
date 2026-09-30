//! The reply to each message of an addon whose version this bridge does not speak
//! (SPEC.md 7.7 and 9.7, decision 17).

use protocol::apps::App;
use protocol::version::VersionFit;

use crate::relay_addon::UPDATE_ADDON;
use crate::story::{UPDATE_BRIDGE, UPDATE_TIMEWAYS};

pub fn update_text(app: App, fit: VersionFit) -> Option<&'static str> {
    match (app, fit) {
        (_, VersionFit::Supported) => None,
        (_, VersionFit::TooNew) => Some(UPDATE_BRIDGE),
        (App::Relay, VersionFit::TooOld) => Some(UPDATE_ADDON),
        (App::Timeways, VersionFit::TooOld) => Some(UPDATE_TIMEWAYS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supported_version_gets_no_update_text() {
        assert_eq!(update_text(App::Relay, VersionFit::Supported), None);
        assert_eq!(update_text(App::Timeways, VersionFit::Supported), None);
    }

    #[test]
    fn a_newer_addon_of_either_app_asks_to_update_the_desktop_program() {
        assert_eq!(
            update_text(App::Relay, VersionFit::TooNew),
            Some(UPDATE_BRIDGE)
        );
        assert_eq!(
            update_text(App::Timeways, VersionFit::TooNew),
            Some(UPDATE_BRIDGE)
        );
    }

    #[test]
    fn an_older_addon_of_either_app_asks_for_its_update() {
        assert_eq!(
            update_text(App::Timeways, VersionFit::TooOld),
            Some(UPDATE_TIMEWAYS)
        );
        assert_eq!(
            update_text(App::Relay, VersionFit::TooOld),
            Some("Update Gnomish Relay in the CurseForge app, then restart WoW.")
        );
    }
}
