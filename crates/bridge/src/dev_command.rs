//! `cargo run -- dev` (SPEC.md 16.1): the addon of this checkout in each game, and the
//! desktop app of this build, until Ctrl-C. Then the release install comes back.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::build_kind::BuildKind;
use crate::config;
use crate::dev_mode::{self, Record, Release};
use crate::dirs::Dirs;
use crate::fs_safe::make_private_dir;
use crate::game_choice::NO_WOW;
use crate::lock::{self, Bridge, BridgeLock};
use crate::service::{self, Installed};

/// The folder of the lock of dev mode, in the data folder. The bridge has its own lock.
pub const LOCK_DIR: &str = "dev";
/// How long the desktop app of this build gets to stop after Ctrl-C.
const STOP_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);

pub fn dev(dirs: &Dirs, args: &[&str]) -> Result<()> {
    if BuildKind::THIS.manages_itself() {
        bail!("dev mode works only in a build from source. In your checkout, run cargo run -- dev");
    }
    match args {
        [] => run_dev(dirs),
        ["--end"] => end_only(dirs),
        _ => bail!("usage: cargo run -- dev [--end]"),
    }
}

/// Whether dev mode runs now, in any terminal.
pub fn is_on(data: &Path) -> bool {
    matches!(lock::status(&data.join(LOCK_DIR)), Ok(Bridge::Runs(_)))
}

fn take_lock(data: &Path) -> Result<BridgeLock> {
    let dir = data.join(LOCK_DIR);
    make_private_dir(&dir)?;
    lock::take(&dir)
        .context("dev mode is already on in another terminal. Press Ctrl-C there to stop it")
}

fn run_dev(dirs: &Dirs) -> Result<()> {
    make_private_dir(&dirs.data)?;
    let _lock = take_lock(&dirs.data)?;
    let old = end_old_session(dirs)?;
    let config = config::load(&dirs.config, &dirs.home)?;
    let addons: Vec<PathBuf> = config.games().into_iter().map(|g| g.addons).collect();
    if addons.is_empty() {
        bail!("{NO_WOW}");
    }
    let release = release_now(&dirs.data, old.as_ref())?;
    let record = dev_mode::plan(&addons, release)?;
    dev_mode::write_record(&dirs.data, &record)?;
    let ran = swap_and_run(dirs, &record);
    let ended = end(dirs);
    ran.and(ended)
}

/// A session that crashed stopped the release, so the release ran before it.
fn release_now(data: &Path, old: Option<&Record>) -> Result<Release> {
    if old.is_some_and(|r| r.release == Release::Ran) {
        return Ok(Release::Ran);
    }
    match lock::status(data)? {
        Bridge::Runs(_) => Ok(Release::Ran),
        Bridge::Stopped => Ok(Release::Stopped),
    }
}

fn end_old_session(dirs: &Dirs) -> Result<Option<Record>> {
    let old = dev_mode::end(&dirs.data)?;
    if old.is_some() {
        println!("Dev mode didn't stop cleanly last time, so your installed addon is back first.");
    }
    Ok(old)
}

fn swap_and_run(dirs: &Dirs, record: &Record) -> Result<()> {
    service::stop_bridge(&dirs.data)?;
    let checkout = checkout_addon()?;
    dev_mode::link_transport(&checkout)?;
    for step in dev_mode::start_steps(record) {
        dev_mode::apply(&step, &checkout)?;
    }
    println!(
        "Dev mode is on. WoW loads the addon from {}",
        checkout.display()
    );
    println!("Type /reload in WoW. If WoW doesn't show your changes, restart it.");
    println!("Press Ctrl-C to stop dev mode and go back to your installed version.");
    run_until_stopped()
}

/// `addon/GnomishRelay` of the checkout that this build came from.
fn checkout_addon() -> Result<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addon/GnomishRelay");
    path.canonicalize()
        .with_context(|| format!("cannot find {}", path.display()))
}

fn run_until_stopped() -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .context("cannot catch Ctrl-C")?;
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .arg("run")
        .spawn()
        .context("can't start the desktop app")?;
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() && !stop.load(Ordering::SeqCst) {
                println!("The desktop app stopped ({status}).");
            }
            return Ok(());
        }
        if stop.load(Ordering::SeqCst) {
            return stop_child(&mut child);
        }
        std::thread::sleep(POLL);
    }
}

/// Ctrl-C reaches the child too. A child that is still there after a wait gets killed.
fn stop_child(child: &mut Child) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < STOP_WAIT {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    child.kill()?;
    child.wait()?;
    Ok(())
}

fn end(dirs: &Dirs) -> Result<()> {
    let Some(record) = dev_mode::end(&dirs.data)? else {
        return Ok(());
    };
    println!("Dev mode is off. Your installed addon is back. Type /reload in WoW.");
    if record.release == Release::Ran {
        start_release(dirs);
    }
    Ok(())
}

fn start_release(dirs: &Dirs) {
    match service::start_installed(dirs) {
        Ok(Installed::Started) => println!("Your installed desktop app is running again."),
        Ok(Installed::NoService) => {
            println!("To start your installed desktop app, run gnomish-relay run");
        }
        Err(e) => println!(
            "Couldn't start your installed desktop app ({e:#}). To start it, run gnomish-relay restart"
        ),
    }
}

/// `cargo run -- dev --end`: the end of a session that didn't stop cleanly.
fn end_only(dirs: &Dirs) -> Result<()> {
    make_private_dir(&dirs.data)?;
    let _lock = take_lock(&dirs.data)?;
    if dev_mode::read_record(&dirs.data)?.is_none() {
        println!("Dev mode isn't on.");
        return Ok(());
    }
    end(dirs)
}

/// A release bridge at its start ends a dev session that didn't stop cleanly, for
/// example after a power cut. The child of dev mode is a build from source, so it never
/// ends its own session. Returns the line to log.
pub fn end_left_behind(data: &Path, build: BuildKind) -> Option<String> {
    if !build.manages_itself() || is_on(data) {
        return None;
    }
    match dev_mode::end(data) {
        Ok(Some(_)) => {
            Some("dev mode: a session didn't stop cleanly, so the installed addon is back".into())
        }
        Ok(None) => None,
        Err(e) => Some(format!(
            "dev mode: cannot put the installed addon back: {e:#}"
        )),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    /// A data folder with the record of a session that crashed after its start: the addon
    /// folder of one game is in its backup, and a link stands in its place.
    fn crashed_session(root: &Path) -> (PathBuf, PathBuf) {
        let data = root.join("data");
        let addons = root.join("game/Interface/AddOns");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(addons.join("GnomishRelay")).unwrap();
        std::fs::create_dir_all(root.join("checkout")).unwrap();
        let record = dev_mode::plan(std::slice::from_ref(&addons), Release::Ran).unwrap();
        dev_mode::write_record(&data, &record).unwrap();
        for step in dev_mode::start_steps(&record) {
            dev_mode::apply(&step, &root.join("checkout")).unwrap();
        }
        (data, addons.join("GnomishRelay"))
    }

    fn is_link(path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .unwrap()
            .file_type()
            .is_symlink()
    }

    #[test]
    fn a_release_bridge_puts_back_a_session_that_crashed() {
        let root = tempfile::tempdir().unwrap();
        let (data, addon) = crashed_session(root.path());

        let line = end_left_behind(&data, BuildKind::Release);

        assert!(line.unwrap().contains("the installed addon is back"));
        assert!(!is_link(&addon));
        assert!(!data.join(dev_mode::RECORD_FILE).exists());
    }

    #[test]
    fn the_child_of_dev_mode_leaves_its_own_session() {
        let root = tempfile::tempdir().unwrap();
        let (data, addon) = crashed_session(root.path());

        let line = end_left_behind(&data, BuildKind::Source);

        assert_eq!(line, None);
        assert!(is_link(&addon));
    }

    #[test]
    fn a_release_bridge_leaves_a_session_that_still_runs() {
        let root = tempfile::tempdir().unwrap();
        let (data, addon) = crashed_session(root.path());
        let _dev = take_lock(&data).unwrap();

        let line = end_left_behind(&data, BuildKind::Release);

        assert_eq!(line, None);
        assert!(is_link(&addon));
    }

    #[test]
    fn a_session_that_crashed_with_the_release_running_starts_it_at_the_next_end() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path();
        let old = Record {
            release: Release::Ran,
            games: Vec::new(),
        };

        let now = release_now(data, Some(&old)).unwrap();
        let fresh = release_now(data, None).unwrap();

        assert_eq!(now, Release::Ran);
        assert_eq!(fresh, Release::Stopped);
    }
}
