//! Finds new screenshots and reads the strip in them (SPEC.md 8.2).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs_safe::open_regular;
use crate::strip::{self, Image};

/// A file that fails to read this often counts as done, so a broken file costs no read
/// in each step. A change of its size makes it new again.
const MAX_READ_TRIES: u32 = 3;

/// How the read of a ready file went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    Worked,
    Failed,
}

pub struct Watcher {
    dir: PathBuf,
    sizes: HashMap<PathBuf, u64>,
    /// The size of each file at its last read that counts.
    done: HashMap<PathBuf, u64>,
    failures: HashMap<PathBuf, u32>,
}

impl Watcher {
    pub fn new(dir: &Path) -> Watcher {
        Watcher {
            dir: dir.to_owned(),
            sizes: HashMap::new(),
            done: HashMap::new(),
            failures: HashMap::new(),
        }
    }

    /// PNG files with the same size as at the last scan, so WoW has finished them. A file
    /// stays ready until `after_read` marks it done.
    pub fn ready(&mut self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut ready = Vec::new();
        let mut present = HashSet::new();
        for entry in entries.flatten() {
            let path = entry.path();
            present.insert(path.clone());
            // `file_type` does not follow links, so a link to another file is skipped.
            let is_png = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"));
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let Ok(size) = entry.metadata().map(|m| m.len()) else {
                continue;
            };
            if !is_png || !kind.is_file() || self.done.get(&path) == Some(&size) {
                continue;
            }
            if self.sizes.insert(path.clone(), size) == Some(size) {
                ready.push(path);
            }
        }
        // Forget deleted files, so the maps stay as small as the folder.
        self.done.retain(|p, _| present.contains(p));
        self.sizes.retain(|p, _| present.contains(p));
        self.failures.retain(|p, _| present.contains(p));
        ready
    }

    /// A file is done after a read that worked, or after `MAX_READ_TRIES` failed reads.
    pub fn after_read(&mut self, path: &Path, read: Read) {
        if read == Read::Failed && self.count_failure(path) < MAX_READ_TRIES {
            return;
        }
        self.failures.remove(path);
        if let Some(size) = self.sizes.get(path) {
            self.done.insert(path.to_owned(), *size);
        }
    }

    fn count_failure(&mut self, path: &Path) -> u32 {
        let failures = self.failures.entry(path.to_owned()).or_insert(0);
        *failures += 1;
        *failures
    }
}

/// The bytes of a strip, and the screenshot for the line test (SPEC.md 7.1.4).
pub struct Shot {
    pub bytes: Vec<u8>,
    pub image: Image,
}

/// `None` for a normal screenshot of the user. `accept` is the tag check of
/// `strip::read_with`.
pub fn read_strip(path: &Path, accept: impl Fn(&[u8]) -> bool) -> Result<Option<Shot>> {
    let file = open_regular(path)?;
    let Some(image) = Image::from_reader_if_marked(file)? else {
        return Ok(None);
    };
    let Some(bytes) = strip::read_with(&image, accept) else {
        return Ok(None);
    };
    Ok(Some(Shot { bytes, image }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_ready_once_its_size_stops_changing() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path());
        let shot = dir.path().join("WoWScrnShot_1.png");
        fs::write(&shot, b"half").unwrap();
        assert!(watcher.ready().is_empty());
        fs::write(&shot, b"half and more").unwrap();
        assert!(watcher.ready().is_empty());
        assert_eq!(watcher.ready(), [shot]);
    }

    #[test]
    fn a_file_stays_ready_until_its_read_worked() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path());
        let shot = dir.path().join("WoWScrnShot_1.png");
        fs::write(&shot, b"whole").unwrap();
        watcher.ready();
        assert_eq!(watcher.ready(), std::slice::from_ref(&shot));
        assert_eq!(watcher.ready(), std::slice::from_ref(&shot));

        watcher.after_read(&shot, Read::Worked);

        assert!(watcher.ready().is_empty());
    }

    #[test]
    fn a_file_that_fails_to_read_three_times_is_done_until_its_size_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path());
        let shot = dir.path().join("WoWScrnShot_1.png");
        fs::write(&shot, b"broken").unwrap();
        watcher.ready();
        for _ in 0..2 {
            assert_eq!(watcher.ready(), std::slice::from_ref(&shot));
            watcher.after_read(&shot, Read::Failed);
        }
        assert_eq!(watcher.ready(), std::slice::from_ref(&shot));
        watcher.after_read(&shot, Read::Failed);
        assert!(watcher.ready().is_empty());

        fs::write(&shot, b"whole at last").unwrap();
        watcher.ready();

        assert_eq!(watcher.ready(), [shot]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_another_file_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secret.txt");
        fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("WoWScrnShot_2.png")).unwrap();
        let mut watcher = Watcher::new(dir.path());
        watcher.ready();
        assert!(watcher.ready().is_empty());
    }

    /// The read stops at the header of a file that is no PNG, so its size costs nothing.
    #[test]
    fn a_huge_file_that_is_no_png_fails_at_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("WoWScrnShot_3.png");
        let file = fs::File::create(&shot).unwrap();
        file.set_len(4 << 30).unwrap();

        assert!(read_strip(&shot, |_| true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_in_the_screenshot_folder_does_not_hang_the_read() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("WoWScrnShot_4.png");
        crate::fs_safe::tests::make_fifo(&shot);

        let read =
            crate::fs_safe::tests::returns_at_once(move || read_strip(&shot, |_| true).is_err());

        assert_eq!(read, Some(true));
    }

    /// The watcher skips a link, but a link can take the place of a file after the scan.
    #[cfg(unix)]
    #[test]
    fn a_screenshot_that_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.png");
        fs::write(
            &target,
            crate::strip::Image::from_rgb(1, 1, vec![0; 3])
                .unwrap()
                .to_png()
                .unwrap(),
        )
        .unwrap();
        let shot = dir.path().join("WoWScrnShot_5.png");
        std::os::unix::fs::symlink(&target, &shot).unwrap();

        assert!(read_strip(&shot, |_| true).is_err());
    }

    /// The text chunk claims 2 GiB, and the sparse file holds them, so only the cap ends it.
    #[test]
    fn a_png_with_a_huge_text_chunk_stops_at_the_file_cap() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("WoWScrnShot_6.png");
        let png = Image::from_rgb(1, 1, vec![0; 3]).unwrap().to_png().unwrap();
        let signature_and_header = &png[..33];
        let text_chunk_start = [&0x7FFF_FFFF_u32.to_be_bytes()[..], b"tEXt"].concat();
        fs::write(&shot, [signature_and_header, &text_chunk_start].concat()).unwrap();
        fs::File::options()
            .write(true)
            .open(&shot)
            .unwrap()
            .set_len(4 << 30)
            .unwrap();

        let error = read_strip(&shot, |_| true).err().unwrap();

        assert!(
            format!("{error:#}").contains("over the size cap"),
            "{error:#}"
        );
    }

    #[test]
    fn other_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path());
        fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        watcher.ready();
        assert!(watcher.ready().is_empty());
    }
}
