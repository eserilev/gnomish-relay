//! The JSON-lines log file in the data folder, and its rotation (SPEC.md 8.5).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs_safe::{LogStart, make_private_dir, open_private_log};

pub const LOG_DIR: &str = "logs";
pub const JSON_LOG: &str = "bridge.jsonl";
pub const MAX_FILE_BYTES: u64 = 5_000_000;
/// The file in use and four old ones.
pub const FILES: usize = 5;

pub struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    files: usize,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    /// Opens nothing yet: the first line makes the folder and the file.
    pub fn new(dir: &Path, max_bytes: u64, files: usize) -> RotatingFile {
        RotatingFile {
            path: dir.join(JSON_LOG),
            max_bytes,
            files,
            file: None,
            size: 0,
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        let len = line.len() as u64 + 1;
        self.open()?;
        if self.size > 0 && self.size + len > self.max_bytes {
            self.rotate()?;
            self.open()?;
        }
        if let Some(file) = &mut self.file {
            writeln!(file, "{line}")?;
        }
        self.size += len;
        Ok(())
    }

    fn open(&mut self) -> Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            make_private_dir(dir)?;
        }
        let file = open_private_log(&self.path, LogStart::Append)?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    /// `bridge.jsonl` becomes `bridge.jsonl.1`, `.1` becomes `.2`, and the oldest goes.
    fn rotate(&mut self) -> Result<()> {
        self.file = None;
        let oldest = self.files.saturating_sub(1);
        remove_if_there(&numbered(&self.path, oldest))?;
        for n in (1..oldest).rev() {
            rename_if_there(&numbered(&self.path, n), &numbered(&self.path, n + 1))?;
        }
        rename_if_there(&self.path, &numbered(&self.path, 1))?;
        self.size = 0;
        Ok(())
    }
}

/// The log files of `dir`, oldest first.
pub fn files_oldest_first(dir: &Path) -> Vec<PathBuf> {
    let path = dir.join(JSON_LOG);
    let mut files: Vec<PathBuf> = (1..FILES)
        .rev()
        .map(|n| numbered(&path, n))
        .filter(|p| p.is_file())
        .collect();
    if path.is_file() {
        files.push(path);
    }
    files
}

fn numbered(path: &Path, n: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

fn remove_if_there(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Windows refuses a rename onto a file, so the target goes first.
fn rename_if_there(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    remove_if_there(to)?;
    std::fs::rename(from, to)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_json_file_rotates_at_its_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join(LOG_DIR);
        let mut file = RotatingFile::new(&logs, 20, 3);

        for line in ["line 1 abcdefgh", "line 2 abcdefgh", "line 3 abcdefgh"] {
            file.write_line(line).unwrap();
        }

        assert_eq!(read(&logs.join(JSON_LOG)), "line 3 abcdefgh\n");
        assert_eq!(read(&logs.join("bridge.jsonl.1")), "line 2 abcdefgh\n");
        assert_eq!(read(&logs.join("bridge.jsonl.2")), "line 1 abcdefgh\n");
    }

    #[test]
    fn the_oldest_file_goes_when_all_files_are_full() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RotatingFile::new(dir.path(), 10, 2);

        for line in ["first one", "second on", "third one"] {
            file.write_line(line).unwrap();
        }

        assert_eq!(read(&dir.path().join(JSON_LOG)), "third one\n");
        assert_eq!(read(&dir.path().join("bridge.jsonl.1")), "second on\n");
        assert!(!dir.path().join("bridge.jsonl.2").exists());
    }

    #[test]
    fn a_new_file_continues_the_size_of_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(JSON_LOG), "0123456789\n").unwrap();
        let mut file = RotatingFile::new(dir.path(), 15, 5);

        file.write_line("abcdef").unwrap();

        assert_eq!(read(&dir.path().join(JSON_LOG)), "abcdef\n");
        assert_eq!(read(&dir.path().join("bridge.jsonl.1")), "0123456789\n");
    }

    #[cfg(unix)]
    #[test]
    fn the_json_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join(LOG_DIR);

        RotatingFile::new(&logs, 100, 5).write_line("x").unwrap();

        let mode = std::fs::metadata(logs.join(JSON_LOG))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn the_files_come_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["bridge.jsonl", "bridge.jsonl.1", "bridge.jsonl.3"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }

        let names: Vec<String> = files_oldest_first(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(names, ["bridge.jsonl.3", "bridge.jsonl.1", "bridge.jsonl"]);
    }
}
