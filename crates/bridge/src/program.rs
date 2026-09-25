//! Finds a program on `PATH` the way a shell does. Windows runs npm tools through a
//! `.cmd` file, and `Command::new` tries only `.exe` (SPEC.md 11.2).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const WINDOWS_EXTENSIONS: [&str; 3] = ["exe", "cmd", "bat"];

/// A name with a folder in it is used as it is.
pub fn find_program(name: &str, path: &OsStr, windows: bool) -> Option<PathBuf> {
    let given = Path::new(name);
    if given.components().count() > 1 {
        return given.is_file().then(|| given.to_owned());
    }
    let has_extension = given.extension().is_some();
    std::env::split_paths(path).find_map(|dir| {
        if !windows || has_extension {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !windows {
            return None;
        }
        WINDOWS_EXTENSIONS
            .iter()
            .map(|ext| dir.join(format!("{name}.{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn windows_finds_a_cmd_file_and_unix_the_plain_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("claude-agent-acp.cmd"), "").unwrap();
        fs::write(dir.path().join("codex-acp"), "").unwrap();
        let path = dir.path().as_os_str();
        assert_eq!(
            find_program("claude-agent-acp", path, true),
            Some(dir.path().join("claude-agent-acp.cmd"))
        );
        assert_eq!(find_program("claude-agent-acp", path, false), None);
        assert_eq!(
            find_program("codex-acp", path, false),
            Some(dir.path().join("codex-acp"))
        );
    }

    #[test]
    fn the_first_folder_on_the_path_wins() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(a.path().join("gemini.exe"), "").unwrap();
        fs::write(b.path().join("gemini.exe"), "").unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(
            find_program("gemini", &path, true),
            Some(a.path().join("gemini.exe"))
        );
    }
}
