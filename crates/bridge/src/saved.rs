//! The reload fallback: signed frames in the saved variables file (SPEC.md 7.5).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const FILE: &str = "GnomishRelay.lua";
/// Saved variables hold at most 200 messages per chat, far below this.
const MAX_FILE: u64 = 16 * 1024 * 1024;
const KEY: &str = "[\"frame\"] = \"";

/// Every `["frame"] = "<hex>"` value in the file. The bridge checks each frame as it
/// checks a strip, so a frame from any place in the file is safe to take.
pub fn frames(text: &str) -> Vec<Vec<u8>> {
    text.match_indices(KEY)
        .filter_map(|(at, _)| {
            let rest = &text[at + KEY.len()..];
            let hex = &rest[..rest.find('"')?];
            from_hex(hex)
        })
        .collect()
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Watches the saved variables file of every account.
pub struct Watcher {
    accounts: PathBuf,
    seen: HashMap<PathBuf, SystemTime>,
}

impl Watcher {
    pub fn new(accounts: &Path) -> Watcher {
        Watcher {
            accounts: accounts.to_owned(),
            seen: HashMap::new(),
        }
    }

    /// The text of each file that is new or changed since the last call. The first
    /// call reads every file; old frames in them fail the time check.
    pub fn changed(&mut self) -> Vec<String> {
        let Ok(accounts) = fs::read_dir(&self.accounts) else {
            return Vec::new();
        };
        let mut texts = Vec::new();
        for account in accounts.flatten() {
            let path = account.path().join("SavedVariables").join(FILE);
            // `symlink_metadata` does not follow a link, so a link is skipped.
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_FILE || self.seen.get(&path) == Some(&modified) {
                continue;
            }
            self.seen.insert(path.clone(), modified);
            if let Ok(text) = fs::read_to_string(&path) {
                texts.push(text);
            }
        }
        texts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_come_from_every_frame_field_and_nothing_else() {
        let text = "GnomishRelayDB = {\n\t[\"outbox\"] = {\n\t\t{\n\t\t\t[\"frame\"] = \"6e5201\",\n\t\t},\n\t},\n\t[\"note\"] = \"[\\\"frame\\\"] = \\\"zz\\\"\",\n\t[\"frame\"] = \"ff00\",\n}\n";
        assert_eq!(frames(text), [vec![0x6e, 0x52, 0x01], vec![0xff, 0x00]]);
    }

    #[test]
    fn broken_hex_is_skipped() {
        assert!(frames("[\"frame\"] = \"abc\"").is_empty());
        assert!(frames("[\"frame\"] = \"zz\"").is_empty());
        assert!(frames("[\"frame\"] = \"ab").is_empty());
    }

    fn account_file(root: &Path) -> PathBuf {
        let dir = root.join("ACCOUNT1").join("SavedVariables");
        fs::create_dir_all(&dir).unwrap();
        dir.join(FILE)
    }

    #[test]
    fn a_file_is_read_again_only_after_it_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = account_file(root.path());
        fs::write(&file, "one").unwrap();
        let mut watcher = Watcher::new(root.path());
        assert_eq!(watcher.changed(), ["one"]);
        assert!(watcher.changed().is_empty());
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::write(&file, "two").unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(watcher.changed(), ["two"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_variables_file_that_is_a_link_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let file = account_file(root.path());
        let target = root.path().join("elsewhere.lua");
        fs::write(&target, "[\"frame\"] = \"00\"").unwrap();
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert!(Watcher::new(root.path()).changed().is_empty());
    }
}
