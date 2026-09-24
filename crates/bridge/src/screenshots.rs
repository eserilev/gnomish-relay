//! Finds new screenshots and reads the strip in them (SPEC.md 8.2).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

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

/// The strip bytes, or `None` for a normal screenshot of the user.
pub fn read_strip(path: &Path) -> Result<Option<Vec<u8>>> {
    let image = Image::from_png(&fs::read(path)?)?;
    Ok(strip::read(&image))
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

    #[test]
    fn other_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path());
        fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        watcher.ready();
        assert!(watcher.ready().is_empty());
    }
}
