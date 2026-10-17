//! The apps that share the bridge: which app a strip belongs to (SPEC.md 9.7,
//! decision 2), and the Lua global that each file of an app sets (decision 5). Each app
//! reads only its own globals, so one app never overwrites a value that the other app is
//! about to read.

use crate::ascii::push_bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Relay,
    Timeways,
    Workshop,
}

pub const ALL_APPS: [App; 3] = [App::Relay, App::Timeways, App::Workshop];

/// The result of the tag check under the key of one app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagCheck {
    pub app: App,
    pub tag_ok: bool,
}

/// Why a strip goes to no app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unrouted {
    /// No key verifies the tag.
    BadTag,
    /// Two or more keys verify the tag. Only equal keys do that, and the bridge refuses them.
    Ambiguous,
}

/// The app whose key verifies the tag of a strip (S29). The tag checks come in as
/// bools, so S29 proves the choice, not the cryptography.
///
/// # Errors
///
/// `BadTag` when no key verifies the tag, `Ambiguous` when two or more do.
pub fn route(checks: &[TagCheck]) -> Result<App, Unrouted> {
    let mut verified: usize = 0;
    let mut found = None;
    let mut i = 0;
    while i < checks.len() {
        if checks[i].tag_ok {
            verified += 1;
            found = Some(checks[i].app);
        }
        i += 1;
    }
    verdict(verified, found)
}

/// `found` is the last app that verified, of `verified` apps.
fn verdict(verified: usize, found: Option<App>) -> Result<App, Unrouted> {
    let Some(app) = found else {
        return Err(Unrouted::BadTag);
    };
    if verified == 1 {
        Ok(app)
    } else {
        Err(Unrouted::Ambiguous)
    }
}

const RELAY_SLOT_DATA: [u8; 21] = *b"GnomishRelay_SlotData";
const TIMEWAYS_SLOT_DATA: [u8; 17] = *b"Timeways_SlotData";
const WORKSHOP_SLOT_DATA: [u8; 24] = *b"GnomishWorkshop_SlotData";
const RELAY_RESTORE: [u8; 20] = *b"GnomishRelay_Restore";
const TIMEWAYS_RESTORE: [u8; 16] = *b"Timeways_Restore";
const WORKSHOP_RESTORE: [u8; 23] = *b"GnomishWorkshop_Restore";
const RELAY_LIVE: [u8; 17] = *b"GnomishRelay_Live";
const TIMEWAYS_LIVE: [u8; 13] = *b"Timeways_Live";
const WORKSHOP_LIVE: [u8; 20] = *b"GnomishWorkshop_Live";

/// The global of the slot body.
pub fn push_slot_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_SLOT_DATA),
        App::Timeways => push_bytes(out, &TIMEWAYS_SLOT_DATA),
        App::Workshop => push_bytes(out, &WORKSHOP_SLOT_DATA),
    }
}

/// The global of the restore file.
pub fn push_restore_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_RESTORE),
        App::Timeways => push_bytes(out, &TIMEWAYS_RESTORE),
        App::Workshop => push_bytes(out, &WORKSHOP_RESTORE),
    }
}

/// The global of the live file.
pub fn push_live_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_LIVE),
        App::Timeways => push_bytes(out, &TIMEWAYS_LIVE),
        App::Workshop => push_bytes(out, &WORKSHOP_LIVE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(push: fn(&mut Vec<u8>, App), app: App) -> Vec<u8> {
        let mut out = Vec::new();
        push(&mut out, app);
        out
    }

    fn check(app: App, tag_ok: bool) -> TagCheck {
        TagCheck { app, tag_ok }
    }

    #[test]
    fn a_strip_that_only_the_relay_key_verifies_goes_to_the_relay() {
        let checks = [check(App::Relay, true), check(App::Timeways, false)];
        assert_eq!(route(&checks), Ok(App::Relay));
    }

    #[test]
    fn a_strip_that_only_the_timeways_key_verifies_goes_to_timeways() {
        let checks = [check(App::Relay, false), check(App::Timeways, true)];
        assert_eq!(route(&checks), Ok(App::Timeways));
    }

    #[test]
    fn a_strip_that_no_key_verifies_has_a_bad_tag() {
        let checks = [check(App::Relay, false), check(App::Timeways, false)];
        assert_eq!(route(&checks), Err(Unrouted::BadTag));
    }

    #[test]
    fn a_strip_with_no_keys_has_a_bad_tag() {
        assert_eq!(route(&[]), Err(Unrouted::BadTag));
    }

    #[test]
    fn a_strip_that_both_keys_verify_is_ambiguous() {
        let checks = [check(App::Relay, true), check(App::Timeways, true)];
        assert_eq!(route(&checks), Err(Unrouted::Ambiguous));
    }

    #[test]
    fn a_strip_that_only_the_workshop_key_verifies_goes_to_the_workshop() {
        let checks = [
            check(App::Relay, false),
            check(App::Timeways, false),
            check(App::Workshop, true),
        ];
        assert_eq!(route(&checks), Ok(App::Workshop));
    }

    #[test]
    fn a_strip_that_three_keys_verify_is_ambiguous() {
        let checks = [
            check(App::Relay, true),
            check(App::Timeways, true),
            check(App::Workshop, true),
        ];
        assert_eq!(route(&checks), Err(Unrouted::Ambiguous));
    }

    #[test]
    fn the_order_of_the_checks_does_not_change_the_app() {
        let checks = [check(App::Timeways, true), check(App::Relay, false)];
        assert_eq!(route(&checks), Ok(App::Timeways));
    }

    #[test]
    fn all_apps_holds_each_app_once() {
        for app in [App::Relay, App::Timeways, App::Workshop] {
            let times = ALL_APPS.iter().filter(|listed| **listed == app).count();
            assert_eq!(times, 1, "{app:?}");
        }
    }

    #[test]
    fn each_app_has_its_own_globals() {
        assert_eq!(
            global(push_slot_global, App::Relay),
            b"GnomishRelay_SlotData"
        );
        assert_eq!(
            global(push_slot_global, App::Timeways),
            b"Timeways_SlotData"
        );
        assert_eq!(
            global(push_restore_global, App::Relay),
            b"GnomishRelay_Restore"
        );
        assert_eq!(
            global(push_restore_global, App::Timeways),
            b"Timeways_Restore"
        );
        assert_eq!(global(push_live_global, App::Relay), b"GnomishRelay_Live");
        assert_eq!(global(push_live_global, App::Timeways), b"Timeways_Live");
        assert_eq!(
            global(push_slot_global, App::Workshop),
            b"GnomishWorkshop_SlotData"
        );
        assert_eq!(
            global(push_restore_global, App::Workshop),
            b"GnomishWorkshop_Restore"
        );
        assert_eq!(
            global(push_live_global, App::Workshop),
            b"GnomishWorkshop_Live"
        );
    }
}
