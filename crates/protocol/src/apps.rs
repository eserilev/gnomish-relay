//! The apps that share the bridge, and the Lua global that each file of an app sets
//! (SPEC.md 9.7, decision 5). Each app reads only its own globals, so one app never
//! overwrites a value that the other app is about to read.

use crate::ascii::push_bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Relay,
    Timeways,
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
