//! The spool folder: one small JSON file for each hook event of a terminal session. The
//! hook writes it, and the bridge takes it (SPEC.md 10.2).

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::fs_safe::{make_private_dir, read_at_most};

pub const SPOOL_DIR: &str = "notices";
pub const MAX_FILE: u64 = 4096;
/// With this many files the bridge stopped reading, so the hook writes no more.
pub const FULL: usize = 100;
pub const READ_AT_ONCE: usize = 64;
/// A `.tmp` file this old belongs to a hook that died before its rename.
pub const TMP_AGE: Duration = Duration::from_mins(1);
const MAX_SESSION: usize = 128;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum SpoolEvent {
    SessionStart,
    TurnStart,
    Waiting,
    Finished,
    Failed,
    SessionEnd,
}

/// One spool file. It has no time: the bridge takes the time of its read.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct SpoolFile {
    pub v: u32,
    pub source: Source,
    pub event: SpoolEvent,
    pub session: String,
    pub repo: String,
    pub text: String,
}

/// 1 to 128 bytes of `[A-Za-z0-9_-]`, as the agents make their session ids.
pub fn is_session_id(id: &str) -> bool {
    (1..=MAX_SESSION).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl SpoolFile {
    /// The fields are exact, and a key twice is an error.
    pub fn parse(bytes: &[u8]) -> Result<SpoolFile, String> {
        let file: SpoolFile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file.v != 1 {
            return Err(format!("version {} is not 1", file.v));
        }
        if !is_session_id(&file.session) {
            return Err("the session id has a bad shape".into());
        }
        Ok(file)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

pub fn spool_dir(data: &Path) -> PathBuf {
    data.join(SPOOL_DIR)
}

/// The time first, so the names sort oldest first.
pub fn unique_name(nanos: u128, pid: u32, count: u32) -> String {
    format!("{nanos:039}-{pid}-{count}")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Written {
    Yes,
    /// No bridge made the folder, so no bridge reads it.
    NoFolder,
    Full,
}

/// Writes `<name>.tmp` and renames it, so the bridge never reads half a file.
pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<Written> {
    let is_real_dir = fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir());
    if !is_real_dir {
        return Ok(Written::NoFolder);
    }
    if fs::read_dir(dir)?.take(FULL).count() >= FULL {
        return Ok(Written::Full);
    }
    let tmp = dir.join(format!("{name}.tmp"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&tmp)?.write_all(bytes)?;
    fs::rename(&tmp, dir.join(format!("{name}.json")))?;
    Ok(Written::Yes)
}

/// Makes the folder, mode 0700, and empties it. A file from before this start has lost
/// its time, and a notice from it lies about when the agent stopped.
pub fn open_spool(dir: &Path) -> anyhow::Result<()> {
    make_private_dir(dir)?;
    for entry in fs::read_dir(dir)?.flatten() {
        let _ = remove_entry(&entry.path());
    }
    Ok(())
}

/// Any local process can make a folder here, and `remove_file` leaves a folder. The
/// removal of a folder does not follow a link inside it.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        return fs::remove_dir_all(path);
    }
    fs::remove_file(path)
}

/// One read of the folder: the files that passed the checks, oldest first, and a log line
/// for each file that did not.
#[derive(Debug, Default)]
pub struct Taken {
    pub files: Vec<SpoolFile>,
    pub refused: Vec<String>,
}

enum Entry {
    Json(PathBuf),
    OldTmp(PathBuf),
    Other,
}

fn classify(path: PathBuf, now: SystemTime) -> Entry {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if extension == "json" {
        return Entry::Json(path);
    }
    if extension != "tmp" {
        return Entry::Other;
    }
    let modified = fs::symlink_metadata(&path).and_then(|m| m.modified());
    let age = modified.map(|m| now.duration_since(m).unwrap_or_default());
    match age {
        Ok(age) if age > TMP_AGE => Entry::OldTmp(path),
        _ => Entry::Other,
    }
}

/// Takes at most `READ_AT_ONCE` files, oldest first. Each file goes before its parse, so a
/// bad file never comes back.
pub fn take_files(dir: &Path, now: SystemTime) -> Taken {
    let mut taken = Taken::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return taken;
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        match classify(entry.path(), now) {
            Entry::Json(path) => names.push(path),
            Entry::OldTmp(path) => {
                let _ = fs::remove_file(path);
            }
            Entry::Other => {}
        }
    }
    names.sort();
    names.truncate(READ_AT_ONCE);
    for path in names {
        match take_one(&path) {
            Ok(file) => taken.files.push(file),
            Err(why) => taken
                .refused
                .push(format!("{}: {why}", display_name(&path))),
        }
    }
    taken
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn take_one(path: &Path) -> Result<SpoolFile, String> {
    let bytes = read_real_file(path);
    remove_entry(path).map_err(|e| format!("cannot delete it: {e}"))?;
    SpoolFile::parse(&bytes?)
}

/// Never through a link: a link can point at any file of the user.
fn read_real_file(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    match read_at_most(path, MAX_FILE) {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(format!("bigger than {MAX_FILE} bytes")),
        Err(e) if e.kind() == ErrorKind::NotFound => Err("gone before its read".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(event: SpoolEvent) -> SpoolFile {
        SpoolFile {
            v: 1,
            source: Source::Claude,
            event,
            session: "abc-123_x".into(),
            repo: "gnomish-relay".into(),
            text: "Done.".into(),
        }
    }

    fn spool() -> (tempfile::TempDir, PathBuf) {
        let data = tempfile::tempdir().unwrap();
        let dir = spool_dir(data.path());
        open_spool(&dir).unwrap();
        (data, dir)
    }

    #[test]
    fn a_spool_file_is_one_json_object_with_the_names_of_the_spec() {
        let text = String::from_utf8(file(SpoolEvent::SessionStart).to_bytes()).unwrap();
        assert_eq!(
            text,
            r#"{"v":1,"source":"claude","event":"session-start","session":"abc-123_x","repo":"gnomish-relay","text":"Done."}"#
        );
    }

    #[test]
    fn a_spool_file_parses_back() {
        let original = file(SpoolEvent::TurnStart);
        assert_eq!(SpoolFile::parse(&original.to_bytes()), Ok(original));
    }

    #[test]
    fn an_unknown_field_a_key_twice_or_a_bad_value_is_refused() {
        let base = r#""source":"codex","event":"finished","session":"s1","repo":"r","text":"t""#;
        assert!(SpoolFile::parse(format!("{{\"v\":1,{base}}}").as_bytes()).is_ok());
        let refused = [
            format!("{{\"v\":1,{base},\"at\":5}}"),
            format!("{{\"v\":1,\"v\":1,{base}}}"),
            format!("{{\"v\":2,{base}}}"),
            base.replace("codex", "gemini"),
            base.replace("finished", "done"),
            format!("{{\"v\":1,{}}}", base.replace("\"s1\"", "\"s 1\"")),
            format!("{{\"v\":1,{}}}", base.replace("\"s1\"", "\"\"")),
            format!("{{\"v\":1,{}}}", base.replace("\"r\"", "7")),
            "[1]".into(),
        ];
        for text in refused {
            assert!(SpoolFile::parse(text.as_bytes()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_session_id_has_at_most_128_bytes_of_letters_digits_dash_and_underscore() {
        assert!(is_session_id(&"a".repeat(128)));
        assert!(!is_session_id(&"a".repeat(129)));
        assert!(!is_session_id("../x"));
        assert!(!is_session_id(""));
    }

    #[test]
    fn names_sort_by_time() {
        let early = unique_name(9, 50_000, 0);
        let late = unique_name(10, 1, 0);
        assert!(early < late);
    }

    #[test]
    fn a_written_file_is_taken_once_and_deleted() {
        let (_data, dir) = spool();
        let bytes = file(SpoolEvent::Waiting).to_bytes();
        assert_eq!(write_file(&dir, "a", &bytes).unwrap(), Written::Yes);

        let taken = take_files(&dir, SystemTime::now());

        assert_eq!(taken.files, [file(SpoolEvent::Waiting)]);
        assert!(taken.refused.is_empty());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(take_files(&dir, SystemTime::now()).files.is_empty());
    }

    #[test]
    fn with_no_folder_the_hook_writes_nothing() {
        let data = tempfile::tempdir().unwrap();
        let dir = spool_dir(data.path());
        assert_eq!(write_file(&dir, "a", b"{}").unwrap(), Written::NoFolder);
        assert!(!dir.exists());
    }

    #[test]
    fn a_full_folder_takes_no_more_files() {
        let (_data, dir) = spool();
        for n in 0..FULL {
            fs::write(dir.join(format!("{n}.json")), b"{}").unwrap();
        }
        assert_eq!(write_file(&dir, "last", b"{}").unwrap(), Written::Full);
        assert!(!dir.join("last.json").exists());
    }

    #[test]
    fn a_read_takes_at_most_64_files_oldest_first() {
        let (_data, dir) = spool();
        for n in 0..65u32 {
            let mut one = file(SpoolEvent::Finished);
            one.text = n.to_string();
            fs::write(
                dir.join(format!("{}.json", unique_name(n.into(), 1, 0))),
                one.to_bytes(),
            )
            .unwrap();
        }

        let first = take_files(&dir, SystemTime::now());
        let second = take_files(&dir, SystemTime::now());

        assert_eq!(first.files.len(), READ_AT_ONCE);
        assert_eq!(first.files[0].text, "0");
        assert_eq!(second.files.len(), 1);
        assert_eq!(second.files[0].text, "64");
    }

    #[test]
    fn a_bad_file_is_deleted_and_named_once() {
        let (_data, dir) = spool();
        fs::write(dir.join("bad.json"), b"{\"v\":1").unwrap();

        let taken = take_files(&dir, SystemTime::now());

        assert!(taken.files.is_empty());
        assert_eq!(taken.refused.len(), 1);
        assert!(
            taken.refused[0].starts_with("bad.json: "),
            "{:?}",
            taken.refused
        );
        assert!(take_files(&dir, SystemTime::now()).refused.is_empty());
    }

    #[test]
    fn a_file_of_4_kib_and_1_byte_is_refused() {
        let (_data, dir) = spool();
        let mut big = file(SpoolEvent::Finished);
        let fixed = big.to_bytes().len() - big.text.len();
        big.text = "a".repeat(usize::try_from(MAX_FILE).unwrap() - fixed);
        fs::write(dir.join("fits.json"), big.to_bytes()).unwrap();
        big.text.push('a');
        fs::write(dir.join("over.json"), big.to_bytes()).unwrap();

        let taken = take_files(&dir, SystemTime::now());

        assert_eq!(taken.files.len(), 1);
        assert!(taken.refused[0].contains("bigger than 4096 bytes"));
    }

    #[test]
    fn a_tmp_file_waits_and_an_old_one_goes() {
        let (_data, dir) = spool();
        fs::write(dir.join("half.tmp"), b"{\"v\"").unwrap();

        let soon = take_files(&dir, SystemTime::now());
        assert!(soon.files.is_empty() && soon.refused.is_empty());
        assert!(dir.join("half.tmp").exists());

        let later = SystemTime::now() + TMP_AGE + Duration::from_secs(1);
        take_files(&dir, later);
        assert!(!dir.join("half.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed() {
        let (data, dir) = spool();
        let secret = data.path().join("secret.json");
        fs::write(&secret, file(SpoolEvent::Waiting).to_bytes()).unwrap();
        std::os::unix::fs::symlink(&secret, dir.join("link.json")).unwrap();

        let taken = take_files(&dir, SystemTime::now());

        assert!(taken.files.is_empty());
        assert!(taken.refused[0].contains("not a plain file"));
        assert!(secret.exists(), "only the link goes");
    }

    #[test]
    fn a_folder_named_json_is_deleted_and_named_once() {
        let (_data, dir) = spool();
        fs::create_dir_all(dir.join("0.json/inner")).unwrap();

        let first = take_files(&dir, SystemTime::now());
        let second = take_files(&dir, SystemTime::now());

        assert!(first.refused[0].contains("not a plain file"));
        assert!(!dir.join("0.json").exists());
        assert!(second.refused.is_empty());
    }

    #[test]
    fn a_start_empties_the_folder() {
        let (_data, dir) = spool();
        fs::write(dir.join("old.json"), b"{}").unwrap();
        fs::create_dir_all(dir.join("0.json/inner")).unwrap();
        open_spool(&dir).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_has_mode_0700() {
        use std::os::unix::fs::PermissionsExt;
        let (_data, dir) = spool();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
