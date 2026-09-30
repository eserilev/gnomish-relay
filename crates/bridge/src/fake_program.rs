//! The fake programs of the tests, such as `gh`, a git hook, or a Timeways program.

use std::io;
use std::path::Path;
use std::process::Command;

/// Writes an executable script through `sh`, so the test process never opens it for writing.
///
/// Tests run in threads. A thread that forks while another thread holds a script open
/// for writing gives the child that handle until its `exec`. A run of the script in
/// that time fails with ETXTBSY ("Text file busy").
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    let status = Command::new("sh")
        .args([
            "-c",
            "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"",
            "sh",
        ])
        .arg(path)
        .arg(text)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("cannot write {}", path.display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_fake_program_runs_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("hello");

        write(&program, "#!/bin/sh\necho \"hi $1\"\n").unwrap();

        let out = Command::new(&program).arg("there").output().unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "hi there\n");
    }

    #[test]
    fn a_fake_program_in_a_missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        let result = write(&dir.path().join("no/such/folder/x"), "#!/bin/sh\n");

        assert!(result.is_err());
    }
}
