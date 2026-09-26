//! The apps that share the bridge: which app a strip belongs to (SPEC.md 9.7,
//! decision 2), and the Lua global that each file of an app sets (decision 5). Each app
//! reads only its own globals, so one app never overwrites a value that the other app is
//! about to read.

use crate::ascii::push_bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Relay,
    Timeways,
}

/// Why a strip goes to no app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unrouted {
    /// No key verifies the tag.
    BadTag,
    /// Both keys verify the tag. Only equal keys do that, and the bridge refuses them.
    Ambiguous,
}

/// The app whose key verifies the tag of a strip (S29). The tag checks come in as
/// bools, so S29 proves the choice, not the cryptography.
///
/// # Errors
///
/// `BadTag` when no key verifies the tag, `Ambiguous` when both do.
pub fn route(relay_tag_ok: bool, timeways_tag_ok: bool) -> Result<App, Unrouted> {
    if relay_tag_ok {
        if timeways_tag_ok {
            return Err(Unrouted::Ambiguous);
        }
        return Ok(App::Relay);
    }
    if timeways_tag_ok {
        return Ok(App::Timeways);
    }
    Err(Unrouted::BadTag)
}

const RELAY_SLOT_DATA: [u8; 21] = *b"GnomishRelay_SlotData";
const TIMEWAYS_SLOT_DATA: [u8; 17] = *b"Timeways_SlotData";
const RELAY_RESTORE: [u8; 20] = *b"GnomishRelay_Restore";
const TIMEWAYS_RESTORE: [u8; 16] = *b"Timeways_Restore";
const RELAY_LIVE: [u8; 17] = *b"GnomishRelay_Live";
const TIMEWAYS_LIVE: [u8; 13] = *b"Timeways_Live";

/// The global of the slot body.
pub fn push_slot_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_SLOT_DATA),
        App::Timeways => push_bytes(out, &TIMEWAYS_SLOT_DATA),
    }
}

/// The global of the restore file.
pub fn push_restore_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_RESTORE),
        App::Timeways => push_bytes(out, &TIMEWAYS_RESTORE),
    }
}

/// The global of the live file.
pub fn push_live_global(out: &mut Vec<u8>, app: App) {
    match app {
        App::Relay => push_bytes(out, &RELAY_LIVE),
        App::Timeways => push_bytes(out, &TIMEWAYS_LIVE),
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

    #[test]
    fn a_strip_that_only_the_relay_key_verifies_goes_to_the_relay() {
        assert_eq!(route(true, false), Ok(App::Relay));
    }

    #[test]
    fn a_strip_that_only_the_timeways_key_verifies_goes_to_timeways() {
        assert_eq!(route(false, true), Ok(App::Timeways));
    }

    #[test]
    fn a_strip_that_no_key_verifies_has_a_bad_tag() {
        assert_eq!(route(false, false), Err(Unrouted::BadTag));
    }

    #[test]
    fn a_strip_that_both_keys_verify_is_ambiguous() {
        assert_eq!(route(true, true), Err(Unrouted::Ambiguous));
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
    }
}
