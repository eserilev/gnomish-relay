//! Finds new screenshots and reads the strip in them (SPEC.md 8.2).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::fs_safe::read_at_most;
use crate::strip::{self, Image, MAX_SIDE};

/// The PNG of the largest image that the bridge decodes, with 16-bit RGBA and no
/// compression, fits. Room for the chunks and the deflate blocks comes on top.
const MAX_PNG: u64 = MAX_SIDE as u64 * MAX_SIDE as u64 * 8 + 1024 * 1024;

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

/// The strip bytes, or `None` for a normal screenshot of the user. `accept` is the tag
/// check of `strip::read_with`.
pub fn read_strip(path: &Path, accept: impl Fn(&[u8]) -> bool) -> Result<Option<Vec<u8>>> {
    let Some(bytes) = read_at_most(path, MAX_PNG)? else {
        bail!("{} is bigger than {MAX_PNG} bytes", path.display());
    };
    let image = Image::from_png(&bytes)?;
    Ok(strip::read_with(&image, accept))
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

    /// A file can grow after the size check of the watcher, so the read has its own limit.
    #[test]
    fn a_screenshot_bigger_than_any_png_of_the_largest_image_is_refused_unread() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("WoWScrnShot_3.png");
        let file = fs::File::create(&shot).unwrap();
        file.set_len(MAX_PNG + 1).unwrap();

        let error = read_strip(&shot, |_| true).unwrap_err();

        assert!(error.to_string().contains("bigger than"), "{error}");
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
