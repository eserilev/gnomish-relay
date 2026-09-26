//! Checks a frame from the strip, finds its app, and returns its records (SPEC.md 6.3,
//! 9.7, S2, S11, S29).

use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use protocol::apps::{App, Unrouted, route};
use protocol::frame::{Reject, check_frame, decode_frame, signed_len};
use protocol::record::{Record, parse_records};
use sha2::Sha256;

pub const RELAY_KEY_FILE: &str = "strip.key";
pub const TIMEWAYS_KEY_FILE: &str = "timeways.key";

pub struct StripKey(Vec<u8>);

impl StripKey {
    pub fn from_hex(hex: &str) -> Result<StripKey> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            bail!("the strip key is not 64 hex digits");
        }
        let key = (0..32)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .context("the strip key is not hex")?;
        Ok(StripKey(key))
    }

    pub fn load(path: &Path) -> Result<StripKey> {
        let hex = std::fs::read_to_string(path).with_context(|| {
            format!(
                "cannot read the strip key at {}. Run scripts/dev-link.sh",
                path.display()
            )
        })?;
        StripKey::from_hex(&hex)
    }

    /// `None` when the file does not exist. Any other error stops the bridge.
    fn load_if_present(path: &Path) -> Result<Option<StripKey>> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            _ => StripKey::load(path).map(Some),
        }
    }

    /// The tag that the addon puts on a frame: the first 8 bytes of HMAC-SHA256.
    pub fn tag(&self, signed: &[u8]) -> [u8; 8] {
        let mut tag = [0; 8];
        if let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&self.0) {
            mac.update(signed);
            tag.copy_from_slice(&mac.finalize().into_bytes()[..8]);
        }
        tag
    }

    /// `verify_truncated_left` compares in constant time.
    fn verify(&self, signed: &[u8], tag: &[u8]) -> bool {
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&self.0) else {
            return false;
        };
        mac.update(signed);
        mac.verify_truncated_left(tag).is_ok()
    }
}

/// The key of each app. Named fields, not a list, so an index can never swap the apps
/// (SPEC.md 9.7, decision 2). With no Timeways key, no strip goes to Timeways.
pub struct KeySet {
    relay: StripKey,
    timeways: Option<StripKey>,
}

impl KeySet {
    /// Equal keys would make every strip `Ambiguous`, so they stop the bridge.
    pub fn new(relay: StripKey, timeways: Option<StripKey>) -> Result<KeySet> {
        if timeways.as_ref().is_some_and(|t| t.0 == relay.0) {
            bail!(
                "{TIMEWAYS_KEY_FILE} is the same as {RELAY_KEY_FILE}. Make a new key for Timeways"
            );
        }
        Ok(KeySet { relay, timeways })
    }

    /// `strip.key`, and `timeways.key` if it exists, from the config folder.
    pub fn load(config_dir: &Path) -> Result<KeySet> {
        let relay = StripKey::load(&config_dir.join(RELAY_KEY_FILE))?;
        let timeways = StripKey::load_if_present(&config_dir.join(TIMEWAYS_KEY_FILE))?;
        KeySet::new(relay, timeways)
    }

    pub fn has_timeways(&self) -> bool {
        self.timeways.is_some()
    }

    fn route(&self, signed: &[u8], tag: &[u8]) -> Result<App, Unrouted> {
        let relay_ok = self.relay.verify(signed, tag);
        let timeways_ok = self
            .timeways
            .as_ref()
            .is_some_and(|key| key.verify(signed, tag));
        route(relay_ok, timeways_ok)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rejected {
    NotAFrame,
    BadTag,
    /// Both keys verify the tag. `KeySet::new` refuses equal keys, so only a chance
    /// match of two 8-byte tags gets here.
    Ambiguous,
    /// A frame in the saved variables of one app that the key of another app signed.
    OtherApp,
    Stale,
    Future,
    BadRecords,
}

/// `bytes` can run past the end of the frame, as they do when read off a strip.
pub fn receive(bytes: &[u8], keys: &KeySet, now: u32) -> Result<(App, Vec<Record>), Rejected> {
    let Ok(frame) = decode_frame(bytes) else {
        return Err(Rejected::NotAFrame);
    };
    let routed = keys.route(&bytes[..signed_len(&frame)], &frame.tag);
    let app = match routed {
        Ok(app) => app,
        Err(Unrouted::BadTag) => return Err(Rejected::BadTag),
        Err(Unrouted::Ambiguous) => return Err(Rejected::Ambiguous),
    };
    // One key verified the tag, so only the time can fail here.
    match check_frame(frame.time, true, now) {
        Ok(()) => {}
        Err(Reject::BadTag) => return Err(Rejected::BadTag),
        Err(Reject::Stale) => return Err(Rejected::Stale),
        Err(Reject::Future) => return Err(Rejected::Future),
    }
    let records = parse_records(&frame.payload).map_err(|_| Rejected::BadRecords)?;
    Ok((app, records))
}

/// An outbox frame counts only for the app whose saved variables hold it (SPEC.md 9.7,
/// decision 3).
pub fn receive_for(
    app: App,
    bytes: &[u8],
    keys: &KeySet,
    now: u32,
) -> Result<Vec<Record>, Rejected> {
    let (signer, records) = receive(bytes, keys, now)?;
    if signer != app {
        return Err(Rejected::OtherApp);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::frame::encode_frame;

    const NOW: u32 = 1_790_211_079;
    const RECORD: &[u8] = b"tok\x1fc1\x1f3\x1f\x1f\x1f\x1fhi";

    fn key(byte: &str) -> StripKey {
        StripKey::from_hex(&byte.repeat(32)).unwrap()
    }

    fn relay_only() -> KeySet {
        KeySet::new(key("ab"), None).unwrap()
    }

    fn both() -> KeySet {
        KeySet::new(key("ab"), Some(key("cd"))).unwrap()
    }

    fn signed(time: u32, payload: &[u8], key: &StripKey) -> Vec<u8> {
        let mut wire = encode_frame(time, 1, payload, [0; 8]).unwrap();
        let body = wire.len() - 8;
        let tag = key.tag(&wire[..body]);
        wire[body..].copy_from_slice(&tag);
        wire
    }

    fn app_of(wire: &[u8], keys: &KeySet) -> Result<App, Rejected> {
        receive(wire, keys, NOW).map(|(app, _)| app)
    }

    #[test]
    fn a_signed_fresh_frame_gives_its_records() {
        let mut wire = signed(NOW, RECORD, &key("ab"));
        wire.extend([7; 40]);
        let (app, records) = receive(&wire, &relay_only(), NOW).unwrap();
        assert_eq!(app, App::Relay);
        assert_eq!(records[0].text, b"hi");
    }

    #[test]
    fn a_frame_signed_with_another_key_is_rejected() {
        let wire = signed(NOW, RECORD, &key("ef"));
        assert_eq!(app_of(&wire, &both()), Err(Rejected::BadTag));
    }

    #[test]
    fn a_frame_goes_to_the_app_whose_key_signed_it() {
        let keys = both();
        assert_eq!(
            app_of(&signed(NOW, RECORD, &key("ab")), &keys),
            Ok(App::Relay)
        );
        assert_eq!(
            app_of(&signed(NOW, RECORD, &key("cd")), &keys),
            Ok(App::Timeways)
        );
    }

    #[test]
    fn swapped_keys_swap_the_apps() {
        let swapped = KeySet::new(key("cd"), Some(key("ab"))).unwrap();
        assert_eq!(
            app_of(&signed(NOW, RECORD, &key("ab")), &swapped),
            Ok(App::Timeways)
        );
        assert_eq!(
            app_of(&signed(NOW, RECORD, &key("cd")), &swapped),
            Ok(App::Relay)
        );
    }

    #[test]
    fn with_no_timeways_key_a_timeways_frame_has_a_bad_tag() {
        let wire = signed(NOW, RECORD, &key("cd"));
        assert_eq!(app_of(&wire, &relay_only()), Err(Rejected::BadTag));
    }

    #[test]
    fn a_frame_that_both_keys_verify_is_ambiguous() {
        let equal = KeySet {
            relay: key("ab"),
            timeways: Some(key("ab")),
        };
        let wire = signed(NOW, RECORD, &key("ab"));
        assert_eq!(app_of(&wire, &equal), Err(Rejected::Ambiguous));
    }

    #[test]
    fn equal_keys_stop_the_bridge() {
        let error = KeySet::new(key("ab"), Some(key("ab"))).err().unwrap();
        assert!(error.to_string().contains("timeways.key is the same"));
    }

    #[test]
    fn an_outbox_frame_counts_only_for_the_app_of_its_key() {
        let keys = both();
        let timeways = signed(NOW, RECORD, &key("cd"));
        assert_eq!(
            receive_for(App::Relay, &timeways, &keys, NOW).err(),
            Some(Rejected::OtherApp)
        );
        assert!(receive_for(App::Timeways, &timeways, &keys, NOW).is_ok());
        let relay = signed(NOW, RECORD, &key("ab"));
        assert_eq!(
            receive_for(App::Timeways, &relay, &keys, NOW).err(),
            Some(Rejected::OtherApp)
        );
    }

    #[test]
    fn an_old_frame_is_rejected() {
        let wire = signed(NOW - 301, RECORD, &key("ab"));
        assert_eq!(app_of(&wire, &relay_only()), Err(Rejected::Stale));
    }

    #[test]
    fn a_frame_from_the_future_is_rejected() {
        let wire = signed(NOW + 61, RECORD, &key("ab"));
        assert_eq!(app_of(&wire, &relay_only()), Err(Rejected::Future));
    }

    #[test]
    fn bytes_that_are_not_a_frame_are_rejected() {
        assert_eq!(
            app_of(b"not a frame", &relay_only()),
            Err(Rejected::NotAFrame)
        );
    }

    #[test]
    fn a_signed_frame_with_broken_records_is_rejected() {
        let wire = signed(NOW, b"only\x1ftwo fields", &key("ab"));
        assert_eq!(app_of(&wire, &relay_only()), Err(Rejected::BadRecords));
    }

    #[test]
    fn a_key_must_be_32_hex_bytes() {
        assert!(StripKey::from_hex("abcd").is_err());
        assert!(StripKey::from_hex(&"zz".repeat(32)).is_err());
        assert!(StripKey::from_hex(&"é".repeat(32)).is_err());
    }

    fn config_with(keys: &[(&str, String)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, hex) in keys {
            std::fs::write(dir.path().join(name), hex).unwrap();
        }
        dir
    }

    #[test]
    fn no_timeways_key_file_means_no_timeways_key() {
        let dir = config_with(&[(RELAY_KEY_FILE, "ab".repeat(32))]);
        assert!(!KeySet::load(dir.path()).unwrap().has_timeways());
    }

    #[test]
    fn a_timeways_key_file_gives_the_timeways_key() {
        let dir = config_with(&[
            (RELAY_KEY_FILE, "ab".repeat(32)),
            (TIMEWAYS_KEY_FILE, "cd".repeat(32)),
        ]);
        assert!(KeySet::load(dir.path()).unwrap().has_timeways());
    }

    #[test]
    fn equal_key_files_stop_the_bridge() {
        let dir = config_with(&[
            (RELAY_KEY_FILE, "ab".repeat(32)),
            (TIMEWAYS_KEY_FILE, "AB".repeat(32) + "\n"),
        ]);
        assert!(KeySet::load(dir.path()).is_err());
    }

    #[test]
    fn a_broken_timeways_key_file_stops_the_bridge() {
        let dir = config_with(&[
            (RELAY_KEY_FILE, "ab".repeat(32)),
            (TIMEWAYS_KEY_FILE, "short".into()),
        ]);
        assert!(KeySet::load(dir.path()).is_err());
    }
}
