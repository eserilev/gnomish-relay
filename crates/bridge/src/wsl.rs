//! The desktop app inside WSL2 on Windows, with the game on the Windows side
//! (SPEC.md 11.5).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// WSL mounts each Windows drive here, as `/mnt/c`.
pub const MOUNT_ROOT: &str = "/mnt";
const OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const BINFMT: &str = "/proc/sys/fs/binfmt_misc";
/// WSL with systemd registers the late name.
const INTEROP_ENTRIES: [&str; 2] = ["WSLInterop", "WSLInterop-late"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wsl {
    /// `None` when `WSL_DISTRO_NAME` is not set, for example in a service of the distro.
    pub distro: Option<String>,
}

/// `None` outside WSL.
pub fn detect(osrelease: &str, distro: Option<&str>) -> Option<Wsl> {
    let distro = distro.filter(|d| !d.is_empty());
    let kernel = osrelease.to_ascii_lowercase().contains("microsoft");
    if !kernel && distro.is_none() {
        return None;
    }
    Some(Wsl {
        distro: distro.map(str::to_owned),
    })
}

/// Only Linux can run under WSL.
pub fn this() -> Option<Wsl> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let osrelease = std::fs::read_to_string(OSRELEASE).unwrap_or_default();
    let distro = std::env::var("WSL_DISTRO_NAME").ok();
    detect(&osrelease, distro.as_deref())
}

/// Whether this distro can start Windows programs.
pub fn has_interop(binfmt: &Path) -> bool {
    INTEROP_ENTRIES
        .iter()
        .any(|name| binfmt.join(name).exists())
}

pub fn interop() -> bool {
    this().is_some() && has_interop(Path::new(BINFMT))
}

pub fn status_line(wsl: &Wsl) -> String {
    match &wsl.distro {
        Some(distro) => format!("Running in WSL2 (distro {distro})"),
        None => "Running in WSL2".into(),
    }
}

/// `C:\Games\WoW` or `C:/Games/WoW` as `<root>/c/Games/WoW`. `None` for a path with no
/// drive letter.
pub fn windows_to_wsl(root: &Path, windows: &str) -> Option<PathBuf> {
    let (drive, rest) = windows.split_once(':')?;
    let letter = single_letter(drive)?;
    if !rest.is_empty() && !rest.starts_with(['/', '\\']) {
        return None;
    }
    let parts = rest.split(['/', '\\']).filter(|p| !p.is_empty());
    Some(parts.fold(root.join(letter.to_string()), |path, part| path.join(part)))
}

fn single_letter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    chars.next().is_none().then(|| letter.to_ascii_lowercase())
}

/// The mounted Windows drives, as `<root>/c`. `/mnt/wsl` and `/mnt/wslg` are no drives.
pub fn drives(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_name().to_str().and_then(single_letter).is_some())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found
}

/// The `PATH` with no entry under `root`. A Windows `claude` from npm there runs outside
/// every wall.
pub fn linux_path(path: &OsStr, root: &Path) -> OsString {
    let kept: Vec<PathBuf> = std::env::split_paths(path)
        .filter(|entry| !entry.starts_with(root))
        .collect();
    std::env::join_paths(kept).unwrap_or_default()
}

/// The `PATH` of this process, with no Windows folder under WSL.
pub fn path_var() -> OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    match this() {
        Some(_) => linux_path(&path, Path::new(MOUNT_ROOT)),
        None => path,
    }
}

/// A program of Windows, such as `cmd.exe`, in `Windows/System32` of the first drive
/// that has it. The `PATH` of the bridge holds no Windows folder.
pub fn windows_program(root: &Path, name: &str) -> Option<PathBuf> {
    let places = [
        ["Windows", "System32"].as_slice(),
        ["Windows", "System32", "WindowsPowerShell", "v1.0"].as_slice(),
    ];
    for drive in drives(root) {
        for place in places {
            let path = place
                .iter()
                .fold(drive.clone(), |p, part| p.join(part))
                .join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// The value that `cmd.exe /c echo %NAME%` printed. cmd prints the name itself when the
/// variable is not set.
pub fn echoed_value(output: &str, name: &str) -> Option<String> {
    let value = output.trim();
    let unset = value.is_empty() || value.eq_ignore_ascii_case(&format!("%{name}%"));
    (!unset).then(|| value.to_owned())
}

/// A variable of the Windows user, such as `USERPROFILE`, as a WSL path.
pub fn windows_folder(name: &str) -> Option<PathBuf> {
    if !interop() {
        return None;
    }
    let root = Path::new(MOUNT_ROOT);
    let cmd = windows_program(root, "cmd.exe")?;
    let output = std::process::Command::new(cmd)
        .args(["/d", "/c", &format!("echo %{name}%")])
        // cmd.exe warns about a Linux working folder, and then works in its own.
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let value = echoed_value(&String::from_utf8_lossy(&output.stdout), name)?;
    windows_to_wsl(root, &value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_microsoft_kernel_or_a_distro_name_means_wsl() {
        let wsl2 = "5.15.167.4-microsoft-standard-WSL2";
        assert_eq!(
            detect(wsl2, Some("Ubuntu")),
            Some(Wsl {
                distro: Some("Ubuntu".into())
            })
        );
        assert_eq!(detect(wsl2, None), Some(Wsl { distro: None }));
        assert_eq!(
            detect("6.9.0-arch1-1", Some("Debian")),
            Some(Wsl {
                distro: Some("Debian".into())
            })
        );
        assert_eq!(detect("6.9.0-arch1-1", None), None);
        assert_eq!(detect("6.9.0-arch1-1", Some("")), None);
    }

    #[test]
    fn interop_needs_one_of_the_binfmt_entries_of_wsl() {
        let binfmt = tempfile::tempdir().unwrap();
        assert!(!has_interop(binfmt.path()));
        std::fs::write(binfmt.path().join("WSLInterop-late"), "enabled").unwrap();
        assert!(has_interop(binfmt.path()));
    }

    #[test]
    fn the_status_line_names_the_distro_when_it_is_known() {
        let ubuntu = Wsl {
            distro: Some("Ubuntu".into()),
        };
        assert_eq!(status_line(&ubuntu), "Running in WSL2 (distro Ubuntu)");
        assert_eq!(status_line(&Wsl { distro: None }), "Running in WSL2");
    }

    #[test]
    fn a_windows_path_maps_to_its_drive_under_the_mount_root() {
        let root = Path::new("/mnt");
        let cases = [
            (
                r"C:\Program Files (x86)\World of Warcraft",
                Some("/mnt/c/Program Files (x86)/World of Warcraft"),
            ),
            (
                "D:/Games/World of Warcraft",
                Some("/mnt/d/Games/World of Warcraft"),
            ),
            (r"e:\", Some("/mnt/e")),
            ("C:", Some("/mnt/c")),
            ("/home/x/wow", None),
            (r"\\server\share", None),
            (r"CD:\x", None),
            (r"C:relative", None),
            (r"1:\x", None),
        ];
        for (windows, want) in cases {
            assert_eq!(
                windows_to_wsl(root, windows),
                want.map(PathBuf::from),
                "{windows}"
            );
        }
    }

    #[test]
    fn only_folders_with_one_letter_are_drives() {
        let root = tempfile::tempdir().unwrap();
        for name in ["c", "d", "wsl", "wslg"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        std::fs::write(root.path().join("e"), "").unwrap();

        let found = drives(root.path());

        assert_eq!(found, [root.path().join("c"), root.path().join("d")]);
    }

    /// The code runs only in the Linux of WSL, where paths use `/` and `:`.
    #[cfg(unix)]
    #[test]
    fn the_linux_path_drops_every_windows_folder() {
        let path = OsStr::new(
            "/home/x/.local/bin:/usr/bin:/mnt/c/Users/x/AppData/Roaming/npm:/mnt/c/WINDOWS/system32",
        );

        let kept = linux_path(path, Path::new("/mnt"));

        assert_eq!(kept, OsString::from("/home/x/.local/bin:/usr/bin"));
    }

    #[test]
    fn a_windows_program_is_found_in_system32_of_a_drive() {
        let root = tempfile::tempdir().unwrap();
        let system32 = root.path().join("d/Windows/System32");
        let powershell = system32.join("WindowsPowerShell/v1.0");
        std::fs::create_dir_all(&powershell).unwrap();
        std::fs::create_dir_all(root.path().join("c")).unwrap();
        std::fs::write(system32.join("cmd.exe"), "").unwrap();
        std::fs::write(powershell.join("powershell.exe"), "").unwrap();

        assert_eq!(
            windows_program(root.path(), "cmd.exe"),
            Some(system32.join("cmd.exe"))
        );
        assert_eq!(
            windows_program(root.path(), "powershell.exe"),
            Some(powershell.join("powershell.exe"))
        );
        assert_eq!(windows_program(root.path(), "reg.exe"), None);
    }

    #[test]
    fn an_echo_of_an_unset_variable_gives_nothing() {
        assert_eq!(
            echoed_value("C:\\Users\\x\r\n", "USERPROFILE").as_deref(),
            Some("C:\\Users\\x")
        );
        assert_eq!(echoed_value("%USERPROFILE%\r\n", "USERPROFILE"), None);
        assert_eq!(echoed_value("\r\n", "USERPROFILE"), None);
    }
}
