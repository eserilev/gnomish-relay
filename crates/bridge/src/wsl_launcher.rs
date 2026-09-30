//! The Windows side of the desktop app in WSL2: the sign-in entry, and the process
//! that keeps the distro alive (SPEC.md 11.5).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::dirs::Dirs;
use crate::fs_safe::make_private_dir;
use crate::install::WSL_START_FILE;
use crate::lock;
use crate::service;

pub const RUN_COMMAND: &str = "wsl-run";
pub const AUTOSTART_COMMAND: &str = "wsl-autostart";
pub const BACKGROUND_FLAG: &str = "--background";
/// Long enough for `restart` to see the old desktop app stop.
const RESTART_DELAY: Duration = Duration::from_secs(3);
const MAX_DISTRO: usize = 64;

/// WSL names a distro with these characters only. Any other name never reaches the
/// `Run` entry or a command line.
pub fn good_distro(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    !name.is_empty() && name.len() <= MAX_DISTRO && name.chars().all(allowed)
}

fn check_distro(name: &str) -> Result<()> {
    if !good_distro(name) {
        bail!("{name:?} isn't a WSL distro name");
    }
    Ok(())
}

/// The value of the `Run` entry. It starts the Windows program, which starts itself
/// again with no console window.
pub fn run_entry(exe: &Path, distro: &str) -> String {
    format!(
        "\"{}\" {RUN_COMMAND} {distro} {BACKGROUND_FLAG}",
        exe.display()
    )
}

/// The arguments of `wsl.exe`. The shell of the distro finds the home folder; Windows
/// knows only the name of the distro.
pub fn wsl_args(distro: &str) -> Vec<String> {
    let start = format!(". \"$HOME/{WSL_START_FILE}\"");
    ["-d", distro, "--exec", "/bin/sh", "-c", &start]
        .map(str::to_owned)
        .into()
}

/// Makes the desktop app in WSL2 the one that starts at sign-in, and stops a Windows
/// desktop app that runs: two desktop apps fight over the game folder (SPEC.md 8.4).
pub fn autostart(dirs: &Dirs, exe: &Path, distro: &str) -> Result<()> {
    check_distro(distro)?;
    service::write_run_entry(&run_entry(exe, distro))?;
    make_private_dir(&dirs.data)?;
    service::stop_bridge(&dirs.data)
}

/// Starts `wsl-run` with no console window, and returns at once.
pub fn start_background(exe: &Path, distro: &str) -> Result<()> {
    check_distro(distro)?;
    let mut child = Command::new(exe);
    child
        .args([RUN_COMMAND, distro])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    service::hide_window(&mut child);
    child
        .spawn()
        .context("can't start the desktop app in WSL")?;
    Ok(())
}

/// Runs the desktop app in the distro again each time it ends. A second copy returns
/// at once, so a start from `restart` and one from sign-in never run two loops.
pub fn keep_running(dirs: &Dirs, distro: &str) -> Result<()> {
    check_distro(distro)?;
    let folder = dirs.data.join("wsl");
    make_private_dir(&folder)?;
    let Ok(_lock) = lock::take(&folder) else {
        return Ok(());
    };
    loop {
        let mut wsl = Command::new("wsl.exe");
        wsl.args(wsl_args(distro))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        service::hide_window(&mut wsl);
        // A failed start, for example a missing distro, waits too, so the loop never spins.
        let _ = wsl.status();
        std::thread::sleep(RESTART_DELAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_characters_of_wsl_make_a_distro_name() {
        for good in ["Ubuntu", "Ubuntu-24.04", "my_distro"] {
            assert!(good_distro(good), "{good}");
        }
        let long = "a".repeat(MAX_DISTRO + 1);
        for bad in ["", "Ubuntu --exec x", "a\"b", "a;b", "a&b", long.as_str()] {
            assert!(!good_distro(bad), "{bad}");
        }
    }

    #[test]
    fn the_run_entry_starts_the_launcher_in_the_background() {
        let exe = Path::new(r"C:\Users\A B\AppData\Local\gnomish-relay\bin\gnomish-relay.exe");

        let entry = run_entry(exe, "Ubuntu");

        assert_eq!(
            entry,
            r#""C:\Users\A B\AppData\Local\gnomish-relay\bin\gnomish-relay.exe" wsl-run Ubuntu --background"#
        );
    }

    #[test]
    fn wsl_runs_the_start_file_of_the_home_folder_in_the_distro() {
        let args = wsl_args("Ubuntu");

        assert_eq!(
            args,
            [
                "-d",
                "Ubuntu",
                "--exec",
                "/bin/sh",
                "-c",
                ". \"$HOME/.config/gnomish-relay/wsl-start.sh\""
            ]
        );
    }

    #[test]
    fn a_second_launcher_returns_at_once() {
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            home: root.path().join("home"),
            config: root.path().join("config"),
            data: root.path().join("data"),
        };
        make_private_dir(&dirs.data.join("wsl")).unwrap();
        let _first = lock::take(&dirs.data.join("wsl")).unwrap();

        let second = keep_running(&dirs, "Ubuntu");

        assert!(second.is_ok());
    }

    #[test]
    fn a_bad_distro_name_starts_nothing() {
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            home: root.path().join("home"),
            config: root.path().join("config"),
            data: root.path().join("data"),
        };

        let error = keep_running(&dirs, "a;b").unwrap_err();

        assert!(error.to_string().contains("isn't a WSL distro name"));
        assert!(start_background(Path::new("x"), "a b").is_err());
    }
}
