//! File writes that never follow a symbolic link (SPEC.md 6.2, rule 7).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Fails for a symbolic link, so a local program cannot point a slot at another folder.
pub fn check_real_dir(path: &Path) -> Result<()> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("cannot read {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!(
            "{} is a symbolic link. The bridge does not write through links.",
            path.display()
        );
    }
    if !meta.is_dir() {
        bail!("{} is not a folder", path.display());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Durability {
    /// The bytes are on the disk before the rename, so a power cut keeps the old or the new file.
    Synced,
    /// The rename is still atomic for a reader, but a power cut can leave an empty file.
    Cached,
}

/// Replaces `dir/name` in one step. A reader sees the old file or the new one, never half.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    write(dir, name, bytes, Durability::Synced)
}

/// Mode 0600: a key signs strips, and the config sets the ceiling of every game message.
pub fn write_private(dir: &Path, name: &str, text: &str) -> Result<()> {
    write_atomic(dir, name, text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// `write_atomic` with no sync. A sync costs milliseconds on Windows, so this is for
/// many files that a second run of the same command writes again.
pub fn write_atomic_unsynced(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    write(dir, name, bytes, Durability::Cached)
}

/// `write_atomic` that skips a file that already holds `bytes`. A slot publish writes 90
/// files, most of them unchanged, and a sync costs milliseconds on Windows.
pub fn write_atomic_if_changed(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    check_real_dir(dir)?;
    if file_holds(&dir.join(name), bytes) {
        return Ok(());
    }
    write_atomic(dir, name, bytes)
}

/// A link never holds the bytes, so the write replaces it.
fn file_holds(path: &Path, bytes: &[u8]) -> bool {
    let is_file = fs::symlink_metadata(path).is_ok_and(|m| m.is_file());
    is_file && fs::read(path).is_ok_and(|old| old == bytes)
}

fn write(dir: &Path, name: &str, bytes: &[u8], durability: Durability) -> Result<()> {
    check_real_dir(dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    // A crash can leave the temp file behind. Removing a link removes only the link.
    let _ = fs::remove_file(&tmp);
    // `create_new` fails if anything, a link too, already has the name.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .with_context(|| format!("cannot create {}", tmp.display()))?;
    file.write_all(bytes)?;
    if let Durability::Synced = durability {
        file.sync_all()?;
    }
    drop(file);
    let path = dir.join(name);
    fs::rename(&tmp, &path).with_context(|| format!("cannot replace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "a.lua", b"old").unwrap();
        write_atomic(dir.path(), "a.lua", b"new").unwrap();
        assert_eq!(fs::read(dir.path().join("a.lua")).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_unsynced_write_also_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic_unsynced(dir.path(), "a.lua", b"old").unwrap();
        write_atomic_unsynced(dir.path(), "a.lua", b"new").unwrap();
        assert_eq!(fs::read(dir.path().join("a.lua")).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn modified(path: &Path) -> std::time::SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    /// A time far in the past, so any write gives the file a different time.
    fn make_old(path: &Path) -> std::time::SystemTime {
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(old).unwrap();
        old
    }

    #[test]
    fn a_write_of_the_same_bytes_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.lua");
        write_atomic(dir.path(), "a.lua", b"same").unwrap();
        let old = make_old(&path);

        write_atomic_if_changed(dir.path(), "a.lua", b"same").unwrap();

        assert_eq!(modified(&path), old);
    }

    #[test]
    fn a_write_of_other_bytes_replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.lua");
        write_atomic(dir.path(), "a.lua", b"old").unwrap();
        let old = make_old(&path);

        write_atomic_if_changed(dir.path(), "a.lua", b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_ne!(modified(&path), old);
    }

    #[test]
    fn a_write_if_changed_makes_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();

        write_atomic_if_changed(dir.path(), "a.lua", b"new").unwrap();

        assert_eq!(fs::read(dir.path().join("a.lua")).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_with_the_same_bytes_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.lua");
        fs::write(&target, b"same").unwrap();
        let link = dir.path().join("a.lua");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic_if_changed(dir.path(), "a.lua", b"same").unwrap();

        assert!(!fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&link).unwrap(), b"same");
    }

    #[cfg(unix)]
    #[test]
    fn a_write_if_changed_into_a_folder_that_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("a.lua"), b"same").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(write_atomic_if_changed(&link, "a.lua", b"same").is_err());
    }

    #[test]
    fn a_leftover_temp_file_does_not_block_the_write() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".a.lua.tmp"), b"crash").unwrap();
        write_atomic(dir.path(), "a.lua", b"new").unwrap();
        assert_eq!(fs::read(dir.path().join("a.lua")).unwrap(), b"new");
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_atomic(&dir.path().join("nope"), "a.lua", b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write_atomic(&link, "a.lua", b"x").is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    }
}
