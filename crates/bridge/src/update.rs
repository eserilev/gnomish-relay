//! `gnomish-relay update`: the program of the latest release in place of this one
//! (SPEC.md 11.3). It uses `curl` and `tar`, which every supported OS has.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

pub const RELEASES: &str = "https://github.com/eserilev/gnomish-relay/releases/latest/download";

/// The archive that `scripts/package.sh` makes for this OS and CPU.
pub fn archive_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("gnomish-relay-x86_64-unknown-linux-gnu.tar.gz"),
        ("macos", "aarch64") => Some("gnomish-relay-aarch64-apple-darwin.tar.gz"),
        ("macos", "x86_64") => Some("gnomish-relay-x86_64-apple-darwin.tar.gz"),
        ("windows", "x86_64") => Some("gnomish-relay-x86_64-pc-windows-msvc.zip"),
        _ => None,
    }
}

fn program_name() -> String {
    format!("gnomish-relay{}", std::env::consts::EXE_SUFFIX)
}

/// The sum in a `sha256sum` line: 64 hex digits, then the file name.
pub fn parse_sum(line: &str) -> Option<[u8; 32]> {
    let hex = line.split_whitespace().next()?;
    if hex.len() != 64 {
        return None;
    }
    let mut sum = [0u8; 32];
    for (byte, pair) in sum.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(sum)
}

fn tool(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed", args.join(" "));
    }
    Ok(())
}

fn download(url: &str, to: &Path) -> Result<()> {
    tool("curl", &["-fsSL", url, "-o", &to.to_string_lossy()])
}

/// Downloads the archive `name` from `base` into `dir`, checks its sum, and unpacks it.
/// Returns the path of the new program.
pub fn fetch(base: &str, name: &str, dir: &Path) -> Result<PathBuf> {
    let archive = dir.join(name);
    let sum_file = dir.join(format!("{name}.sha256"));
    download(&format!("{base}/{name}"), &archive)?;
    download(&format!("{base}/{name}.sha256"), &sum_file)?;
    // The sum comes from the same release. It finds a broken download, not a changed release.
    let want = parse_sum(&fs::read_to_string(&sum_file)?).context("the .sha256 file is damaged")?;
    let have: [u8; 32] = Sha256::digest(fs::read(&archive)?).into();
    if have != want {
        bail!("the download of {name} has a wrong SHA-256 sum");
    }
    // The tar of Windows (bsdtar) also unpacks a zip.
    tool(
        "tar",
        &[
            "-xf",
            &archive.to_string_lossy(),
            "-C",
            &dir.to_string_lossy(),
        ],
    )?;
    let program = dir.join(program_name());
    if !program.is_file() {
        bail!("{name} has no {}", program_name());
    }
    Ok(program)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Replaced {
    Same,
    New,
}

/// Puts the program `new` in place of `exe`. A running copy keeps its old file.
pub fn replace(exe: &Path, new: &Path) -> Result<Replaced> {
    if fs::read(exe)? == fs::read(new)? {
        return Ok(Replaced::Same);
    }
    let dir = exe.parent().context("the program has no folder")?;
    let name = exe.file_name().context("the program has no name")?;
    let staged = dir.join(format!("{}.new", name.to_string_lossy()));
    let old = dir.join(format!("{}.old", name.to_string_lossy()));
    // A copy, because the new file can be on another disk, and a rename cannot cross disks.
    fs::copy(new, &staged).with_context(|| format!("cannot write {}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    }
    // Windows refuses to replace a running program, but it lets us rename it.
    let _ = fs::remove_file(&old);
    if cfg!(windows) {
        fs::rename(exe, &old)?;
    }
    if let Err(e) = fs::rename(&staged, exe) {
        let _ = fs::rename(&old, exe);
        return Err(e).with_context(|| format!("cannot replace {}", exe.display()));
    }
    Ok(Replaced::New)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    const SUM_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn file_url(dir: &Path) -> String {
        let path = dir.to_string_lossy().replace('\\', "/");
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    /// A release folder with an archive that holds `program`, and its sum.
    fn release(program: &[u8]) -> (tempfile::TempDir, String) {
        let release = tempfile::tempdir().unwrap();
        let build = tempfile::tempdir().unwrap();
        fs::write(build.path().join(program_name()), program).unwrap();
        let name = "test.tar.gz";
        let archive = release.path().join(name);
        tool(
            "tar",
            &[
                "-czf",
                &archive.to_string_lossy(),
                "-C",
                &build.path().to_string_lossy(),
                &program_name(),
            ],
        )
        .unwrap();
        let sum: [u8; 32] = Sha256::digest(fs::read(&archive).unwrap()).into();
        let mut hex = String::new();
        for b in sum {
            let _ = write!(hex, "{b:02x}");
        }
        fs::write(
            release.path().join(format!("{name}.sha256")),
            format!("{hex}  {name}\n"),
        )
        .unwrap();
        (release, name.to_owned())
    }

    // CI runs the tests on each OS of the release job.
    #[test]
    fn this_os_has_a_release_archive() {
        assert!(archive_name().is_some());
    }

    #[test]
    fn a_sum_line_gives_the_32_bytes_of_the_sum() {
        let sum = parse_sum(&format!("{SUM_OF_ABC}  abc.tar.gz\n")).unwrap();
        assert_eq!(sum, <[u8; 32]>::from(Sha256::digest(b"abc")));
    }

    #[test]
    fn a_short_or_bad_sum_line_is_refused() {
        assert_eq!(parse_sum(""), None);
        assert_eq!(parse_sum("abcd  x"), None);
        assert_eq!(parse_sum(&SUM_OF_ABC.replace('b', "g")), None);
    }

    #[test]
    fn fetch_unpacks_the_program_of_a_release() {
        let (release, name) = release(b"new program");
        let work = tempfile::tempdir().unwrap();
        let program = fetch(&file_url(release.path()), &name, work.path()).unwrap();
        assert_eq!(fs::read(program).unwrap(), b"new program");
    }

    #[test]
    fn fetch_refuses_an_archive_with_a_wrong_sum() {
        let (release, name) = release(b"new program");
        fs::write(release.path().join(&name), b"changed").unwrap();
        let work = tempfile::tempdir().unwrap();
        let error = fetch(&file_url(release.path()), &name, work.path())
            .err()
            .unwrap();
        assert!(error.to_string().contains("wrong SHA-256"), "{error}");
    }

    #[test]
    fn fetch_fails_when_the_release_is_missing() {
        let release = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        assert!(fetch(&file_url(release.path()), "none.tar.gz", work.path()).is_err());
    }

    #[test]
    fn replace_puts_the_new_program_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(program_name());
        let new = dir.path().join("download");
        fs::write(&exe, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        assert_eq!(replace(&exe, &new).unwrap(), Replaced::New);
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn replace_keeps_a_program_that_is_already_the_latest() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(program_name());
        let new = dir.path().join("download");
        fs::write(&exe, b"same").unwrap();
        fs::write(&new, b"same").unwrap();
        assert_eq!(replace(&exe, &new).unwrap(), Replaced::Same);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
