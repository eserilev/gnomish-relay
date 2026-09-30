//! `gnomish-relay update`: the program of the latest release in place of this one
//! (SPEC.md 11.3). It uses `curl` and `tar`, which every supported OS has.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use protocol::apps::App;
use sha2::{Digest, Sha256};

use crate::app_files::key_addon_name;
use crate::config;
use crate::dirs::Dirs;
use crate::install;
use crate::timeways_install;

pub const RELEASES: &str = "https://github.com/eserilev/gnomish-relay/releases/latest/download";

/// The target of the release builds for this OS and CPU. Timeways uses the same names.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

/// The archive that `scripts/package.sh` makes for this OS and CPU.
pub fn archive_name() -> Option<String> {
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    target().map(|target| format!("gnomish-relay-{target}.{extension}"))
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

pub fn tool(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed", args.join(" "));
    }
    Ok(())
}

pub fn download(url: &str, to: &Path) -> Result<()> {
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

/// Puts the program `new` at `target`, in place of an older one or as a first install.
pub fn install_program(target: &Path, new: &Path) -> Result<Replaced> {
    if target.exists() {
        return replace(target, new);
    }
    let dir = target.parent().context("the program has no folder")?;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    let staged = target.with_extension("new");
    fs::copy(new, &staged).with_context(|| format!("cannot write {}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&staged, target).with_context(|| format!("cannot write {}", target.display()))?;
    Ok(Replaced::New)
}

/// A key addon that is new since the launch of the game loads only after a restart
/// (SPEC.md 7.3.2). `relay_addons` is `None` with the relay off.
pub fn finish_line(relay_addons: Option<&Path>) -> &'static str {
    let key_addon_is_new =
        relay_addons.is_some_and(|addons| !addons.join(key_addon_name(App::Relay)).is_dir());
    if key_addon_is_new {
        "Restart WoW to finish."
    } else {
        "Type /reload in WoW to finish."
    }
}

/// The `AddOns` folder of a config with the relay.
fn relay_addons(dirs: &Dirs) -> Option<PathBuf> {
    let config = config::load(&dirs.config, &dirs.home).ok()?;
    config.relay.as_ref()?;
    Some(install::addons_dir(config.wow.as_deref()?))
}

/// The Timeways programs that setup installed, from the latest Timeways release
/// (SPEC.md 11.4). A failure prints one line: the desktop app still updates.
fn update_timeways(dirs: &Dirs) -> Vec<String> {
    let Ok(config) = config::load(&dirs.config, &dirs.home) else {
        return Vec::new();
    };
    let Some(program) = timeways_install::installed_story_program(&config) else {
        return Vec::new();
    };
    let sources = timeways_install::Sources::from_env();
    match timeways_install::update(dirs, &sources, &program) {
        Ok(changed) => changed,
        Err(e) => {
            println!("{}", update_failed_line(&e));
            Vec::new()
        }
    }
}

fn update_failed_line(error: &anyhow::Error) -> String {
    format!(
        "Timeways: couldn't update the story program. {} To try again, run gnomish-relay update",
        timeways_install::sentence(&format!("{error:#}"))
    )
}

/// Returns whether a Timeways program changed.
fn print_timeways_update(dirs: &Dirs) -> bool {
    let changed = update_timeways(dirs);
    if changed.is_empty() {
        return false;
    }
    println!("Updated Timeways: {}", changed.join(", "));
    true
}

/// `update --timeways-only`, which `update` runs in the program that it just installed.
pub fn timeways_only(dirs: &Dirs) -> Result<()> {
    print_timeways_update(dirs);
    Ok(())
}

/// The old program checks a release against the old version range, so it refuses a
/// Timeways that needs the new desktop app.
fn timeways_in_new_program(exe: &Path) {
    let status = Command::new(exe)
        .args(["update", "--timeways-only"])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        println!(
            "Timeways: couldn't update the story program. To try again, run gnomish-relay update"
        );
    }
}

/// Installs the latest release in place of `current_exe`, and restarts the bridge.
pub fn self_update(dirs: &Dirs) -> Result<()> {
    let name = archive_name().context("there is no release build for this OS and CPU")?;
    let base = std::env::var("GNOMISH_URL").unwrap_or_else(|_| RELEASES.to_owned());
    let exe = std::env::current_exe()?;
    let work = dirs.data.join("update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let replaced = fetch(&base, &name, &work).and_then(|new| replace(&exe, &new));
    let _ = std::fs::remove_dir_all(&work);
    match replaced? {
        Replaced::New => {
            println!("Updated {}", exe.display());
            timeways_in_new_program(&exe);
        }
        Replaced::Same => {
            if !print_timeways_update(dirs) {
                println!("You already have the latest version.");
                return Ok(());
            }
        }
    }
    // Before the restart: the new bridge writes the key addon at its start.
    let finish = finish_line(relay_addons(dirs).as_deref());
    crate::service::restart(dirs, &exe)?;
    println!("{finish}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_timeways_update_gives_the_error_and_then_the_next_step() {
        let error = anyhow::anyhow!("the Timeways manifest is damaged");

        assert_eq!(
            update_failed_line(&error),
            "Timeways: couldn't update the story program. The Timeways manifest is damaged. To try again, run gnomish-relay update"
        );
    }

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
        fs::write(
            release.path().join(format!("{name}.sha256")),
            format!("{}  {name}\n", crate::ids::hex(&sum)),
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
    fn an_update_asks_for_a_restart_only_when_the_key_addon_is_new() {
        let addons = tempfile::tempdir().unwrap();
        assert_eq!(finish_line(Some(addons.path())), "Restart WoW to finish.");
        fs::create_dir(addons.path().join("GnomishRelay_Key")).unwrap();
        assert_eq!(
            finish_line(Some(addons.path())),
            "Type /reload in WoW to finish."
        );
        assert_eq!(finish_line(None), "Type /reload in WoW to finish.");
    }

    #[test]
    fn a_first_install_puts_the_program_into_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("download");
        fs::write(&new, b"story").unwrap();
        let target = dir.path().join("bin").join("timeways-story");

        assert_eq!(install_program(&target, &new).unwrap(), Replaced::New);
        assert_eq!(install_program(&target, &new).unwrap(), Replaced::Same);

        assert_eq!(fs::read(&target).unwrap(), b"story");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
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
