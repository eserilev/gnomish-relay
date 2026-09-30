//! How the bridge runs in the background: the login service of each OS, a restart,
//! and a process with no service (SPEC.md 11.3).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config;
use crate::dirs::Dirs;
use crate::fs_safe::{LogStart, make_private_dir, open_private_log, write_atomic, write_private};
use crate::install::{self, SYSTEMD_UNIT};
use crate::lock::{self, Bridge};
use crate::wsl::{self, Wsl};
use crate::wsl_launcher;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
/// Big enough for weeks of normal logs. A bigger log starts again.
const MAX_LOG: u64 = 4 * 1024 * 1024;
const LOG_FILE: &str = "bridge.log";

fn command(program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed", args.join(" "));
    }
    Ok(())
}

/// The Run key of the user needs no admin rights, unlike a scheduled task. The desktop
/// app of Windows and the one in WSL2 share one entry, so only one starts.
pub fn write_run_entry(value: &str) -> Result<()> {
    command(
        "reg",
        &[
            "add",
            RUN_KEY,
            "/v",
            "Gnomish Relay",
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ],
    )
}

/// Starts the bridge at each login, and now (SPEC.md 11.3).
pub fn autostart(dirs: &Dirs) -> Result<()> {
    let exe = std::env::current_exe()?;
    if let Some(wsl) = wsl::this() {
        let distro = wsl_distro(&wsl)?;
        let windows = windows_app()?;
        command(
            &windows.to_string_lossy(),
            &[wsl_launcher::AUTOSTART_COMMAND, distro],
        )?;
        return restart(dirs, &exe);
    }
    if cfg!(windows) {
        write_run_entry(&format!("\"{}\" run --background", exe.display()))?;
        restart_process(dirs, &exe)?;
    } else if cfg!(target_os = "macos") {
        let log = load_launchd_agent(dirs, &exe)?;
        println!("Log: {}", log.display());
    } else {
        write_systemd_unit(dirs, &exe)?;
        command("systemctl", &["--user", "enable", SYSTEMD_UNIT])?;
        command("systemctl", &["--user", "restart", SYSTEMD_UNIT])?;
        println!("logs: journalctl --user -u gnomish-relay");
    }
    Ok(())
}

/// A service starts with almost no `PATH`, so it gets the one of this shell, and finds
/// the agents that the shell finds.
fn write_systemd_unit(dirs: &Dirs, exe: &Path) -> Result<()> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let xdg = xdg_vars();
    let dir = systemd_dir(dirs);
    std::fs::create_dir_all(&dir)?;
    write_atomic(
        &dir,
        SYSTEMD_UNIT,
        install::systemd_unit(exe, &path_var, &xdg).as_bytes(),
    )?;
    command("systemctl", &["--user", "daemon-reload"])
}

/// Writes the launchd agent with the `PATH` of this shell, and starts it. Returns its log.
fn load_launchd_agent(dirs: &Dirs, exe: &Path) -> Result<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let dir = launch_agents_dir(dirs);
    std::fs::create_dir_all(&dir)?;
    let name = format!("{}.plist", install::LAUNCHD_LABEL);
    let log = launchd_log(dirs);
    write_atomic(
        &dir,
        &name,
        install::launchd_plist(exe, &path_var, &log).as_bytes(),
    )?;
    let domain = launchd_domain()?;
    let plist = dir.join(&name).to_string_lossy().into_owned();
    let _ = command("launchctl", &["bootout", &domain, &plist]);
    command("launchctl", &["bootstrap", &domain, &plist])?;
    Ok(log)
}

fn systemd_dir(dirs: &Dirs) -> PathBuf {
    install::systemd_dir(&dirs.home)
}

fn launch_agents_dir(dirs: &Dirs) -> PathBuf {
    dirs.home.join("Library").join("LaunchAgents")
}

fn launchd_domain() -> Result<String> {
    let uid = std::process::Command::new("id").arg("-u").output()?.stdout;
    Ok(format!("gui/{}", String::from_utf8(uid)?.trim()))
}

/// Where the bridge of each kind of start writes its log.
enum BridgeLog {
    Journal,
    File(PathBuf),
}

/// Restarts the bridge through the login service of setup, or as a process with no
/// service. `exe` is the program to start: after an update, `current_exe` names the
/// old file.
pub fn restart(dirs: &Dirs, exe: &Path) -> Result<()> {
    // A service restart succeeds even when the new bridge stops at once on a bad config.
    config::load(&dirs.config, &dirs.home).context(
        "config.toml has an error, so the desktop app can't start. Fix it, then run gnomish-relay restart",
    )?;
    let before = lock::status(&dirs.data)?;
    let log = restart_service(dirs, exe)?;
    confirm_start(dirs, &log, &before)
}

fn restart_service(dirs: &Dirs, exe: &Path) -> Result<BridgeLog> {
    if let Some(wsl) = wsl::this() {
        return restart_in_wsl(dirs, exe, &wsl).map(BridgeLog::File);
    }
    if cfg!(target_os = "linux") && systemd_dir(dirs).join(SYSTEMD_UNIT).is_file() {
        write_systemd_unit(dirs, exe)?;
        command("systemctl", &["--user", "restart", SYSTEMD_UNIT])?;
        return Ok(BridgeLog::Journal);
    }
    let plist = launch_agents_dir(dirs).join(format!("{}.plist", install::LAUNCHD_LABEL));
    if cfg!(target_os = "macos") && plist.is_file() {
        return load_launchd_agent(dirs, exe).map(BridgeLog::File);
    }
    restart_process(dirs, exe).map(BridgeLog::File)
}

/// `WSL_DISTRO_NAME` names the distro for the `Run` entry of Windows.
fn wsl_distro(wsl: &Wsl) -> Result<&str> {
    wsl.distro.as_deref().context(
        "WSL_DISTRO_NAME isn't set, so Windows can't start the desktop app. Run gnomish-relay restart from a WSL terminal",
    )
}

/// The Windows program that starts the desktop app in WSL2, from the Windows installer.
fn windows_app() -> Result<PathBuf> {
    let missing =
        "the Windows part of Gnomish Relay is missing. Run the Windows installer in PowerShell";
    let local = wsl::windows_folder("LOCALAPPDATA").context(missing)?;
    let exe = local
        .join("gnomish-relay")
        .join("bin")
        .join("gnomish-relay.exe");
    if !exe.is_file() {
        bail!("{missing}");
    }
    Ok(exe)
}

/// The Windows launcher starts the desktop app again after a stop. A second launcher
/// exits at its lock, so this call is safe while one runs (SPEC.md 11.5).
fn restart_in_wsl(dirs: &Dirs, exe: &Path, wsl: &Wsl) -> Result<PathBuf> {
    let distro = wsl_distro(wsl)?;
    write_wsl_start(dirs, exe)?;
    let windows = windows_app()?;
    make_private_dir(&dirs.data)?;
    stop_bridge(&dirs.data)?;
    command(
        &windows.to_string_lossy(),
        &[
            wsl_launcher::RUN_COMMAND,
            distro,
            wsl_launcher::BACKGROUND_FLAG,
        ],
    )?;
    Ok(dirs.data.join(LOG_FILE))
}

/// With the `PATH` of this shell, less its Windows folders, as for the systemd unit.
fn write_wsl_start(dirs: &Dirs, exe: &Path) -> Result<()> {
    let path_var = wsl::path_var().to_string_lossy().into_owned();
    let file = dirs.home.join(install::WSL_START_FILE);
    let folder = file.parent().context("the start file has no folder")?;
    make_private_dir(folder)?;
    let script = install::wsl_start_script(exe, &path_var, &xdg_vars());
    write_private(folder, "wsl-start.sh", &script)
}

fn xdg_vars() -> Vec<(&'static str, String)> {
    install::XDG_VARS
        .iter()
        .filter_map(|name| Some((*name, std::env::var(name).ok()?)))
        .collect()
}

fn launchd_log(dirs: &Dirs) -> PathBuf {
    dirs.home
        .join("Library")
        .join("Logs")
        .join("gnomish-relay.log")
}

/// A bridge that stops at start holds the lock only for a moment, so the check waits a
/// second after the lock and looks again. `before` is the bridge from before the
/// restart: a bridge started by hand keeps the lock, and the new one never starts.
fn confirm_start(dirs: &Dirs, log: &BridgeLog, before: &Bridge) -> Result<()> {
    let data = &dirs.data;
    let bridge = if lock::wait_until_runs(data, std::time::Duration::from_secs(10))? {
        std::thread::sleep(std::time::Duration::from_secs(1));
        lock::status(data)?
    } else {
        Bridge::Stopped
    };
    if let Bridge::Runs(Some(pid)) = bridge
        && *before == bridge
    {
        bail!(
            "another copy of the desktop app (process {pid}) is still running. Stop it, then run gnomish-relay restart"
        );
    }
    if bridge != Bridge::Stopped {
        println!("The desktop app is running.");
        return Ok(());
    }
    match last_log_line(log) {
        Some(line) => bail!("the desktop app didn't start. Its last log line: {line}"),
        None => bail!("the desktop app didn't start, and its log is empty"),
    }
}

fn last_log_line(log: &BridgeLog) -> Option<String> {
    let text = match log {
        BridgeLog::Journal => {
            let args = [
                "--user",
                "-u",
                SYSTEMD_UNIT,
                "-n",
                "1",
                "--no-pager",
                "-o",
                "cat",
            ];
            let out = std::process::Command::new("journalctl")
                .args(args)
                .output()
                .ok()?;
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
        BridgeLog::File(path) => std::fs::read_to_string(path).ok()?,
    };
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

/// A bridge that runs with no service gets stopped, and then `exe` starts in the
/// background. Returns the log file.
fn restart_process(dirs: &Dirs, exe: &Path) -> Result<PathBuf> {
    make_private_dir(&dirs.data)?;
    stop_bridge(&dirs.data)?;
    let log = start_background(dirs, exe)?;
    println!("Log: {}", log.display());
    Ok(log)
}

/// Stops the bridge of the data folder `data`, if one runs, and waits for its lock.
pub fn stop_bridge(data: &Path) -> Result<()> {
    match lock::status(data)? {
        Bridge::Stopped => {}
        Bridge::Runs(None) => {
            bail!(
                "the desktop app is running, but its process id is unknown. Stop it yourself, then run gnomish-relay restart"
            )
        }
        Bridge::Runs(Some(pid)) => stop_process(pid)?,
    }
    if !lock::wait_until_stopped(data, std::time::Duration::from_secs(10))? {
        bail!("the desktop app didn't stop");
    }
    Ok(())
}

fn stop_process(pid: u32) -> Result<()> {
    let pid = pid.to_string();
    if cfg!(windows) {
        // `/T` also stops the agents that the bridge started.
        command("taskkill", &["/PID", &pid, "/T", "/F"])
    } else {
        command("kill", &[&pid])
    }
}

/// Starts `run` as a new process with no console window, and its log in a file.
pub fn start_background(dirs: &Dirs, exe: &Path) -> Result<PathBuf> {
    let (_child, log) = spawn_logged(dirs, exe)?;
    Ok(log)
}

/// `run` with its log in a file, as `start_background`, but it waits and gives the exit
/// status. The launcher of WSL2 needs a process that lives as long as the bridge.
pub fn run_logged(dirs: &Dirs, exe: &Path) -> Result<i32> {
    let (mut child, _log) = spawn_logged(dirs, exe)?;
    let status = child.wait().context("the desktop app didn't run")?;
    Ok(status.code().unwrap_or(1))
}

/// Keeps a program that the bridge starts from opening a console window on Windows.
pub fn hide_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn spawn_logged(dirs: &Dirs, exe: &Path) -> Result<(std::process::Child, PathBuf)> {
    make_private_dir(&dirs.data)?;
    let log_path = dirs.data.join(LOG_FILE);
    let start = if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > MAX_LOG) {
        LogStart::Fresh
    } else {
        LogStart::Append
    };
    let log = open_private_log(&log_path, start)?;
    let mut child = std::process::Command::new(exe);
    child
        .arg("run")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    hide_window(&mut child);
    let child = child.spawn().context("can't start the desktop app")?;
    Ok((child, log_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(root: &Path) -> Dirs {
        Dirs {
            home: root.join("home"),
            config: root.join("config").join("gnomish-relay"),
            data: root.join("data"),
        }
    }

    #[test]
    fn the_last_log_line_skips_the_blank_lines_at_the_end() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("bridge.log");
        std::fs::write(&log, "first\nlast words\n\n  \n").unwrap();

        let line = last_log_line(&BridgeLog::File(log));

        assert_eq!(line.as_deref(), Some("last words"));
    }

    #[test]
    fn a_missing_log_has_no_last_line() {
        let root = tempfile::tempdir().unwrap();
        let log = BridgeLog::File(root.path().join("none.log"));
        assert_eq!(last_log_line(&log), None);
    }

    #[test]
    fn the_systemd_unit_lies_where_the_user_manager_reads_it_also_with_xdg_config_home() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());

        let dir = systemd_dir(&dirs);

        assert_eq!(dir, root.path().join("home/.config/systemd/user"));
        let unit = dir.join(SYSTEMD_UNIT);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&unit, "").unwrap();
        assert_eq!(install::service_file(&dirs.home), Some(unit));
    }

    /// `sh run` fails at once, which is enough: only the log matters here.
    #[cfg(unix)]
    #[test]
    fn a_background_start_makes_the_data_folder_and_starts_a_big_log_again() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        std::fs::create_dir_all(&dirs.data).unwrap();
        let big = vec![b'x'; usize::try_from(MAX_LOG).unwrap() + 1];
        std::fs::write(dirs.data.join("bridge.log"), big).unwrap();

        let log = start_background(&dirs, Path::new("/bin/sh")).unwrap();

        assert_eq!(log, dirs.data.join("bridge.log"));
        assert!(std::fs::metadata(&log).unwrap().len() < MAX_LOG);
    }

    /// `sh run` fails at once with 127: no file `run` in the working folder.
    #[cfg(unix)]
    #[test]
    fn a_logged_run_waits_and_gives_the_exit_status() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());

        let status = run_logged(&dirs, Path::new("/bin/sh")).unwrap();

        assert_eq!(status, 127);
        assert!(dirs.data.join(LOG_FILE).is_file());
    }

    #[test]
    fn a_confirm_fails_when_the_old_bridge_still_holds_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        make_private_dir(&dirs.data).unwrap();
        let _old = lock::take(&dirs.data).unwrap();
        let before = lock::status(&dirs.data).unwrap();
        let log = BridgeLog::File(root.path().join("none.log"));

        let error = confirm_start(&dirs, &log, &before).unwrap_err();

        let pid = std::process::id();
        assert!(
            error.to_string().contains(&format!(
                "another copy of the desktop app (process {pid}) is still running"
            )),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_restart_refuses_a_bridge_whose_process_id_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        make_private_dir(&dirs.data).unwrap();
        let _running = lock::take(&dirs.data).unwrap();
        std::fs::remove_file(dirs.data.join("bridge.pid")).unwrap();

        let error = restart_process(&dirs, Path::new("/bin/sh")).unwrap_err();

        assert!(
            error.to_string().contains("process id is unknown"),
            "{error}"
        );
    }
}
