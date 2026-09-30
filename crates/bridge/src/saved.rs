//! What the bridge reads in a saved variables file: the signed frames of the reload
//! fallback (SPEC.md 7.5), the token of the addon (SPEC.md 7.6), and the results of the
//! self-test (SPEC.md 14.3).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use protocol::apps::App;

use crate::app_files::saved_variables_file;
use crate::fs_safe::read_at_most;

/// Saved variables hold at most 200 messages per chat, far below this.
const MAX_FILE: u64 = 16 * 1024 * 1024;

/// Every `["frame"] = "<hex>"` value in the file. The bridge checks each frame as it
/// checks a strip, so a frame from any place in the file is safe to take.
pub fn frames(text: &str) -> Vec<Vec<u8>> {
    hex_fields(text, "frame")
}

/// Every `["<name>"] = "<hex>"` value in the file. Hex needs no Lua escape, so the
/// escapes that WoW writes never matter.
pub fn hex_fields(text: &str, name: &str) -> Vec<Vec<u8>> {
    let key = format!("[\"{name}\"] = \"");
    text.match_indices(&key)
        .filter_map(|(at, _)| {
            let rest = &text[at + key.len()..];
            let hex = &rest[..rest.find('"')?];
            from_hex(hex)
        })
        .collect()
}

/// The text of a plain file under the size limit. A link is skipped. A Lua string can
/// hold any byte, so a byte that is not UTF-8 becomes U+FFFD. Hex fields stay whole.
fn read_plain(path: &Path) -> Option<(SystemTime, String)> {
    // `symlink_metadata` does not follow a link.
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = read_at_most(path, MAX_FILE).ok()??;
    Some((
        meta.modified().ok()?,
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// The newest `file` in the saved variables of all accounts, with its path.
pub fn newest(accounts: &Path, file: &str) -> Option<(PathBuf, String)> {
    let mut found: Vec<(SystemTime, PathBuf, String)> = Vec::new();
    for account in fs::read_dir(accounts).ok()?.flatten() {
        let path = account.path().join("SavedVariables").join(file);
        if let Some((modified, text)) = read_plain(&path) {
            found.push((modified, path, text));
        }
    }
    let (_, path, text) = found.into_iter().max_by_key(|(modified, _, _)| *modified)?;
    Some((path, text))
}

/// The token of the addon: the `token` field at the top of the table, one tab deep, as
/// WoW writes it (SPEC.md 7.6). Only an id that the addon can make counts.
pub fn saved_token(text: &str) -> Option<String> {
    let key = "\n\t[\"token\"] = \"";
    let at = text.find(key)? + key.len();
    let rest = &text[at..];
    let token = &rest[..rest.find('"')?];
    is_token(token).then(|| token.to_owned())
}

fn is_token(token: &str) -> bool {
    let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-';
    (1..=32).contains(&token.len()) && token.bytes().all(allowed)
}

pub fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// One changed saved variables file, with the name of its account folder.
pub struct SavedFile {
    pub account: String,
    pub text: String,
}

/// Watches the saved variables file of one app in every account.
pub struct Watcher {
    accounts: PathBuf,
    file: String,
    seen: HashMap<PathBuf, SystemTime>,
}

impl Watcher {
    pub fn new(accounts: &Path, app: App) -> Watcher {
        Watcher {
            accounts: accounts.to_owned(),
            file: saved_variables_file(app),
            seen: HashMap::new(),
        }
    }

    /// Each file that is new or changed since the last call. The first call reads every
    /// file; old frames in them fail the time check.
    pub fn changed(&mut self) -> Vec<SavedFile> {
        let Ok(accounts) = fs::read_dir(&self.accounts) else {
            return Vec::new();
        };
        let mut texts = Vec::new();
        for account in accounts.flatten() {
            let path = account.path().join("SavedVariables").join(&self.file);
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
            if let Ok(Some(bytes)) = read_at_most(&path, MAX_FILE) {
                texts.push(SavedFile {
                    account: account.file_name().to_string_lossy().into_owned(),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                });
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
    fn a_hex_field_is_found_by_its_name_only() {
        let text = "DB = {\n\t[\"results\"] = \"7b7d\",\n\t[\"load\"] = \"5b5d\",\n}\n";
        assert_eq!(hex_fields(text, "results"), [b"{}".to_vec()]);
        assert_eq!(hex_fields(text, "load"), [b"[]".to_vec()]);
        assert!(hex_fields(text, "combat").is_empty());
    }

    #[test]
    fn the_newest_file_of_all_accounts_is_read_even_with_bytes_that_are_not_utf8() {
        let root = tempfile::tempdir().unwrap();
        let old = account_file(root.path());
        fs::write(&old, "old").unwrap();
        let dir = root.path().join("ACCOUNT2").join("SavedVariables");
        fs::create_dir_all(&dir).unwrap();
        let new = dir.join("GnomishRelay.lua");
        fs::write(&new, b"[\"results\"] = \"6f6b\", [\"x\"] = \"\xff\"").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&new)
            .unwrap()
            .set_modified(later)
            .unwrap();

        let (path, text) = newest(root.path(), "GnomishRelay.lua").unwrap();

        assert_eq!(path, new);
        assert_eq!(hex_fields(&text, "results"), [b"ok".to_vec()]);
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
        dir.join("GnomishRelay.lua")
    }

    #[test]
    fn a_file_is_read_again_only_after_it_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = account_file(root.path());
        fs::write(&file, "one").unwrap();
        let mut watcher = Watcher::new(root.path(), App::Relay);
        assert_eq!(texts(watcher.changed()), ["one"]);
        assert!(watcher.changed().is_empty());
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::write(&file, "two").unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(texts(watcher.changed()), ["two"]);
    }

    fn texts(files: Vec<SavedFile>) -> Vec<String> {
        files.into_iter().map(|f| f.text).collect()
    }

    #[test]
    fn a_changed_file_names_its_account_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::write(account_file(root.path()), "one").unwrap();

        let files = Watcher::new(root.path(), App::Relay).changed();

        assert_eq!(files[0].account, "ACCOUNT1");
    }

    #[test]
    fn the_token_is_the_one_at_the_top_of_the_table() {
        let text = "GnomishRelayDB = {\n\t[\"chats\"] = {\n\t\t{\n\t\t\t[\"token\"] = \"inner\",\n\t\t},\n\t},\n\t[\"token\"] = \"k3y_-9\",\n}\n";

        assert_eq!(saved_token(text).as_deref(), Some("k3y_-9"));
    }

    #[test]
    fn a_file_with_no_token_or_a_bad_one_has_none() {
        assert_eq!(saved_token("GnomishRelayDB = {\n}\n"), None);
        assert_eq!(saved_token("\n\t[\"token\"] = \"Bad Token\",\n"), None);
        assert_eq!(saved_token("\n\t[\"token\"] = \"\",\n"), None);
        let long = format!("\n\t[\"token\"] = \"{}\",\n", "a".repeat(33));
        assert_eq!(saved_token(&long), None);
    }

    /// A Lua string can hold any byte, so WoW can write one that is not UTF-8.
    #[test]
    fn a_file_with_a_byte_that_is_not_utf8_still_gives_its_frames() {
        let root = tempfile::tempdir().unwrap();
        let file = account_file(root.path());
        fs::write(&file, b"[\"name\"] = \"\xff\", [\"frame\"] = \"6e52\"").unwrap();

        let texts = texts(Watcher::new(root.path(), App::Relay).changed());

        assert_eq!(texts.len(), 1);
        assert_eq!(frames(&texts[0]), [vec![0x6e, 0x52]]);
    }

    #[test]
    fn each_app_watches_only_its_own_saved_variables_file() {
        let root = tempfile::tempdir().unwrap();
        let relay = account_file(root.path());
        fs::write(&relay, "relay").unwrap();
        fs::write(relay.with_file_name("Timeways.lua"), "story").unwrap();
        assert_eq!(
            texts(Watcher::new(root.path(), App::Relay).changed()),
            ["relay"]
        );
        assert_eq!(
            texts(Watcher::new(root.path(), App::Timeways).changed()),
            ["story"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_variables_file_that_is_a_link_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let file = account_file(root.path());
        let target = root.path().join("elsewhere.lua");
        fs::write(&target, "[\"frame\"] = \"00\"").unwrap();
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert!(Watcher::new(root.path(), App::Relay).changed().is_empty());
    }
}
