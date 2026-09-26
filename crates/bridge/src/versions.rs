//! The reply to each message of an addon whose version this bridge does not speak
//! (SPEC.md 7.7 and 9.7, decision 17).

use protocol::apps::App;
use protocol::version::VersionFit;

use crate::story::{UPDATE_BRIDGE, UPDATE_TIMEWAYS};

/// The bridge writes the relay addon again at each start, so an old relay addon only
/// means that the game did not reload.
pub const RELOAD_RELAY: &str = "Type /reload in the game to load the new Gnomish Relay.";

pub fn update_text(app: App, fit: VersionFit) -> Option<&'static str> {
    match (app, fit) {
        (_, VersionFit::Supported) => None,
        (_, VersionFit::TooNew) => Some(UPDATE_BRIDGE),
        (App::Relay, VersionFit::TooOld) => Some(RELOAD_RELAY),
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
    fn an_older_timeways_asks_to_update_timeways_and_an_older_relay_asks_for_a_reload() {
        assert_eq!(
            update_text(App::Timeways, VersionFit::TooOld),
            Some(UPDATE_TIMEWAYS)
        );
        assert_eq!(
            update_text(App::Relay, VersionFit::TooOld),
            Some(RELOAD_RELAY)
        );
    }
}
