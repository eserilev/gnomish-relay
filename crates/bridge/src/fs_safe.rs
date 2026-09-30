//! File writes that never follow a symbolic link (SPEC.md 6.2, rule 7), and reads with a
//! size limit (rule 9).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The bytes of a file of at most `max` bytes, or `None` for a bigger one. The read stops
/// at the limit: a size check before the read misses a file that grows.
pub fn read_at_most(path: &Path, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Ok(None);
    }
    Ok(Some(bytes))
}

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

/// Who can read a new file. Windows has no mode: its files inherit the folder rights.
#[derive(Clone, Copy)]
enum Readers {
    /// The umask decides.
    Anyone,
    /// Mode 0600 from the start.
    Owner,
}

/// Replaces `dir/name` in one step. A reader sees the old file or the new one, never half.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    write(dir, name, bytes, Durability::Synced, Readers::Anyone)
}

/// Mode 0600: a key signs strips, and the config sets the ceiling of every game message.
pub fn write_private(dir: &Path, name: &str, text: &str) -> Result<()> {
    write(
        dir,
        name,
        text.as_bytes(),
        Durability::Synced,
        Readers::Owner,
    )
}

/// `write_atomic` with no sync. A sync costs milliseconds on Windows, so this is for
/// many files that a second run of the same command writes again.
pub fn write_atomic_unsynced(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    write(dir, name, bytes, Durability::Cached, Readers::Anyone)
}

/// Mode 0700, also for a folder that an older bridge made with the umask. The data and
/// config folders hold keys, chats, and the replay store.
pub fn make_private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("cannot make {}", path.display()))?;
    check_real_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// How a log file opens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogStart {
    Append,
    /// A log that grew too big starts again.
    Fresh,
}

/// Mode 0600, and never through a link: the log names each chat and each prompt.
pub fn open_private_log(path: &Path, start: LogStart) -> Result<File> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!(
            "{} is a symbolic link. The bridge does not write through links.",
            path.display()
        );
    }
    let mut options = OpenOptions::new();
    options
        .create(true)
        .write(true)
        .append(start == LogStart::Append)
        .truncate(start == LogStart::Fresh);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    // The mode of `open` counts only for a new file. This one changes the open file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
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

fn write(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    durability: Durability,
    readers: Readers,
) -> Result<()> {
    check_real_dir(dir)?;
    let (mut file, tmp) = create_temp(dir, name, readers)?;
    file.write_all(bytes)?;
    if let Durability::Synced = durability {
        file.sync_all()?;
    }
    drop(file);
    let path = dir.join(name);
    fs::rename(&tmp, &path).with_context(|| format!("cannot replace {}", path.display()))
}

fn create_temp(dir: &Path, name: &str, readers: Readers) -> Result<(File, PathBuf)> {
    let tmp = dir.join(format!(".{name}.tmp"));
    // A crash can leave the temp file behind. Removing a link removes only the link.
    let _ = fs::remove_file(&tmp);
    let mut options = OpenOptions::new();
    // `create_new` fails if anything, a link too, already has the name.
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Readers::Owner = readers {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = readers;
    let file = options
        .open(&tmp)
        .with_context(|| format!("cannot create {}", tmp.display()))?;
    Ok((file, tmp))
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

    #[test]
    fn a_limited_read_takes_a_file_of_the_limit_and_refuses_one_byte_more() {
        let dir = tempfile::tempdir().unwrap();
        let fits = dir.path().join("fits");
        let over = dir.path().join("over");
        fs::write(&fits, b"1234").unwrap();
        fs::write(&over, b"12345").unwrap();

        assert_eq!(read_at_most(&fits, 4).unwrap(), Some(b"1234".to_vec()));
        assert_eq!(read_at_most(&over, 4).unwrap(), None);
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// A chmod after the rename leaves a moment when others read the file, and it
    /// follows a link that takes the place of the file.
    #[cfg(unix)]
    #[test]
    fn a_private_temp_file_has_mode_0600_from_the_start() {
        let dir = tempfile::tempdir().unwrap();

        let (_file, tmp) = create_temp(dir.path(), "a.key", Readers::Owner).unwrap();

        assert_eq!(mode(&tmp), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_write_replaces_an_old_file_that_others_could_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.key");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        write_private(dir.path(), "a.key", "new").unwrap();

        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn a_private_folder_has_mode_0700_also_when_it_was_open_before() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join("a/b");
        let old = root.path().join("old");
        fs::create_dir(&old).unwrap();
        fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();

        make_private_dir(&new).unwrap();
        make_private_dir(&old).unwrap();

        assert_eq!(mode(&new), 0o700);
        assert_eq!(mode(&old), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_log_has_mode_0600_and_keeps_its_lines() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bridge.log");
        fs::write(&path, "old\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let mut log = open_private_log(&path, LogStart::Append).unwrap();
        log.write_all(b"new\n").unwrap();

        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\nnew\n");
    }

    #[test]
    fn a_fresh_log_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bridge.log");
        fs::write(&path, "old\n").unwrap();

        open_private_log(&path, LogStart::Fresh).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }

    #[cfg(unix)]
    #[test]
    fn a_log_that_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, "other file").unwrap();
        let link = dir.path().join("bridge.log");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(open_private_log(&link, LogStart::Append).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "other file");
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
