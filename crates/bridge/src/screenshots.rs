//! Finds new screenshots and reads the strip in them (SPEC.md 8.2).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs_safe::open_regular;
use crate::strip::{self, Image};

pub struct Watcher {
    dir: PathBuf,
    sizes: HashMap<PathBuf, u64>,
    done: HashSet<PathBuf>,
}

impl Watcher {
    pub fn new(dir: &Path) -> Watcher {
        Watcher {
            dir: dir.to_owned(),
            sizes: HashMap::new(),
            done: HashSet::new(),
        }
    }

    /// New PNG files with the same size as at the last scan, so WoW has finished them.
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
            if !is_png || !kind.is_file() || self.done.contains(&path) {
                continue;
            }
            let Ok(size) = entry.metadata().map(|m| m.len()) else {
                continue;
            };
            if self.sizes.insert(path.clone(), size) == Some(size) {
                self.sizes.remove(&path);
                self.done.insert(path.clone());
                ready.push(path);
            }
        }
        // Forget deleted files, so the sets stay as small as the folder.
        self.done.retain(|p| present.contains(p));
        self.sizes.retain(|p, _| present.contains(p));
        ready
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
        assert!(watcher.ready().is_empty());
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
