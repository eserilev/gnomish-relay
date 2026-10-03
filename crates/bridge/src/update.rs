//! `gnomish-relay update`: the program of the latest release in place of this one
//! (SPEC.md 11.3). It uses `curl` and `tar`, which every supported OS has.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use protocol::apps::App;
use semver::Version;
use sha2::{Digest, Sha256};

use crate::app_files::key_addon_name;
use crate::auto_update::{Parts, Wanted, pinned_releases};
use crate::config;
use crate::dirs::Dirs;
use crate::timeways_install;
use crate::timeways_release::NeedsNewerApp;

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
/// (SPEC.md 7.3.2). `relay_addons` holds the `AddOns` folder of each served game, and
/// is empty with the relay off.
pub fn finish_line(relay_addons: &[PathBuf]) -> &'static str {
    let key_addon_is_new = relay_addons
        .iter()
        .any(|addons| !addons.join(key_addon_name(App::Relay)).is_dir());
    if key_addon_is_new {
        "Restart WoW to finish."
    } else {
        "Type /reload in WoW to finish."
    }
}

/// The `AddOns` folder of each served game, with the relay on.
fn relay_addons(dirs: &Dirs) -> Vec<PathBuf> {
    let Ok(config) = config::load(&dirs.config, &dirs.home) else {
        return Vec::new();
    };
    if config.relay.is_none() {
        return Vec::new();
    }
    config.games().into_iter().map(|g| g.addons).collect()
}

/// Which release each part takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// `gnomish-relay update` by hand.
    Latest,
    /// `update --auto`: the version of each addon on disk (SPEC.md 11.3).
    AddonVersions,
}

impl Pick {
    fn timeways_only_args(self) -> &'static [&'static str] {
        match self {
            Pick::Latest => &["update", "--timeways-only"],
            Pick::AddonVersions => &["update", "--timeways-only", "--auto"],
        }
    }
}

/// The Timeways programs that setup installed, from the release that `pick` names
/// (SPEC.md 11.4). A failure prints one line: the desktop app still updates.
fn update_timeways(dirs: &Dirs, pick: Pick) -> Vec<String> {
    let Ok(config) = config::load(&dirs.config, &dirs.home) else {
        return Vec::new();
    };
    let Some(program) = timeways_install::installed_story_program(&config) else {
        return Vec::new();
    };
    let sources = match pick {
        Pick::Latest => timeways_install::Sources::from_env(),
        Pick::AddonVersions => match wanted(dirs).ok().and_then(|w| w.timeways) {
            Some(version) => timeways_install::Sources::pinned(&version),
            None => return Vec::new(),
        },
    };
    match timeways_install::update(dirs, &sources, &program) {
        Ok(changed) => changed,
        Err(e) => {
            println!("{}", update_failed_line(&e));
            Vec::new()
        }
    }
}

fn update_failed_line(error: &anyhow::Error) -> String {
    if let Some(line) = timeways_install::download_failed_line(error, "run gnomish-relay update") {
        return line;
    }
    format!(
        "Timeways: couldn't update the story program. {} To try again, run gnomish-relay update",
        timeways_install::sentence(&format!("{error:#}"))
    )
}

/// Returns whether a Timeways program changed.
fn print_timeways_update(dirs: &Dirs, pick: Pick) -> bool {
    let changed = update_timeways(dirs, pick);
    if changed.is_empty() {
        return false;
    }
    println!("Updated Timeways: {}", changed.join(", "));
    true
}

/// `update --timeways-only`, which `update` runs in the program that it just installed.
pub fn timeways_only(dirs: &Dirs, pick: Pick) -> Result<()> {
    print_timeways_update(dirs, pick);
    Ok(())
}

/// The old program checks a release against the old version range, so it refuses a
/// Timeways that needs the new desktop app.
fn timeways_in_new_program(exe: &Path, pick: Pick) {
    let status = Command::new(exe).args(pick.timeways_only_args()).status();
    if !status.is_ok_and(|s| s.success()) {
        println!(
            "Timeways: couldn't update the story program. To try again, run gnomish-relay update"
        );
    }
}

/// `GNOMISH_URL` wins, as in `install.sh`. With no version, the latest release.
fn relay_releases(version: Option<&Version>) -> String {
    if let Ok(url) = std::env::var("GNOMISH_URL") {
        return url;
    }
    match version {
        Some(version) => pinned_releases("gnomish-relay", version),
        None => RELEASES.to_owned(),
    }
}

/// Installs the latest release in place of `current_exe`, and restarts the bridge.
pub fn self_update(dirs: &Dirs) -> Result<()> {
    update_desktop_app(dirs, &relay_releases(None), Pick::Latest)
}

/// Installs the release in `base` in place of `current_exe`, and restarts the bridge.
fn update_desktop_app(dirs: &Dirs, base: &str, pick: Pick) -> Result<()> {
    let name = archive_name().context("there is no release build for this OS and CPU")?;
    let exe = std::env::current_exe()?;
    let work = dirs.data.join("update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let replaced = fetch(base, &name, &work).and_then(|new| replace(&exe, &new));
    let _ = std::fs::remove_dir_all(&work);
    match (replaced?, pick) {
        (Replaced::New, _) => {
            println!("Updated {}", exe.display());
            timeways_in_new_program(&exe, pick);
        }
        // A wrong release must not try again every minute.
        (Replaced::Same, Pick::AddonVersions) => bail!("the release installed nothing new"),
        (Replaced::Same, Pick::Latest) => {
            if !print_timeways_update(dirs, pick) {
                println!("You already have the latest version.");
                return Ok(());
            }
        }
    }
    restart_bridge(dirs, &exe)
}

fn restart_bridge(dirs: &Dirs, exe: &Path) -> Result<()> {
    // Before the restart: the new bridge writes the key addon at its start.
    let finish = finish_line(&relay_addons(dirs));
    crate::service::restart(dirs, exe)?;
    println!("{finish}");
    Ok(())
}

fn wanted(dirs: &Dirs) -> Result<Wanted> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let parts = Parts::of(dirs, &config).context("auto_update is off in config.toml")?;
    Ok(parts.wanted())
}

/// `update --auto`, which the bridge starts when an addon on disk is newer (SPEC.md 11.3).
pub fn auto_update(dirs: &Dirs) -> Result<()> {
    let wanted = wanted(dirs)?;
    println!("{} auto-update: {wanted:?}", crate::run::now());
    if let Some(version) = &wanted.relay {
        return update_desktop_app(dirs, &relay_releases(Some(version)), Pick::AddonVersions);
    }
    let Some(version) = &wanted.timeways else {
        println!("Nothing to update.");
        return Ok(());
    };
    let exe = std::env::current_exe()?;
    match auto_update_timeways(dirs, version) {
        Ok(()) => restart_bridge(dirs, &exe),
        // The latest desktop app knows the new range, and installs Timeways itself.
        Err(e) if e.is::<NeedsNewerApp>() => {
            update_desktop_app(dirs, &relay_releases(None), Pick::AddonVersions)
        }
        Err(e) => Err(e),
    }
}

fn auto_update_timeways(dirs: &Dirs, version: &Version) -> Result<()> {
    let config = config::load(&dirs.config, &dirs.home)?;
    let program = timeways_install::installed_story_program(&config)
        .context("setup didn't install the story program")?;
    let changed =
        timeways_install::update(dirs, &timeways_install::Sources::pinned(version), &program)?;
    if changed.is_empty() {
        bail!("Timeways {version} installed nothing new");
    }
    println!("Updated Timeways: {}", changed.join(", "));
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
        let games = [addons.path().to_owned()];
        assert_eq!(finish_line(&games), "Restart WoW to finish.");
        fs::create_dir(addons.path().join("GnomishRelay_Key")).unwrap();
        assert_eq!(finish_line(&games), "Type /reload in WoW to finish.");
        assert_eq!(finish_line(&[]), "Type /reload in WoW to finish.");
    }

    #[test]
    fn an_update_asks_for_a_restart_when_any_served_game_lacks_the_key_addon() {
        let forever = tempfile::tempdir().unwrap();
        let anniversary = tempfile::tempdir().unwrap();
        fs::create_dir(forever.path().join("GnomishRelay_Key")).unwrap();

        let games = [forever.path().to_owned(), anniversary.path().to_owned()];

        assert_eq!(finish_line(&games), "Restart WoW to finish.");
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
